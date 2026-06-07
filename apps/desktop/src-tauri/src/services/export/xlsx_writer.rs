//! xlsx workbook writer over the export IR (`rust_xlsxwriter` — pure
//! Rust, no JS/zip shelling).
//!
//! Formatting (see `plan/ARTIFACT_EXPORT.md` §5):
//!
//! - One worksheet per section; names sanitized to Excel's rules
//!   (≤ 31 chars, no `[ ] : * ? / \`), deduped with a numeric suffix.
//! - Table sheets: bold header row with fill, frozen header
//!   (`set_freeze_panes(1, 0)`), autofilter over the table range.
//! - Column widths clamped to `[10, 60]` from the widest cell over
//!   the first ~100 rows; long / multiline cells get text-wrap.
//! - `KeyValues` sheets: bold Field column (width 24), wrapped Value
//!   column (width 80).
//!
//! `write_string` stores plain strings — xlsx never evaluates them as
//! formulas, so no apostrophe-prefixing here (that would corrupt the
//! data; the CSV/TSV writers handle their own injection escaping).

use std::path::Path;

use rust_xlsxwriter::{Color, DocProperties, Format, Workbook, Worksheet, XlsxError};

use crate::error::{AppError, AppResult};

use super::ir::{ExportDoc, ExportSection, ExportTable, KeyValueSection};

/// Excel's hard limit on worksheet-name length.
const MAX_SHEET_NAME_CHARS: usize = 31;
/// Column-width clamp bounds (Excel character units).
const MIN_COL_WIDTH: f64 = 10.0;
const MAX_COL_WIDTH: f64 = 60.0;
/// Rows sampled for the column-width heuristic.
const WIDTH_SAMPLE_ROWS: usize = 100;
/// Header fill — a light steel blue that prints legibly in grayscale.
const HEADER_FILL: Color = Color::RGB(0x00D9_E1F2);

/// Write the document to `dest_path` as a single workbook.
///
/// # Errors
///
/// Returns [`AppError::Internal`] when the xlsx engine rejects the
/// write (invalid path, disk full, etc.).
pub fn write_workbook(doc: &ExportDoc, dest_path: &Path) -> AppResult<()> {
    let mut workbook = build_workbook(doc)?;
    workbook.save(dest_path).map_err(|e| xlsx_err(&e))?;
    Ok(())
}

/// In-memory variant used by tests — same workbook bytes as
/// [`write_workbook`] without touching the filesystem.
///
/// # Errors
///
/// Returns [`AppError::Internal`] when the xlsx engine fails.
pub fn write_workbook_to_buffer(doc: &ExportDoc) -> AppResult<Vec<u8>> {
    let mut workbook = build_workbook(doc)?;
    workbook.save_to_buffer().map_err(|e| xlsx_err(&e))
}

fn build_workbook(doc: &ExportDoc) -> AppResult<Workbook> {
    let mut workbook = Workbook::new();
    workbook.set_properties(
        &DocProperties::new()
            .set_title(doc.title.as_str())
            .set_author("Tessera"),
    );

    let mut used_names: Vec<String> = Vec::with_capacity(doc.sections.len());
    for section in &doc.sections {
        let name = unique_sheet_name(section.name(), &used_names);
        used_names.push(name.clone());

        let worksheet = workbook.add_worksheet();
        worksheet.set_name(&name).map_err(|e| xlsx_err(&e))?;
        match section {
            ExportSection::Table(table) => write_table_sheet(worksheet, table)?,
            ExportSection::KeyValues(kv) => write_key_values_sheet(worksheet, kv)?,
        }
    }
    Ok(workbook)
}

