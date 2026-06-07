//! CSV / TSV writers over the export IR.
//!
//! Format choices (see `plan/ARTIFACT_EXPORT.md` §5):
//!
//! - CSV files start with a UTF-8 BOM — Excel on Windows misdetects
//!   BOM-less UTF-8; Google Sheets ignores the BOM.
//! - CRLF record terminator (RFC 4180 / Excel default).
//! - Spreadsheet-formula injection is neutralized by prefixing any
//!   cell that starts with `=`, `+`, `-`, `@`, TAB, or CR with an
//!   apostrophe. Excel/Sheets treat the apostrophe as a
//!   literal-text marker instead of evaluating the payload.
//! - `KeyValues` sections render as two-column `Field,Value` tables.

use std::io::Write;

use crate::error::{AppError, AppResult};

use super::ir::{ExportDoc, ExportSection, ExportTable, KeyValueSection};

/// UTF-8 byte-order mark emitted at the start of CSV/TSV files.
pub const UTF8_BOM: &[u8] = &[0xEF, 0xBB, 0xBF];

/// Prefix cells that a spreadsheet would interpret as a formula with
/// an apostrophe so pasted/imported content can never execute.
#[must_use]
pub fn escape_formula_injection(cell: &str) -> String {
    match cell.chars().next() {
        Some('=' | '+' | '-' | '@' | '\t' | '\r') => format!("'{cell}"),
        _ => cell.to_string(),
    }
}

/// Write one section as delimiter-separated values.
///
/// `bom` controls the UTF-8 BOM (true for files consumed by Excel,
/// false for the clipboard TSV variant).
///
/// # Errors
///
/// Returns [`AppError::Io`] when the underlying writer fails.
pub fn write_section<W: Write>(
    writer: W,
    section: &ExportSection,
    delimiter: u8,
    bom: bool,
) -> AppResult<()> {
    match section {
        ExportSection::Table(table) => write_table(writer, table, delimiter, bom),
        ExportSection::KeyValues(kv) => write_key_values(writer, kv, delimiter, bom),
    }
}

fn write_table<W: Write>(
    mut writer: W,
    table: &ExportTable,
    delimiter: u8,
    bom: bool,
) -> AppResult<()> {
    if bom {
        writer.write_all(UTF8_BOM)?;
    }
    let mut csv = builder(delimiter).from_writer(writer);
    csv.write_record(table.columns.iter().map(|c| escape_formula_injection(c)))?;
    for row in &table.rows {
        csv.write_record(row.iter().map(|c| escape_formula_injection(c)))?;
    }
    csv.flush()?;
    Ok(())
}

fn write_key_values<W: Write>(
    mut writer: W,
    kv: &KeyValueSection,
    delimiter: u8,
    bom: bool,
) -> AppResult<()> {
    if bom {
        writer.write_all(UTF8_BOM)?;
    }
    let mut csv = builder(delimiter).from_writer(writer);
    csv.write_record(["Field", "Value"])?;
    for (field, value) in &kv.entries {
        csv.write_record([
            escape_formula_injection(field),
            escape_formula_injection(value),
        ])?;
    }
    csv.flush()?;
    Ok(())
}

fn builder(delimiter: u8) -> csv::WriterBuilder {
    let mut b = csv::WriterBuilder::new();
    b.delimiter(delimiter)
        .terminator(csv::Terminator::CRLF)
        .quote_style(csv::QuoteStyle::Necessary);
    b
}

/// Render a whole document as clipboard-ready TSV: no BOM, sections
/// concatenated with a blank line, each section preceded by its name
/// when the document has more than one.
///
/// Rendering into an in-memory buffer cannot fail; any unexpected
/// writer error degrades to an empty string rather than poisoning
/// the clipboard path.
#[must_use]
pub fn render_tsv(doc: &ExportDoc) -> String {
    let mut parts: Vec<String> = Vec::with_capacity(doc.sections.len());
    for section in &doc.sections {
        let mut buf: Vec<u8> = Vec::new();
        if write_section(&mut buf, section, b'\t', false).is_err() {
            continue;
        }
        let body = String::from_utf8_lossy(&buf).into_owned();
        if doc.sections.len() > 1 {
            parts.push(format!("{}\r\n{}", section.name(), body));
        } else {
            parts.push(body);
        }
    }
    parts.join("\r\n")
}

