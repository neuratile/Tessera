//! Final supplied evidence, identity checks and complete request budgeting.
use super::{
    output, redact_source, review_context_service, review_v1, AppError, AppResult, ContextBudget,
    GenerateRequest, LlmProvider, Message, ReviewContext, ReviewRequest, Value,
};
use serde_json::json;

pub(super) fn prepare(
    request: &ReviewRequest<'_>,
    llm: &dyn LlmProvider,
) -> AppResult<ReviewContext> {
    request.manifest.validate()?;
    validate_identity(request)?;
    if request.capture.snapshot_id != request.manifest.snapshot_id.to_string()
        || request.capture.index_fingerprint != request.manifest.index_fingerprint
        || request.capture.base_commit != request.manifest.base_commit
        || request.capture.git_object_format
            != serde_json::to_value(request.manifest.git_object_format)?
                .as_str()
                .unwrap_or("")
    {
        return Err(AppError::InvalidInput(
            "Review manifest does not match capture".into(),
        ));
    }
    let schema = review_v1::tool_schema();
    review_context_service::assemble(
        request.capture,
        request.support,
        &[],
        ContextBudget {
            fixed_prompt_tokens: prompt_cost(
                llm,
                request.model,
                &messages(llm, &json!({}), "", &schema),
                &schema,
            ),
            model_window: llm.capabilities().max_context_tokens as usize,
            output_reserve: llm.capabilities().max_output_tokens.min(6_000) as usize,
        },
        redact_source,
        |s| token_cost(llm, s),
    )
}

fn metadata(request: &ReviewRequest<'_>, context: &ReviewContext) -> Value {
    let metadata_only = metadata_sources(request, context);
    let sources: Vec<_> = request
        .manifest
        .sources
        .iter()
        .filter(|s| {
            context
                .excerpts
                .iter()
                .any(|e| e.path == s.path && output::source_side(e.side) == s.side)
                || metadata_only.contains(s)
        })
        .collect();
    json!({"snapshotId":context.snapshot_id,"gitObjectFormat":context.git_object_format,"baseCommit":context.base_commit,
        "indexFingerprint":context.index_fingerprint,"changes":request.manifest.changes,
        "sources":sources,"metadataOnlySources":metadata_only,
        "exclusions":context.omitted.iter().map(|e| json!({"path":e.path,"reason":e.reason})).collect::<Vec<_>>(),
        "truncation":context.truncation.iter().map(|t| json!({"resource":t.resource,"reason":t.reason,"omittedCount":t.omitted_count})).collect::<Vec<_>>(),
        "limitations":context.limitations})
}

pub(super) fn fit(
    request: &ReviewRequest<'_>,
    context: &mut ReviewContext,
    llm: &dyn LlmProvider,
    schema: &crate::providers::llm::ToolSchema,
) -> AppResult<Vec<Message>> {
    let budget = llm
        .capabilities()
        .max_context_tokens
        .saturating_sub(llm.capabilities().max_output_tokens.min(6_000))
        .min(8_000) as usize;
    loop {
        // Bind every supplied excerpt to persisted blob identity, not merely its path.
        for e in &context.excerpts {
            let side = output::source_side(e.side);
            if !request.manifest.sources.iter().any(|s| {
                s.path == e.path
                    && s.side == side
                    && s.blob_id == e.blob_id
                    && s.content_hash == e.content_hash
                    && e.end_line <= s.line_count
            }) {
                return Err(AppError::InvalidInput(
                    "Supplied source does not match persisted manifest".into(),
                ));
            }
        }
        let messages = messages(
            llm,
            &metadata(request, context),
            &context.model_text(),
            schema,
        );
        let cost = prompt_cost(llm, request.model, &messages, schema);
        if cost <= budget {
            context.input_tokens = cost;
            return Ok(messages);
        }
        let Some(index) = context.excerpts.iter().rposition(|e| !e.required) else {
            return Err(AppError::LimitExceeded(
                "Required review prompt exceeds budget; stage a smaller change".into(),
            ));
        };
        let omitted = context.excerpts.remove(index);
        context
            .omitted
            .push(crate::services::git_change_service::ExcludedFile {
                path: omitted.path,
                reason: "prompt_budget".into(),
            });
        if let Some(t) = context
            .truncation
            .iter_mut()
            .find(|t| t.resource == "final_prompt")
        {
            t.omitted_count += 1;
        } else {
            context.truncation.push(review_context_service::Truncation {
                resource: "final_prompt",
                reason: "prompt_budget",
                omitted_count: 1,
            });
        }
    }
}

