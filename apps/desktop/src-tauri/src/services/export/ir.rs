//! Export intermediate representation (IR).
//!
//! One mapper per artifact type produces a writer-agnostic
//! [`ExportDoc`]; every output format (csv, tsv, xlsx — and later the
//! Jira adapter, see `plan/ARTIFACT_EXPORT.md` §3) consumes only this
//! IR. This keeps the artifact-type × destination matrix at N+M
//! instead of N×M.
//!
//! Cells are pre-flattened `String`s: arrays become numbered
//! newline-joined lines, nested objects flatten to labelled lines.
//! The flattening helpers live here so all mappers share identical
//! formatting.

use serde::Serialize;

/// Hard cap on a single cell's character count. xlsx rejects cells
/// above 32,767 characters; CSV/TSV apply the same cap for
/// consistency so an export never silently differs between formats.
pub const MAX_CELL_CHARS: usize = 32_767;

/// Suffix appended when a cell is truncated at [`MAX_CELL_CHARS`].
pub const TRUNCATION_SUFFIX: &str = "… (truncated)";

/// A complete export-ready document: title plus ordered sections.
#[derive(Debug, Clone, Serialize)]
pub struct ExportDoc {
    pub title: String,
    pub sections: Vec<ExportSection>,
}

/// One logical sheet / CSV file inside an [`ExportDoc`].
#[derive(Debug, Clone, Serialize)]
pub enum ExportSection {
    /// Tabular artifacts (test cases, findings, bugs).
    Table(ExportTable),
    /// Prose artifacts (test plan, project context).
    KeyValues(KeyValueSection),
}

impl ExportSection {
    /// Section name — sheet name in xlsx, filename suffix for CSV
    /// siblings.
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Table(t) => &t.name,
            Self::KeyValues(kv) => &kv.name,
        }
    }
}

/// A rectangular table with a header row.
#[derive(Debug, Clone, Serialize)]
pub struct ExportTable {
    pub name: String,
    pub columns: Vec<String>,
    /// Cells pre-flattened to strings; each row has `columns.len()`
    /// entries.
    pub rows: Vec<Vec<String>>,
}

/// A two-column Field / Value section for prose artifacts.
#[derive(Debug, Clone, Serialize)]
pub struct KeyValueSection {
    pub name: String,
    pub entries: Vec<(String, String)>,
}

/// Truncate a cell to [`MAX_CELL_CHARS`], appending
/// [`TRUNCATION_SUFFIX`] when content was dropped. Operates on char
/// boundaries so multi-byte text never splits mid-codepoint.
#[must_use]
pub fn clamp_cell(value: String) -> String {
    if value.chars().count() <= MAX_CELL_CHARS {
        return value;
    }
    let keep = MAX_CELL_CHARS - TRUNCATION_SUFFIX.chars().count();
    let mut out: String = value.chars().take(keep).collect();
    out.push_str(TRUNCATION_SUFFIX);
    out
}

/// Join a string array into numbered lines: `1. a\n2. b`. Empty
/// input yields an empty string.
#[must_use]
pub fn numbered_lines(items: &[String]) -> String {
    items
        .iter()
        .enumerate()
        .map(|(i, item)| format!("{}. {}", i + 1, item))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Join plain (unnumbered) lines with newlines.
#[must_use]
pub fn joined_lines(items: &[String]) -> String {
    items.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_cell_passes_short_values_through() {
        assert_eq!(clamp_cell("hello".into()), "hello");
    }

    #[test]
    fn clamp_cell_truncates_to_limit_with_suffix() {
        let long = "x".repeat(MAX_CELL_CHARS + 100);
        let clamped = clamp_cell(long);
        assert_eq!(clamped.chars().count(), MAX_CELL_CHARS);
        assert!(clamped.ends_with(TRUNCATION_SUFFIX));
    }

    #[test]
    fn clamp_cell_respects_multibyte_boundaries() {
        let long = "日本語テスト🎌".repeat(MAX_CELL_CHARS / 4);
        let clamped = clamp_cell(long);
        assert!(clamped.chars().count() <= MAX_CELL_CHARS);
        assert!(clamped.ends_with(TRUNCATION_SUFFIX));
    }

    #[test]
    fn numbered_lines_formats_each_entry() {
        let items = vec!["first".to_string(), "second".to_string()];
        assert_eq!(numbered_lines(&items), "1. first\n2. second");
    }

    #[test]
    fn numbered_lines_empty_input_is_empty() {
        assert_eq!(numbered_lines(&[]), "");
    }

    #[test]
    fn section_name_resolves_for_both_variants() {
        let table = ExportSection::Table(ExportTable {
            name: "T".into(),
            columns: vec![],
            rows: vec![],
        });
        let kv = ExportSection::KeyValues(KeyValueSection {
            name: "K".into(),
            entries: vec![],
        });
        assert_eq!(table.name(), "T");
        assert_eq!(kv.name(), "K");
    }
}
