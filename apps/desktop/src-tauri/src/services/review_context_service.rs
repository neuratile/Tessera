//! Local staged-review context seam (#108), not a review runtime or wire DTO.
//!
//! Required hunks + containing symbols precede bounded index-only support.
//! No embedding service is required: relative imports and filename-related tests
//! are deterministic hints. Cached retrieval supplies paths/hashes, never text.
//! Caller supplies a trusted secret redactor; this module rejects transformations
//! that change logical line layout. No raw patch is sent to the model.

use crate::services::git_change_service::is_test;
use crate::{
    error::AppResult,
    services::git_change_service::{
        CapturedBlob, ExcludedFile, StagedChangeSet, SupportingCapture,
    },
};
use std::collections::BTreeSet;
mod source;
use source::{append_changed, excerpt, invalid, limit, render, safe_source};

/// Base (`Old`) or staged index (`New`) side of an evidence excerpt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Side {
    Old,
    New,
}

/// Evidence identity is raw-byte SHA-256, never a redacted-content hash.
#[derive(Debug)]
pub struct Excerpt {
    pub path: String,
    pub side: Side,
    pub blob_id: String,
    pub content_hash: String,
    pub start_line: u32,
    pub end_line: u32,
    pub text: String,
    pub required: bool,
}

/// A cached semantic hit is only a hint. Stale hashes are visibly rejected and
/// exact staged content is re-read; cached text/ranges are never evidence.
#[derive(Debug)]
pub struct RetrievalHint {
    pub path: String,
    pub content_hash: String,
}

/// Visible deterministic shortening of optional context, never changed code.
#[derive(Debug)]
pub struct Truncation {
    pub resource: &'static str,
    pub reason: &'static str,
    pub omitted_count: usize,
}

/// Redacted, bounded evidence bound to the immutable capture's identities.
#[derive(Debug)]
pub struct ReviewContext {
    pub snapshot_id: String,
    pub git_object_format: String,
    pub base_commit: Option<String>,
    pub index_fingerprint: String,
    pub excerpts: Vec<Excerpt>,
    pub omitted: Vec<ExcludedFile>,
    pub truncation: Vec<Truncation>,
    /// Visible limitations, not claims of exhaustive dependency/test discovery.
    pub limitations: Vec<&'static str>,
    pub input_tokens: usize,
}

/// Includes instructions/schema reserve and the model's output reserve.
#[derive(Debug, Clone, Copy)]
pub struct ContextBudget {
    pub fixed_prompt_tokens: usize,
    pub model_window: usize,
    pub output_reserve: usize,
}
impl ContextBudget {
    fn available(self) -> AppResult<usize> {
        self.model_window
            .checked_sub(self.output_reserve)
            .map(|n| n.min(8_000))
            .and_then(|n| n.checked_sub(self.fixed_prompt_tokens))
            .ok_or_else(|| limit("Instructions and output reserve exceed the model window"))
    }
}

impl ReviewContext {
    /// #109 must validate every model citation against this supplied evidence.
    /// Empty/file-level references require separate manifest validation.
    #[must_use]
    pub fn permits(&self, path: &str, side: Side, hash: &str, start: u32, end: u32) -> bool {
        start > 0
            && start <= end
            && self.excerpts.iter().any(|e| {
                e.path == path
                    && e.side == side
                    && e.content_hash == hash
                    && e.start_line <= start
                    && end <= e.end_line
            })
    }
    /// Model prompt source section; paths are JSON escaped, lines stay original.
    /// Caller must instruct the model that source is untrusted data, not commands.
    #[must_use]
    pub fn model_text(&self) -> String {
        render(&self.excerpts)
    }
}

