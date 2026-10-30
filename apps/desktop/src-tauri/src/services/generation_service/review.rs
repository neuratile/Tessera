//! One-shot review generation seam (#109). No persistence lifecycle or coordinator.
//! Unlike artifact generation, no retry, normalization, salvage or repair is allowed.
mod input;
mod output;
use input::{fit, metadata_sources, prepare};
#[cfg(test)]
mod tests;

use crate::{
    error::{AppError, AppResult},
    prompts::review_v1,
    providers::{
        factory::{self, ProviderConfig},
        llm::{GenerateRequest, LlmError, LlmProvider, Message},
    },
    repositories::{
        provider_config_repo,
        review_repo::{
            ReviewExclusion, ReviewFinding, ReviewProvenance, ReviewProvider, ReviewSnapshot,
            ReviewTruncation,
        },
    },
    services::{
        git_change_service::{StagedChangeSet, SupportingCapture},
        provider_config_service,
        review_context_service::{self, ContextBudget, ReviewContext},
    },
    utils::{crypto::CryptoKey, secret_redaction::redact_source},
};
use futures::StreamExt;
use serde_json::Value;
use sqlx::SqlitePool;
use std::{future::Future, sync::Arc, time::Duration};
use uuid::Uuid;

/// All references are to immutable local captures and the persisted manifest.
pub struct ReviewRequest<'a> {
    /// Explicit connection pick; None fails before constructing a provider.
    pub provider_config_id: Option<Uuid>,
    /// Explicit model choice, pinned for this one call.
    pub model: &'a str,
    /// Owned capture from #106, never working-tree text.
    pub capture: &'a StagedChangeSet,
    /// Snapshot-bound optional context from #108.
    pub support: &'a SupportingCapture,
    /// Persisted #107 source identity, independently validated.
    pub manifest: &'a ReviewSnapshot,
}

/// Validated data for a later coordinator to persist; no reproduced evidence.
pub struct ReviewGenerationOutcome {
    pub findings: Vec<ReviewFinding>,
    pub provenance: ReviewProvenance,
    /// Final omissions, including redaction and full-wrapper budget selection.
    /// Append to `ReviewSession.excluded_files` before publishing the result.
    pub excluded_files: Vec<ReviewExclusion>,
    pub truncation: Vec<ReviewTruncation>,
}

/// Generate findings through the sole LLM service path with a pinned active config.
/// Cancellation drops the in-flight stream. One 120-second deadline includes config
/// lookup and stream drain; no automatic provider fallback or additional call.
/// # Errors
/// Rejects inactive/missing picks, invalid context/citations/JSON and incomplete or
/// over-limit streams. Errors carry existing recovery hints, never payload previews.
pub async fn generate_review(
    pool: &SqlitePool,
    crypto: &CryptoKey,
    request: ReviewRequest<'_>,
    cancelled: impl Future<Output = ()> + Send,
) -> AppResult<ReviewGenerationOutcome> {
    run(
        pool,
        crypto,
        request,
        cancelled,
        Duration::from_secs(120),
        factory::build_llm_provider,
    )
    .await
}

async fn run(
    pool: &SqlitePool,
    crypto: &CryptoKey,
    request: ReviewRequest<'_>,
    cancelled: impl Future<Output = ()> + Send,
    timeout: Duration,
    build: impl FnOnce(&ProviderConfig) -> Result<Arc<dyn LlmProvider>, LlmError>,
) -> AppResult<ReviewGenerationOutcome> {
    tokio::select! {
        biased;
        () = cancelled => Err(AppError::InvalidInput("Review cancelled; start a new review when ready".into())),
        result = tokio::time::timeout(timeout, generate_once(pool, crypto, request, build)) =>
            result.map_err(|_| AppError::LimitExceeded("Review timed out; start a new review".into()))?,
    }
}

