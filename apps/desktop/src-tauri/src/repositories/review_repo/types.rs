//! Persisted review DTOs. Raw source is stored separately, never in report text.
use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! wire_enum {
    ($name:ident { $($variant:ident => $wire:literal),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
        pub enum $name { $(#[serde(rename = $wire)] $variant),+ }
    };
}
wire_enum!(ReviewStatus { Queued => "queued", Running => "running", Completed => "completed", Failed => "failed", Cancelled => "cancelled", Interrupted => "interrupted" });
wire_enum!(ReviewOutcome { Reviewed => "reviewed", NoChanges => "no_changes", NothingReviewable => "nothing_reviewable" });
wire_enum!(ReviewScope { Staged => "staged" });
wire_enum!(ReviewChangeKind { Added => "added", Modified => "modified", Deleted => "deleted", Renamed => "renamed" });
wire_enum!(SourceSide { Old => "old", New => "new" });
wire_enum!(GitObjectFormat { Sha1 => "sha1", Sha256 => "sha256" });
wire_enum!(ReviewProvider { Ollama => "ollama", OllamaCloud => "ollama-cloud", Openai => "openai", Openrouter => "openrouter", Anthropic => "anthropic", Gemini => "gemini" });
wire_enum!(ReviewStepKind { Capture => "capture", Context => "context", Findings => "findings", Persist => "persist" });
wire_enum!(ReviewSeverity { Critical => "critical", Major => "major", Minor => "minor", Trivial => "trivial" });
wire_enum!(EvidenceState { Suspected => "suspected", Reproduced => "reproduced", Inconclusive => "inconclusive" });

impl ReviewStatus {
    #[must_use]
    pub fn is_terminal(self) -> bool {
        !matches!(self, Self::Queued | Self::Running)
    }
    #[must_use]
    pub fn can_transition(self, next: Self) -> bool {
        !self.is_terminal()
            && (self == next
                || match self {
                    Self::Queued => matches!(
                        next,
                        Self::Running | Self::Failed | Self::Cancelled | Self::Interrupted
                    ),
                    Self::Running => next.is_terminal(),
                    _ => false,
                })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewProvenance {
    pub provider_config_id: Uuid,
    pub provider: ReviewProvider,
    pub model: String,
    pub prompt_version: String,
    #[serde(
        default,
        deserialize_with = "non_null_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub input_tokens: Option<u32>,
    #[serde(
        default,
        deserialize_with = "non_null_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub output_tokens: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewExclusion {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewSource {
    pub path: String,
    pub side: SourceSide,
    pub blob_id: String,
    pub mode: String,
    pub content_hash: String,
    pub byte_length: u32,
    pub line_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewChange {
    pub change_kind: ReviewChangeKind,
    #[serde(
        default,
        deserialize_with = "non_null_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub old_path: Option<String>,
    #[serde(
        default,
        deserialize_with = "non_null_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub new_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewSnapshot {
    pub snapshot_id: Uuid,
    pub git_object_format: GitObjectFormat,
    #[serde(
        default,
        deserialize_with = "non_null_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub base_commit: Option<String>,
    pub index_fingerprint: String,
    pub sources: Vec<ReviewSource>,
    pub changes: Vec<ReviewChange>,
    pub context_paths: Vec<String>,
    pub exclusions: Vec<ReviewExclusion>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewSourceReference {
    pub snapshot_id: Uuid,
    pub path: String,
    pub side: SourceSide,
    #[serde(
        default,
        deserialize_with = "non_null_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub line_start: Option<u32>,
    #[serde(
        default,
        deserialize_with = "non_null_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub line_end: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewFinding {
    pub finding_id: Uuid,
    pub summary: String,
    pub explanation: String,
    pub expected_behavior: String,
    /// Absent on legacy records; new review generation always supplies this.
    #[serde(
        default,
        deserialize_with = "non_null_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub suggested_test: Option<String>,
    pub severity: ReviewSeverity,
    pub sources: Vec<ReviewSourceReference>,
    pub evidence_state: EvidenceState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewStep {
    pub step_id: Uuid,
    pub step: ReviewStepKind,
    pub status: ReviewStatus,
    pub progress: u32,
    pub created_at: String,
    #[serde(
        default,
        deserialize_with = "non_null_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub error: Option<ReviewError>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewTruncation {
    pub resource: String,
    pub reason: String,
    pub omitted_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewSession {
    pub review_id: Uuid,
    pub project_id: Uuid,
    pub scope: ReviewScope,
    pub status: ReviewStatus,
    pub sequence: u32,
    pub progress: u32,
    pub provenance: ReviewProvenance,
    pub created_at: String,
    pub updated_at: String,
    #[serde(
        default,
        deserialize_with = "non_null_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub outcome: Option<ReviewOutcome>,
    #[serde(
        default,
        deserialize_with = "non_null_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub error: Option<ReviewError>,
    #[serde(
        default,
        deserialize_with = "non_null_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub snapshot: Option<ReviewSnapshot>,
    pub steps: Vec<ReviewStep>,
    pub findings: Vec<ReviewFinding>,
    /// Final context omissions, appended after prompt selection. Raw snapshot stays immutable.
    #[serde(
        default,
        deserialize_with = "non_null_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub excluded_files: Option<Vec<ReviewExclusion>>,
    pub truncation: Vec<ReviewTruncation>,
}

/// A link to an existing immutable artifact row/version, not another test payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewArtifactLink {
    pub link_id: Uuid,
    pub finding_id: Uuid,
    pub snapshot_id: Uuid,
    pub artifact_id: Uuid,
    pub test_id: Uuid,
    pub test_version: u32,
    /// SHA-256 from `saved_test_content_hash`, covering all saved file paths/bytes.
    pub test_content_hash: String,
    pub provenance: ReviewProvenance,
}

/// Append-only attempt. Execution details/results remain in existing run storage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewRunEnvironment {
    pub runner: String,
    pub image_id: String,
    pub started_at: String,
    pub finished_at: String,
    pub timeout_ms: u32,
    pub memory_bytes: u32,
    pub cpu_millis: u32,
    pub output_truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewRunLink {
    pub attempt_id: Uuid,
    pub artifact_link_id: Uuid,
    pub run_id: Uuid,
    pub environment: ReviewRunEnvironment,
    pub evidence_state: EvidenceState,
    #[serde(
        default,
        deserialize_with = "non_null_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub assertion: Option<String>,
    #[serde(
        default,
        deserialize_with = "non_null_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub expected: Option<String>,
    #[serde(
        default,
        deserialize_with = "non_null_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub observed: Option<String>,
}

fn non_null_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}