impl From<csv::Error> for AppError {
    fn from(err: csv::Error) -> Self {
        match err.into_kind() {
            csv::ErrorKind::Io(io_err) => Self::Io(io_err),
            other => Self::Internal(anyhow::anyhow!("csv write failed: {other:?}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::export::ir::{ExportTable, KeyValueSection};

    fn sample_table() -> ExportSection {
        ExportSection::Table(ExportTable {
            name: "Cases".into(),
            columns: vec!["ID".into(), "Title".into()],
            rows: vec![
                vec!["TC-1".into(), "first".into()],
                vec!["TC-2".into(), "has,comma".into()],
            ],
        })
    }

    #[test]
    fn csv_starts_with_bom_and_uses_crlf() {
        let mut buf: Vec<u8> = Vec::new();
        write_section(&mut buf, &sample_table(), b',', true).expect("write");
        assert_eq!(&buf[..3], UTF8_BOM);
        let text = String::from_utf8(buf[3..].to_vec()).expect("utf8");
        assert_eq!(text, "ID,Title\r\nTC-1,first\r\nTC-2,\"has,comma\"\r\n");
    }

    #[test]
    fn tsv_variant_has_no_bom_and_tab_delimiter() {
        let mut buf: Vec<u8> = Vec::new();
        write_section(&mut buf, &sample_table(), b'\t', false).expect("write");
        let text = String::from_utf8(buf).expect("utf8");
        assert!(text.starts_with("ID\tTitle\r\n"));
    }

    #[test]
    fn formula_cells_get_apostrophe_prefix() {
        let section = ExportSection::Table(ExportTable {
            name: "Inj".into(),
            columns: vec!["A".into()],
            rows: vec![
                vec!["=HYPERLINK(\"http://evil\")".into()],
                vec!["+1".into()],
                vec!["-1".into()],
                vec!["@cmd".into()],
                vec!["safe".into()],
            ],
        });
        let mut buf: Vec<u8> = Vec::new();
        write_section(&mut buf, &section, b',', false).expect("write");
        let text = String::from_utf8(buf).expect("utf8");
        assert!(text.contains("'=HYPERLINK"));
        assert!(text.contains("'+1"));
        assert!(text.contains("'-1"));
        assert!(text.contains("'@cmd"));
        assert!(text.contains("\r\nsafe\r\n"));
    }

    #[test]
    fn key_values_render_as_field_value_rows() {
        let section = ExportSection::KeyValues(KeyValueSection {
            name: "Plan".into(),
            entries: vec![("Summary".into(), "All good".into())],
        });
        let mut buf: Vec<u8> = Vec::new();
        write_section(&mut buf, &section, b',', false).expect("write");
        let text = String::from_utf8(buf).expect("utf8");
        assert_eq!(text, "Field,Value\r\nSummary,All good\r\n");
    }

    #[test]
    fn multiline_cells_are_quoted() {
        let section = ExportSection::Table(ExportTable {
            name: "Steps".into(),
            columns: vec!["Steps".into()],
            rows: vec![vec!["1. one\n2. two".into()]],
        });
        let mut buf: Vec<u8> = Vec::new();
        write_section(&mut buf, &section, b',', false).expect("write");
        let text = String::from_utf8(buf).expect("utf8");
        assert!(text.contains("\"1. one\n2. two\""));
    }

    #[test]
    fn unicode_round_trips() {
        let section = ExportSection::Table(ExportTable {
            name: "U".into(),
            columns: vec!["Text".into()],
            rows: vec![vec!["日本語 🎌 ümlaut".into()]],
        });
        let mut buf: Vec<u8> = Vec::new();
        write_section(&mut buf, &section, b',', true).expect("write");
        let text = String::from_utf8(buf[3..].to_vec()).expect("utf8");
        assert!(text.contains("日本語 🎌 ümlaut"));
    }

    #[test]
    fn render_tsv_single_section_has_no_name_header() {
        let doc = ExportDoc {
            title: "t".into(),
            sections: vec![sample_table()],
        };
        let tsv = render_tsv(&doc);
        assert!(tsv.starts_with("ID\tTitle"));
        assert!(!tsv.contains("Cases"));
    }

    #[test]
    fn render_tsv_multi_section_names_and_separates_sections() {
        let doc = ExportDoc {
            title: "t".into(),
            sections: vec![
                sample_table(),
                ExportSection::KeyValues(KeyValueSection {
                    name: "Summary".into(),
                    entries: vec![("Summary".into(), "ok".into())],
                }),
            ],
        };
        let tsv = render_tsv(&doc);
        assert!(tsv.starts_with("Cases\r\nID\tTitle"));
        assert!(tsv.contains("\r\n\r\nSummary\r\nField\tValue"));
    }
}
