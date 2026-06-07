//! Artifact export service — xlsx / csv / tsv (see
//! `plan/ARTIFACT_EXPORT.md`).
//!
//! Mirrors the generation-service pattern: this module is the sole
//! entry point for exports; commands in `commands/exports.rs` stay
//! thin. Flow: fetch artifact → [`build_export_doc`] (pure mapper →
//! IR) → format writer → write file(s) → return every written path
//! so the frontend toast can list them.
//!
//! Multi-section documents exported to CSV/TSV produce sibling files
//! (`{stem}.{section-slug}.{ext}`) because those formats cannot hold
//! more than one table per file; xlsx holds one worksheet per
//! section in a single workbook.

pub mod csv_writer;
pub mod ir;
pub mod mappers;
pub mod xlsx_writer;

use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use sqlx::SqlitePool;

use crate::error::{AppError, AppResult};
use crate::repositories::artifact_repo;

pub use csv_writer::render_tsv;
pub use ir::ExportDoc;
pub use mappers::build_export_doc;

/// Output formats the export command accepts. The lowercase serde
/// names are the IPC wire values (`ExportFormatSchema` in
/// `packages/shared/` mirrors them).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    Xlsx,
    Csv,
    Tsv,
}

impl ExportFormat {
    /// Canonical file extension (no leading dot).
    #[must_use]
    pub fn extension(self) -> &'static str {
        match self {
            Self::Xlsx => "xlsx",
            Self::Csv => "csv",
            Self::Tsv => "tsv",
        }
    }

    fn delimiter(self) -> u8 {
        match self {
            Self::Tsv => b'\t',
            // xlsx never reaches the delimiter path.
            Self::Xlsx | Self::Csv => b',',
        }
    }
}

/// Export an artifact to `dest_path` in the requested format.
/// Returns every file written (CSV/TSV may emit section siblings).
///
/// # Errors
///
/// - [`AppError::NotFound`] when the artifact does not exist.
/// - [`AppError::InvalidInput`] when the artifact has no structured
///   data or `dest_path` fails validation.
/// - [`AppError::Io`] / [`AppError::Internal`] when writing fails.
pub async fn export_artifact(
    pool: &SqlitePool,
    artifact_id: &str,
    format: ExportFormat,
    dest_path: &Path,
) -> AppResult<Vec<PathBuf>> {
    let artifact = artifact_repo::fetch(pool, artifact_id).await?;
    let doc = build_export_doc(&artifact)?;
    let dest = validate_dest_path(dest_path, format)?;

    // File IO is blocking; keep it off the async executor.
    tokio::task::spawn_blocking(move || write_doc(&doc, format, &dest))
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("export task panicked: {e}")))?
}

/// Render an artifact as clipboard-ready TSV (no files written).
///
/// # Errors
///
/// - [`AppError::NotFound`] when the artifact does not exist.
/// - [`AppError::InvalidInput`] when the artifact has no structured
///   data.
pub async fn artifact_tsv(pool: &SqlitePool, artifact_id: &str) -> AppResult<String> {
    let artifact = artifact_repo::fetch(pool, artifact_id).await?;
    let doc = build_export_doc(&artifact)?;
    Ok(render_tsv(&doc))
}

fn write_doc(doc: &ExportDoc, format: ExportFormat, dest: &Path) -> AppResult<Vec<PathBuf>> {
    match format {
        ExportFormat::Xlsx => {
            xlsx_writer::write_workbook(doc, dest)?;
            Ok(vec![dest.to_path_buf()])
        }
        ExportFormat::Csv | ExportFormat::Tsv => write_delimited(doc, format, dest),
    }
}

