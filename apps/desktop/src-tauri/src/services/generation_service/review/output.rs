//! Strict stream and output trust boundaries; never salvage invalid JSON.
use crate::{
    error::{AppError, AppResult},
    prompts::review_v1,
    providers::llm::{Chunk, FinishReason, LlmError, ToolSchema, Usage},
    repositories::review_repo::{
        EvidenceState, ReviewFinding, ReviewSeverity, ReviewSnapshot, ReviewSourceReference,
        SourceSide,
    },
    services::review_context_service::{ReviewContext, Side},
};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Default)]
pub(super) struct Collector {
    text: String,
    args: String,
    tool_id: Option<String>,
    done: Option<Usage>,
    bytes: usize,
}
impl Collector {
    pub(super) fn push(
        &mut self,
        chunk: Chunk,
        provider: &'static str,
        output_cap: u32,
    ) -> AppResult<()> {
        if self.done.is_some() {
            return Err(invalid(provider));
        }
        match chunk {
            Chunk::TextDelta(s) => {
                self.add_size(s.len())?;
                self.text.push_str(&s);
            }
            Chunk::ToolCallStart { id, name } => {
                self.add_size(id.len().saturating_add(name.len()))?;
                if name != review_v1::TOOL_NAME || id.is_empty() || self.tool_id.is_some() {
                    return Err(invalid(provider));
                }
                self.tool_id = Some(id);
            }
            Chunk::ToolCallArgsDelta { id, json_fragment } => {
                if self.tool_id.as_ref() != Some(&id) {
                    return Err(invalid(provider));
                }
                self.add_size(json_fragment.len())?;
                self.args.push_str(&json_fragment);
            }
            Chunk::Done {
                usage,
                finish_reason,
            } => {
                if !matches!(
                    finish_reason,
                    FinishReason::Stop | FinishReason::ToolUse | FinishReason::StopSequence
                ) || usage.output_tokens > output_cap
                {
                    return Err(invalid(provider));
                }
                self.done = Some(usage);
            }
        }
        Ok(())
    }
    fn add_size(&mut self, n: usize) -> AppResult<()> {
        self.bytes = self.bytes.saturating_add(n);
        if self.bytes > 128 * 1024 {
            return Err(AppError::LimitExceeded(
                "Review output exceeds 128 KiB".into(),
            ));
        }
        Ok(())
    }
    pub(super) fn finish(self, provider: &'static str) -> AppResult<(String, Option<Usage>)> {
        let usage = self.done.ok_or_else(|| {
            AppError::Llm(LlmError::StreamInterrupted {
                provider,
                message: "Review stream ended without terminal completion".into(),
            })
        })?;
        let payload = if self.tool_id.is_some() {
            if !self.text.trim().is_empty() {
                return Err(invalid(provider));
            }
            self.args
        } else {
            self.text
        };
        Ok((payload, (usage != Usage::default()).then_some(usage)))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Citation {
    snapshot_id: Uuid,
    path: String,
    side: SourceSide,
    blob_id: String,
    content_hash: String,
    line_start: Option<u32>,
    line_end: Option<u32>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Finding {
    summary: String,
    explanation: String,
    expected_behavior: String,
    severity: ReviewSeverity,
    sources: Vec<Citation>,
    suggested_test: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Payload {
    findings: Vec<Finding>,
}

pub(super) fn source_side(side: Side) -> SourceSide {
    match side {
        Side::Old => SourceSide::Old,
        Side::New => SourceSide::New,
    }
}
fn context_side(side: SourceSide) -> Side {
    match side {
        SourceSide::Old => Side::Old,
        SourceSide::New => Side::New,
    }
}

pub(super) fn validate(
    payload: &str,
    schema: &ToolSchema,
    manifest: &ReviewSnapshot,
    context: &ReviewContext,
    metadata: &[&crate::repositories::review_repo::ReviewSource],
    provider: &'static str,
) -> AppResult<Vec<ReviewFinding>> {
    // Typed first: serde rejects duplicate fields rather than silently taking the last.
    let parsed: Payload = serde_json::from_str(payload).map_err(|_| invalid(provider))?;
    let value: serde_json::Value = serde_json::from_str(payload).map_err(|_| invalid(provider))?;
    let validator = jsonschema::JSONSchema::compile(&schema.parameters_schema)
        .map_err(|_| invalid(provider))?;
    if !validator.is_valid(&value) {
        return Err(invalid(provider));
    }
    parsed
        .findings
        .into_iter()
        .map(|f| {
            if [
                &f.summary,
                &f.explanation,
                &f.expected_behavior,
                &f.suggested_test,
            ]
            .iter()
            .any(|s| !super::safe_text(s))
            {
                return Err(invalid(provider));
            }
            let sources = f
                .sources
                .into_iter()
                .map(|s| {
                    let reference = ReviewSourceReference {
                        snapshot_id: s.snapshot_id,
                        path: s.path,
                        side: s.side,
                        line_start: s.line_start,
                        line_end: s.line_end,
                    };
                    reference
                        .validate(manifest)
                        .map_err(|_| invalid(provider))?;
                    if !manifest.sources.iter().any(|b| {
                        b.path == reference.path
                            && b.side == s.side
                            && b.blob_id == s.blob_id
                            && b.content_hash == s.content_hash
                    }) || !match (s.line_start, s.line_end) {
                        (Some(start), Some(end)) => context.permits(
                            &reference.path,
                            context_side(s.side),
                            &s.content_hash,
                            start,
                            end,
                        ),
                        (None, None) => metadata.iter().any(|b| {
                            b.path == reference.path
                                && b.side == s.side
                                && b.blob_id == s.blob_id
                                && b.content_hash == s.content_hash
                        }),
                        _ => false,
                    } {
                        return Err(invalid(provider));
                    }
                    Ok(reference)
                })
                .collect::<AppResult<Vec<_>>>()?;
            Ok(ReviewFinding {
                finding_id: Uuid::new_v4(),
                summary: f.summary,
                explanation: f.explanation,
                expected_behavior: f.expected_behavior,
                severity: f.severity,
                sources,
                suggested_test: Some(f.suggested_test),
                evidence_state: EvidenceState::Suspected,
            })
        })
        .collect()
}
fn invalid(provider: &'static str) -> AppError {
    LlmError::InvalidResponse {
        provider,
        message: "Invalid or unsupported review findings; no findings accepted".into(),
    }
    .into()
}

// Preserve the code and existing recovery hint, but never echo a provider body,
// payload, endpoint or credential into records/logs.
pub(super) fn safe_provider_error(error: &LlmError) -> AppError {
    let provider = error.provider();
    let message = "Review provider call failed".to_string();
    match error {
        LlmError::ConnectionFailed { .. } => LlmError::ConnectionFailed { provider, message },
        LlmError::AuthFailed { .. } => LlmError::AuthFailed { provider, message },
        LlmError::RateLimited {
            retry_after_seconds,
            ..
        } => LlmError::RateLimited {
            provider,
            retry_after_seconds: *retry_after_seconds,
        },
        LlmError::ContextExceeded {
            requested_tokens,
            limit,
            ..
        } => LlmError::ContextExceeded {
            provider,
            requested_tokens: *requested_tokens,
            limit: *limit,
        },
        LlmError::InvalidResponse { .. } => LlmError::InvalidResponse { provider, message },
        LlmError::SchemaValidationFailed { .. } => LlmError::SchemaValidationFailed {
            provider,
            payload_preview: message,
        },
        LlmError::StreamInterrupted { .. } => LlmError::StreamInterrupted { provider, message },
        LlmError::ProviderUnavailable { .. } => LlmError::ProviderUnavailable { provider, message },
        LlmError::Unsupported { .. } => LlmError::Unsupported {
            provider,
            feature: "review structured output",
        },
    }
    .into()
}