/// Pure assembly over immutable raw captures. `redact` is mandatory and trusted
/// to remove secrets (return None when safe redaction is unavailable). It receives
/// each *whole* blob before excerpting, including multiline secret boundaries.
/// A redaction may change characters but never CR/LF layout or logical line count.
/// `count_tokens` must conservatively count the entire source section including
/// labels; `str::len` is a safe byte-based fallback when no tokenizer is available.
/// Required code never truncates. Optional whole-file support is dropped in
/// reverse rank order. Required redaction failures visibly exclude the whole file.
pub fn assemble(
    snapshot: &StagedChangeSet,
    support: &SupportingCapture,
    hints: &[RetrievalHint],
    budget: ContextBudget,
    redact: impl Fn(&str) -> Option<String>,
    count_tokens: impl Fn(&str) -> usize,
) -> AppResult<ReviewContext> {
    if support.snapshot_id != snapshot.snapshot_id
        || support.index_fingerprint != snapshot.index_fingerprint
    {
        return Err(invalid(
            "Supporting context belongs to a different snapshot",
        ));
    }
    if support.files.len() > 20 {
        return Err(limit("More than 20 supporting context files"));
    }
    let available = budget.available()?;
    let mut result = ReviewContext {
        snapshot_id: snapshot.snapshot_id.clone(),
        git_object_format: snapshot.git_object_format.clone(),
        base_commit: snapshot.base_commit.clone(),
        index_fingerprint: snapshot.index_fingerprint.clone(),
        excerpts: vec![],
        omitted: snapshot
            .excluded_files
            .iter()
            .chain(support.omitted.iter())
            .map(|e| ExcludedFile {
                path: e.path.clone(),
                reason: e.reason.clone(),
            })
            .collect(),
        truncation: vec![],
        limitations: vec!["filename_tests_and_relative_imports_only"],
        input_tokens: 0,
    };
    if hints.is_empty() {
        result.limitations.push("no_embeddings_exact_context_only");
    }
    let excluded: BTreeSet<_> = snapshot
        .excluded_files
        .iter()
        .map(|e| e.path.as_str())
        .collect();
    let metadata_source = append_changed(snapshot, &excluded, &redact, &mut result)?;
    let required_count = count_tokens(&render(&result.excerpts));
    if required_count > available {
        return Err(limit(
            "Required changed code exceeds prompt budget; stage a smaller change",
        ));
    }
    result.excerpts.extend(metadata_source);
    record_retrieval_status(snapshot, support, hints, &mut result);
    // Defend the pure assembly seam even if a caller supplies excluded support.
    let mut seen = excluded;
    seen.extend(
        snapshot
            .files
            .iter()
            .flat_map(|f| f.new.iter())
            .map(|b| b.path.as_str()),
    );
    for blob in &support.files {
        if !seen.insert(&blob.path) {
            continue;
        }
        let Some(safe) = safe_source(blob, &redact)? else {
            result.omitted.push(ExcludedFile {
                path: blob.path.clone(),
                reason: "redaction_unmappable".into(),
            });
            continue;
        };
        let count = safe.split_inclusive('\n').count();
        if count == 0 {
            continue;
        }
        result.excerpts.push(excerpt(
            blob,
            Side::New,
            1,
            u32::try_from(count).map_err(|_| invalid("Invalid captured line count"))?,
            safe,
            false,
        ));
    }
    fit_prompt(&mut result, available, &count_tokens)?;
    if !result
        .excerpts
        .iter()
        .any(|e| e.side == Side::New && is_test(&e.path))
    {
        result.limitations.push("no_related_tests_supplied");
    }
    result.input_tokens = budget.fixed_prompt_tokens + count_tokens(&render(&result.excerpts));
    Ok(result)
}

fn record_retrieval_status(
    snapshot: &StagedChangeSet,
    support: &SupportingCapture,
    hints: &[RetrievalHint],
    result: &mut ReviewContext,
) {
    for hint in hints {
        if result.omitted.iter().any(|e| e.path == hint.path) {
            continue;
        }
        let blob = snapshot
            .files
            .iter()
            .flat_map(|f| f.new.iter())
            .chain(support.files.iter())
            .find(|b| b.path == hint.path);
        let reason = match blob {
            Some(b) if b.content_hash == hint.content_hash => None,
            Some(_) => Some("stale_retrieval_replaced"),
            None => Some("stale_retrieval_unavailable"),
        };
        if let Some(reason) = reason {
            result.omitted.push(ExcludedFile {
                path: hint.path.clone(),
                reason: reason.into(),
            });
        }
    }
}

fn fit_prompt(
    result: &mut ReviewContext,
    available: usize,
    count_tokens: &impl Fn(&str) -> usize,
) -> AppResult<()> {
    let mut omitted_count = 0;
    while count_tokens(&render(&result.excerpts)) > available {
        let Some(e) = result.excerpts.pop() else {
            break;
        };
        if e.required {
            return Err(limit("Required changed code exceeds prompt budget"));
        }
        result.omitted.push(ExcludedFile {
            path: e.path,
            reason: "prompt_budget".into(),
        });
        omitted_count += 1;
    }
    if omitted_count > 0 {
        result.truncation.push(Truncation {
            resource: "prompt_input",
            reason: "prompt_budget",
            omitted_count,
        });
    }
    let context_omitted = result
        .omitted
        .iter()
        .filter(|e| e.reason == "context_limit")
        .count();
    if context_omitted > 0 {
        result.truncation.push(Truncation {
            resource: "supporting_context",
            reason: "context_limit",
            omitted_count: context_omitted,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests;