/// CSV/TSV: the first section goes to the user-chosen path; each
/// additional section becomes a `{stem}.{section-slug}.{ext}`
/// sibling next to it.
fn write_delimited(doc: &ExportDoc, format: ExportFormat, dest: &Path) -> AppResult<Vec<PathBuf>> {
    let mut written = Vec::with_capacity(doc.sections.len());
    for (idx, section) in doc.sections.iter().enumerate() {
        let path = if idx == 0 {
            dest.to_path_buf()
        } else {
            sibling_path(dest, section.name(), format)
        };
        let file = File::create(&path)?;
        csv_writer::write_section(BufWriter::new(file), section, format.delimiter(), true)?;
        written.push(path);
    }
    Ok(written)
}

fn sibling_path(dest: &Path, section_name: &str, format: ExportFormat) -> PathBuf {
    let stem = dest
        .file_stem()
        .map_or_else(|| "export".to_string(), |s| s.to_string_lossy().into_owned());
    let slug = section_slug(section_name);
    dest.with_file_name(format!("{stem}.{slug}.{}", format.extension()))
}

/// Windows device names that cannot be used as a bare filename
/// component regardless of extension.
const WINDOWS_RESERVED: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// Slug a section name into `[a-z0-9-]` for sibling filenames.
fn section_slug(name: &str) -> String {
    let mut slug = String::with_capacity(name.len());
    let mut last_was_dash = true; // suppress a leading dash
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
            last_was_dash = false;
        } else if !last_was_dash {
            slug.push('-');
            last_was_dash = true;
        }
    }
    let slug = slug.trim_end_matches('-').to_string();
    if slug.is_empty() {
        return "section".to_string();
    }
    if WINDOWS_RESERVED.contains(&slug.as_str()) {
        return format!("{slug}-section");
    }
    slug
}