async fn generate_once(
    pool: &SqlitePool,
    crypto: &CryptoKey,
    request: ReviewRequest<'_>,
    build: impl FnOnce(&ProviderConfig) -> Result<Arc<dyn LlmProvider>, LlmError>,
) -> AppResult<ReviewGenerationOutcome> {
    let id = request
        .provider_config_id
        .ok_or_else(|| AppError::InvalidInput("Select an active connection in Settings".into()))?;
    if !safe_text(request.model) {
        return Err(AppError::InvalidInput("Select a valid model".into()));
    }
    let (llm, provider) = pin_provider(pool, crypto, id, build).await?;
    let mut context = prepare(&request, llm.as_ref())?;
    let schema = review_v1::tool_schema();
    let messages = fit(&request, &mut context, llm.as_ref(), &schema)?;
    let capabilities = llm.capabilities();
    let output_cap = capabilities.max_output_tokens.min(6_000);
    if output_cap == 0 {
        return Err(AppError::LimitExceeded("Model has no output budget".into()));
    }
    let mut stream = llm.stream(GenerateRequest {
        model: request.model.into(),
        messages,
        tools: if capabilities.supports_tools {
            vec![schema.clone()]
        } else {
            vec![]
        },
        temperature: Some(0.0),
        max_tokens: Some(output_cap),
        stop_sequences: vec![],
    });
    let mut collector = output::Collector::default();
    while let Some(chunk) = stream.next().await {
        collector.push(
            chunk.map_err(|e| output::safe_provider_error(&e))?,
            llm.name(),
            output_cap,
        )?;
    }
    let (payload, usage) = collector.finish(llm.name())?;
    let findings = output::validate(
        &payload,
        &schema,
        request.manifest,
        &context,
        &metadata_sources(&request, &context),
        llm.name(),
    )?;
    Ok(ReviewGenerationOutcome {
        findings,
        provenance: ReviewProvenance {
            provider_config_id: id,
            provider,
            model: request.model.into(),
            prompt_version: review_v1::VERSION.into(),
            input_tokens: usage.map(|u| u.input_tokens),
            output_tokens: usage.map(|u| u.output_tokens),
        },
        excluded_files: context
            .omitted
            .iter()
            .map(|e| ReviewExclusion {
                path: e.path.clone(),
                reason: e.reason.clone(),
            })
            .collect(),
        truncation: context
            .truncation
            .iter()
            .map(|t| {
                Ok(ReviewTruncation {
                    resource: t.resource.into(),
                    reason: t.reason.into(),
                    omitted_count: u32::try_from(t.omitted_count)
                        .map_err(|_| AppError::LimitExceeded("Too many omissions".into()))?,
                })
            })
            .collect::<AppResult<_>>()?,
    })
}

async fn pin_provider(
    pool: &SqlitePool,
    crypto: &CryptoKey,
    id: Uuid,
    build: impl FnOnce(&ProviderConfig) -> Result<Arc<dyn LlmProvider>, LlmError>,
) -> AppResult<(Arc<dyn LlmProvider>, ReviewProvider)> {
    let row = provider_config_repo::fetch(pool, &id.to_string())
        .await
        .map_err(|e| match e {
            AppError::NotFound(_) => {
                AppError::InvalidInput("Select an existing active connection in Settings".into())
            }
            other => other,
        })?;
    if !row.is_active {
        return Err(AppError::InvalidInput(
            "The selected connection is not active".into(),
        ));
    }
    let config = provider_config_service::build_provider_config(crypto, &row)?;
    let llm = build(&config).map_err(|e| output::safe_provider_error(&e))?;
    // ProviderKind keeps ollama-cloud configuration identity even if its runtime is ollama.
    let runtime = if row.provider == "ollama-cloud" {
        "ollama"
    } else {
        row.provider.as_str()
    };
    if llm.name() != runtime {
        return Err(AppError::Config(
            "Provider does not match the selected connection".into(),
        ));
    }
    let provider: ReviewProvider = serde_json::from_value(Value::String(row.provider.clone()))?;
    Ok((llm, provider))
}

fn safe_text(s: &str) -> bool {
    !s.trim().is_empty() && s.len() <= 16_384 && !s.contains('\0')
}