fn write_table_sheet(worksheet: &mut Worksheet, table: &ExportTable) -> AppResult<()> {
    let header_format = Format::new().set_bold().set_background_color(HEADER_FILL);
    let wrap_format = Format::new().set_text_wrap();

    for (col, column) in table.columns.iter().enumerate() {
        worksheet
            .write_with_format(0, to_col(col), column.as_str(), &header_format)
            .map_err(|e| xlsx_err(&e))?;
    }

    for (row_idx, row) in table.rows.iter().enumerate() {
        let row_n = to_row(row_idx + 1);
        for (col, cell) in row.iter().enumerate() {
            if cell.contains('\n') {
                worksheet
                    .write_with_format(row_n, to_col(col), cell.as_str(), &wrap_format)
                    .map_err(|e| xlsx_err(&e))?;
            } else {
                worksheet
                    .write_string(row_n, to_col(col), cell.as_str())
                    .map_err(|e| xlsx_err(&e))?;
            }
        }
    }

    for (col, width) in column_widths(table) {
        worksheet
            .set_column_width(to_col(col), width)
            .map_err(|e| xlsx_err(&e))?;
    }

    worksheet.set_freeze_panes(1, 0).map_err(|e| xlsx_err(&e))?;
    if !table.columns.is_empty() {
        let last_row = to_row(table.rows.len().max(1));
        let last_col = to_col(table.columns.len() - 1);
        worksheet
            .autofilter(0, 0, last_row, last_col)
            .map_err(|e| xlsx_err(&e))?;
    }
    Ok(())
}

fn write_key_values_sheet(worksheet: &mut Worksheet, kv: &KeyValueSection) -> AppResult<()> {
    let field_format = Format::new().set_bold();
    let value_format = Format::new().set_text_wrap();

    for (row_idx, (field, value)) in kv.entries.iter().enumerate() {
        let row_n = to_row(row_idx);
        worksheet
            .write_with_format(row_n, 0, field.as_str(), &field_format)
            .map_err(|e| xlsx_err(&e))?;
        worksheet
            .write_with_format(row_n, 1, value.as_str(), &value_format)
            .map_err(|e| xlsx_err(&e))?;
    }
    worksheet.set_column_width(0, 24).map_err(|e| xlsx_err(&e))?;
    worksheet.set_column_width(1, 80).map_err(|e| xlsx_err(&e))?;
    Ok(())
}

/// Width heuristic: widest line (cells can be multiline) across the
/// header and the first [`WIDTH_SAMPLE_ROWS`] rows, clamped to
/// `[MIN_COL_WIDTH, MAX_COL_WIDTH]`.
fn column_widths(table: &ExportTable) -> Vec<(usize, f64)> {
    table
        .columns
        .iter()
        .enumerate()
        .map(|(col, header)| {
            let mut max_chars = widest_line(header);
            for row in table.rows.iter().take(WIDTH_SAMPLE_ROWS) {
                if let Some(cell) = row.get(col) {
                    max_chars = max_chars.max(widest_line(cell));
                }
            }
            // Small padding so text does not touch the column border.
            #[allow(clippy::cast_precision_loss)] // widths are tiny vs f64 mantissa
            let width = (max_chars as f64 + 2.0).clamp(MIN_COL_WIDTH, MAX_COL_WIDTH);
            (col, width)
        })
        .collect()
}

fn widest_line(cell: &str) -> usize {
    cell.lines().map(|l| l.chars().count()).max().unwrap_or(0)
}

/// Sanitize a section name into a legal, unique worksheet name.
pub(super) fn unique_sheet_name(raw: &str, used: &[String]) -> String {
    let base = sanitize_sheet_name(raw);
    if !used.iter().any(|u| u.eq_ignore_ascii_case(&base)) {
        return base;
    }
    for n in 2.. {
        let suffix = format!(" {n}");
        let keep = MAX_SHEET_NAME_CHARS.saturating_sub(suffix.chars().count());
        let candidate: String = base.chars().take(keep).collect::<String>() + &suffix;
        if !used.iter().any(|u| u.eq_ignore_ascii_case(&candidate)) {
            return candidate;
        }
    }
    unreachable!("numeric suffixes are unbounded");
}

fn sanitize_sheet_name(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .filter(|c| !matches!(c, '[' | ']' | ':' | '*' | '?' | '/' | '\\'))
        .collect();
    let trimmed = cleaned.trim().trim_matches('\'').trim();
    let truncated: String = trimmed.chars().take(MAX_SHEET_NAME_CHARS).collect();
    let final_name = truncated.trim().to_string();
    if final_name.is_empty() {
        "Sheet".to_string()
    } else {
        final_name
    }
}