fn token_cost(llm: &dyn LlmProvider, s: &str) -> usize {
    // Byte count is conservative even for non-English/code tokenization. Never rely
    // on the existing chars/4 heuristic for this security/resource boundary.
    s.len().max(llm.count_tokens(s))
}
fn prompt_cost(
    llm: &dyn LlmProvider,
    model: &str,
    messages: &[Message],
    schema: &crate::providers::llm::ToolSchema,
) -> usize {
    // Count the entire provider-neutral request, including role/content wrappers,
    // schema, escaping and generation options. Concrete provider framing has a
    // small additional reserve; bytes conservatively bound tokenization.
    let request = GenerateRequest {
        model: model.into(),
        messages: messages.to_vec(),
        tools: if llm.capabilities().supports_tools {
            vec![schema.clone()]
        } else {
            vec![]
        },
        temperature: Some(0.0),
        max_tokens: Some(llm.capabilities().max_output_tokens.min(6_000)),
        stop_sequences: vec![],
    };
    serde_json::to_string(&request).map_or(usize::MAX, |s| token_cost(llm, &s).saturating_add(256))
}
fn messages(
    llm: &dyn LlmProvider,
    metadata: &Value,
    source: &str,
    schema: &crate::providers::llm::ToolSchema,
) -> Vec<Message> {
    let mut result = review_v1::build_messages(metadata, source);
    if !llm.capabilities().supports_tools {
        result.insert(
            1,
            Message::system(format!(
                "Return JSON matching this schema: {}",
                schema.parameters_schema
            )),
        );
    }
    result
}

fn validate_identity(request: &ReviewRequest<'_>) -> AppResult<()> {
    use crate::{
        repositories::review_repo::{ReviewChangeKind, SourceSide},
        services::git_change_service::ChangeKind,
    };
    use sha2::{Digest, Sha256};
    let manifest = request.manifest;
    let captures = request
        .capture
        .files
        .iter()
        .flat_map(|f| {
            f.old
                .iter()
                .map(|b| (SourceSide::Old, b))
                .chain(f.new.iter().map(|b| (SourceSide::New, b)))
        })
        .chain(request.support.files.iter().map(|b| (SourceSide::New, b)))
        .collect::<Vec<_>>();
    let identities: std::collections::BTreeSet<_> = captures
        .iter()
        .map(|(side, b)| (*side as u8, b.path.as_str()))
        .collect();
    if captures.len() != manifest.sources.len()
        || identities.len() != captures.len()
        || captures.iter().any(|(side, b)| {
            !manifest.sources.iter().any(|s| {
                s.side == *side
                    && s.path == b.path
                    && s.blob_id == b.blob_id
                    && s.mode == b.mode
                    && s.content_hash == b.content_hash
                    && format!("{:x}", Sha256::digest(&b.content)) == s.content_hash
                    && b.content.len() == s.byte_length as usize
                    && b.content.split_inclusive(|b| *b == b'\n').count() == s.line_count as usize
            })
        })
        || request.capture.files.len() != manifest.changes.len()
        || request.capture.files.iter().any(|f| {
            !manifest.changes.iter().any(|c| {
                c.change_kind
                    == match f.change_kind {
                        ChangeKind::Added => ReviewChangeKind::Added,
                        ChangeKind::Modified => ReviewChangeKind::Modified,
                        ChangeKind::Deleted => ReviewChangeKind::Deleted,
                        ChangeKind::Renamed => ReviewChangeKind::Renamed,
                    }
                    && c.old_path.as_deref() == f.old.as_ref().map(|b| b.path.as_str())
                    && c.new_path.as_deref() == f.new.as_ref().map(|b| b.path.as_str())
            })
        })
    {
        return Err(AppError::InvalidInput(
            "Captured source does not match persisted manifest".into(),
        ));
    }
    Ok(())
}

// Metadata-only changes retain file-level evidence even if optional source was
// dropped for budget. Redaction failures exclude the entire change instead.
pub(super) fn metadata_sources<'a>(
    request: &'a ReviewRequest<'_>,
    context: &ReviewContext,
) -> Vec<&'a crate::repositories::review_repo::ReviewSource> {
    request
        .manifest
        .sources
        .iter()
        .filter(|s| {
            request.capture.files.iter().any(|f| {
                f.hunks.is_empty()
                    && !f.old.iter().chain(f.new.iter()).any(|b| {
                        context
                            .omitted
                            .iter()
                            .any(|e| e.path == b.path && e.reason != "prompt_budget")
                    })
                    && match s.side {
                        crate::repositories::review_repo::SourceSide::Old => f.old.as_ref(),
                        crate::repositories::review_repo::SourceSide::New => f.new.as_ref(),
                    }
                    .is_some_and(|b| b.path == s.path)
            })
        })
        .collect()
}