/// Validate and normalize the caller-supplied destination path.
/// Rust-side because any frontend code can invoke the command — the
/// save dialog is a convenience, not a trust boundary.
///
/// - Must be absolute and NUL-free.
/// - Parent directory must exist; it is canonicalized and the
///   filename re-joined so `..` segments cannot escape it.
/// - An existing directory at the destination is rejected.
/// - The format's extension is appended when missing.
fn validate_dest_path(raw: &Path, format: ExportFormat) -> AppResult<PathBuf> {
    let raw_text = raw.to_string_lossy();
    if raw_text.contains('\u{0}') {
        return Err(AppError::InvalidInput(
            "destination path contains a NUL byte".into(),
        ));
    }
    if !raw.is_absolute() {
        return Err(AppError::InvalidInput(
            "destination path must be absolute".into(),
        ));
    }
    let file_name = raw
        .file_name()
        .ok_or_else(|| AppError::InvalidInput("destination path has no filename".into()))?;
    let parent = raw
        .parent()
        .ok_or_else(|| AppError::InvalidInput("destination path has no parent directory".into()))?;
    let parent = parent.canonicalize().map_err(|e| {
        AppError::InvalidInput(format!(
            "destination directory does not exist or is unreadable: {e}"
        ))
    })?;

    let mut dest = parent.join(file_name);
    if dest.is_dir() {
        return Err(AppError::InvalidInput(
            "destination path is an existing directory".into(),
        ));
    }

    let has_expected_ext = dest
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case(format.extension()));
    if !has_expected_ext {
        let name = dest
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        dest.set_file_name(format!("{name}.{}", format.extension()));
    }
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_pool_at;
    use crate::repositories::artifact_repo::{ArtifactInsert, ArtifactType, GenerationMetadata};
    use chrono::Utc;
    use uuid::Uuid;

    // -- format plumbing ----------------------------------------------------

    #[test]
    fn export_format_deserializes_lowercase_wire_values() {
        let cases = [
            ("\"xlsx\"", ExportFormat::Xlsx),
            ("\"csv\"", ExportFormat::Csv),
            ("\"tsv\"", ExportFormat::Tsv),
        ];
        for (wire, expected) in cases {
            let parsed: ExportFormat = serde_json::from_str(wire).expect("parse");
            assert_eq!(parsed, expected);
        }
        assert!(serde_json::from_str::<ExportFormat>("\"pdf\"").is_err());
    }

    // -- slug ---------------------------------------------------------------

    #[test]
    fn section_slug_normalizes_to_lower_kebab() {
        assert_eq!(section_slug("Test Cases"), "test-cases");
        assert_eq!(section_slug("  Files!! "), "files");
        assert_eq!(section_slug("日本語"), "section");
    }

    #[test]
    fn section_slug_avoids_windows_reserved_names() {
        assert_eq!(section_slug("CON"), "con-section");
        assert_eq!(section_slug("aux"), "aux-section");
    }

    // -- path validation ----------------------------------------------------

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tessera-export-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        dir
    }

    #[test]
    fn relative_paths_are_rejected() {
        let err = validate_dest_path(Path::new("out.csv"), ExportFormat::Csv)
            .expect_err("must reject");
        assert_eq!(err.code(), "INVALID_INPUT");
    }

    #[test]
    fn missing_parent_directory_is_rejected() {
        let bogus = temp_dir().join("does-not-exist").join("out.csv");
        let err = validate_dest_path(&bogus, ExportFormat::Csv).expect_err("must reject");
        assert_eq!(err.code(), "INVALID_INPUT");
    }

    #[test]
    fn existing_directory_destination_is_rejected() {
        let dir = temp_dir();
        let sub = dir.join("taken.csv");
        std::fs::create_dir_all(&sub).expect("mkdir");
        let err = validate_dest_path(&sub, ExportFormat::Csv).expect_err("must reject");
        assert_eq!(err.code(), "INVALID_INPUT");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dotdot_segments_are_neutralized_by_canonicalization() {
        let dir = temp_dir();
        let sneaky = dir.join("..").join(
            dir.file_name()
                .map(PathBuf::from)
                .expect("temp dir has a name"),
        );
        let sneaky = sneaky.join("out.csv");
        let validated = validate_dest_path(&sneaky, ExportFormat::Csv).expect("validate");
        // The resolved path must live directly inside the canonical dir.
        assert_eq!(
            validated.parent().expect("parent"),
            dir.canonicalize().expect("canonical")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_extension_is_appended() {
        let dir = temp_dir();
        let validated = validate_dest_path(&dir.join("report"), ExportFormat::Xlsx)
            .expect("validate");
        assert!(validated.to_string_lossy().ends_with("report.xlsx"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn mismatched_extension_is_appended_not_replaced() {
        let dir = temp_dir();
        let validated =
            validate_dest_path(&dir.join("report.txt"), ExportFormat::Csv).expect("validate");
        assert!(validated.to_string_lossy().ends_with("report.txt.csv"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn correct_extension_is_kept_case_insensitively() {
        let dir = temp_dir();
        let validated =
            validate_dest_path(&dir.join("report.XLSX"), ExportFormat::Xlsx).expect("validate");
        assert!(validated.to_string_lossy().ends_with("report.XLSX"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    // -- end-to-end ---------------------------------------------------------

    async fn seeded_pool_with_artifact(
        artifact_type: ArtifactType,
        data: serde_json::Value,
    ) -> (SqlitePool, PathBuf, String) {
        let db_path = std::env::temp_dir().join(format!("tessera-export-{}.db", Uuid::new_v4()));
        let pool = init_pool_at(&db_path).await.expect("pool");
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO projects (id, user_id, name, root_path, created_at, updated_at) \
             VALUES ('p1', '00000000-0000-4000-8000-000000000001', 'p', '/tmp/p', ?, ?)",
        )
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .expect("seed project");

        let id = artifact_repo::insert(
            &pool,
            ArtifactInsert {
                project_id: "p1".into(),
                artifact_type,
                title: "Export me".into(),
                content_md: "# md".into(),
                structured_data: data,
                generation_metadata: GenerationMetadata {
                    provider: "ollama".into(),
                    model: "qwen2.5-coder:7b".into(),
                    prompt_version: "test_cases_v2".into(),
                    input_tokens: 1,
                    output_tokens: 1,
                    started_at: "2026-06-07T00:00:00Z".into(),
                    completed_at: "2026-06-07T00:00:01Z".into(),
                },
                parent_id: None,
            },
        )
        .await
        .expect("insert artifact");
        (pool, db_path, id)
    }

    fn test_cases_payload() -> serde_json::Value {
        serde_json::json!({
            "cases": [{
                "id": "TC-1",
                "title": "First case",
                "type": "positive",
                "priority": "p1",
                "steps": [{ "action": "do it", "expectedResult": "works" }]
            }],
            "files": [{ "path": "a.test.ts", "contents": "it()", "isTest": true }]
        })
    }

    #[tokio::test]
    async fn export_xlsx_end_to_end_writes_single_workbook() {
        let (pool, db_path, id) =
            seeded_pool_with_artifact(ArtifactType::TestCases, test_cases_payload()).await;
        let dir = temp_dir();
        let dest = dir.join("cases.xlsx");

        let written = export_artifact(&pool, &id, ExportFormat::Xlsx, &dest)
            .await
            .expect("export");
        assert_eq!(written.len(), 1);
        let bytes = std::fs::read(&written[0]).expect("read back");
        assert_eq!(&bytes[..4], b"PK\x03\x04");

        pool.close().await;
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_file(&db_path);
    }

    #[tokio::test]
    async fn export_csv_multi_section_writes_named_siblings() {
        let (pool, db_path, id) =
            seeded_pool_with_artifact(ArtifactType::TestCases, test_cases_payload()).await;
        let dir = temp_dir();
        let dest = dir.join("cases.csv");

        let written = export_artifact(&pool, &id, ExportFormat::Csv, &dest)
            .await
            .expect("export");
        assert_eq!(written.len(), 2);
        assert!(written[0].to_string_lossy().ends_with("cases.csv"));
        assert!(written[1].to_string_lossy().ends_with("cases.files.csv"));
        let primary = std::fs::read(&written[0]).expect("read back");
        assert_eq!(&primary[..3], csv_writer::UTF8_BOM);

        pool.close().await;
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_file(&db_path);
    }

    #[tokio::test]
    async fn export_missing_artifact_returns_not_found() {
        let (pool, db_path, _id) =
            seeded_pool_with_artifact(ArtifactType::TestCases, test_cases_payload()).await;
        let dir = temp_dir();
        let err = export_artifact(&pool, "nope", ExportFormat::Csv, &dir.join("x.csv"))
            .await
            .expect_err("must 404");
        assert_eq!(err.code(), "NOT_FOUND");
        pool.close().await;
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_file(&db_path);
    }

    #[tokio::test]
    async fn export_null_payload_returns_invalid_input() {
        let (pool, db_path, id) =
            seeded_pool_with_artifact(ArtifactType::BugReport, serde_json::Value::Null).await;
        let dir = temp_dir();
        let err = export_artifact(&pool, &id, ExportFormat::Xlsx, &dir.join("x.xlsx"))
            .await
            .expect_err("must reject");
        assert_eq!(err.code(), "INVALID_INPUT");
        pool.close().await;
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_file(&db_path);
    }

    #[tokio::test]
    async fn artifact_tsv_renders_clipboard_payload() {
        let (pool, db_path, id) =
            seeded_pool_with_artifact(ArtifactType::TestCases, test_cases_payload()).await;
        let tsv = artifact_tsv(&pool, &id).await.expect("tsv");
        // Multi-section doc: section names included.
        assert!(tsv.starts_with("Test Cases\r\nID\tTitle"));
        assert!(tsv.contains("TC-1\tFirst case"));
        pool.close().await;
        let _ = std::fs::remove_file(&db_path);
    }
}