fn xlsx_err(err: &XlsxError) -> AppError {
    AppError::Internal(anyhow::anyhow!("xlsx write failed: {err}"))
}

#[allow(clippy::cast_possible_truncation)] // bounded by Excel's 1,048,576-row / 16,384-col limits upstream
fn to_row(idx: usize) -> u32 {
    idx as u32
}

#[allow(clippy::cast_possible_truncation)] // column counts come from fixed mapper layouts (≤ 13)
fn to_col(idx: usize) -> u16 {
    idx as u16
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::export::ir::{ExportTable, KeyValueSection};

    fn sample_doc() -> ExportDoc {
        ExportDoc {
            title: "Sample".into(),
            sections: vec![
                ExportSection::Table(ExportTable {
                    name: "Test Cases".into(),
                    columns: vec!["ID".into(), "Steps".into()],
                    rows: vec![vec!["TC-1".into(), "1. one\n2. two".into()]],
                }),
                ExportSection::KeyValues(KeyValueSection {
                    name: "Plan".into(),
                    entries: vec![("Summary".into(), "ok".into())],
                }),
            ],
        }
    }

    #[test]
    fn buffer_output_is_nonempty_zip() {
        let bytes = write_workbook_to_buffer(&sample_doc()).expect("buffer");
        // xlsx is a zip container — check the local-file-header magic.
        assert!(bytes.len() > 4);
        assert_eq!(&bytes[..4], b"PK\x03\x04");
    }

    #[test]
    fn empty_table_still_produces_workbook() {
        let doc = ExportDoc {
            title: "Empty".into(),
            sections: vec![ExportSection::Table(ExportTable {
                name: "Findings".into(),
                columns: vec!["ID".into()],
                rows: vec![],
            })],
        };
        let bytes = write_workbook_to_buffer(&doc).expect("buffer");
        assert_eq!(&bytes[..4], b"PK\x03\x04");
    }

    #[test]
    fn sheet_names_strip_illegal_chars() {
        assert_eq!(sanitize_sheet_name("a[b]c:d*e?f/g\\h"), "abcdefgh");
    }

    #[test]
    fn sheet_names_truncate_to_31_chars() {
        let long = "x".repeat(50);
        assert_eq!(sanitize_sheet_name(&long).chars().count(), 31);
    }

    #[test]
    fn empty_sheet_name_falls_back() {
        assert_eq!(sanitize_sheet_name("[]:*?/\\"), "Sheet");
        assert_eq!(sanitize_sheet_name("   "), "Sheet");
    }

    #[test]
    fn duplicate_sheet_names_get_numeric_suffix() {
        let used = vec!["Cases".to_string()];
        assert_eq!(unique_sheet_name("Cases", &used), "Cases 2");
        let used = vec!["Cases".to_string(), "Cases 2".to_string()];
        assert_eq!(unique_sheet_name("Cases", &used), "Cases 3");
    }

    #[test]
    fn duplicate_long_names_stay_within_limit() {
        let long = "y".repeat(40);
        let first = sanitize_sheet_name(&long);
        let second = unique_sheet_name(&long, std::slice::from_ref(&first));
        assert_ne!(first, second);
        assert!(second.chars().count() <= MAX_SHEET_NAME_CHARS);
    }

    #[test]
    fn column_width_heuristic_clamps_both_ends() {
        let table = ExportTable {
            name: "W".into(),
            columns: vec!["a".into(), "Header".into()],
            rows: vec![vec!["x".into(), "z".repeat(200)]],
        };
        let widths = column_widths(&table);
        assert!((widths[0].1 - MIN_COL_WIDTH).abs() < f64::EPSILON);
        assert!((widths[1].1 - MAX_COL_WIDTH).abs() < f64::EPSILON);
    }

    #[test]
    fn width_heuristic_uses_widest_line_not_total_length() {
        let table = ExportTable {
            name: "W".into(),
            columns: vec!["Steps".into()],
            rows: vec![vec!["1. short\n2. also short".to_string()]],
        };
        let widths = column_widths(&table);
        assert!(widths[0].1 < MAX_COL_WIDTH);
    }
}
