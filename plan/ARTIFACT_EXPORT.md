# Artifact Export — Excel / CSV / Google Sheets first, Jira next

> Status: **shipped** — Phases 1–2 merged (xlsx/csv/tsv #56, Markdown/JSON #59); Phase 3 (Jira) shipped via [`JIRA_INTEGRATION.md`](./JIRA_INTEGRATION.md) (#62) · Owner: core · Created: 2026-06-06

## 1. Why

QA/QT practitioners we talked to overwhelmingly work in Jira; artifact export is table-stakes for Tessera adoption. [`ROADMAP.md`](./ROADMAP.md) lists "No export integrations" as a known gap. Strategy:

1. **Ship file export first** — Excel (`.xlsx`) + CSV/TSV. Zero auth, zero network, fully local-first. Covers Google Sheets (xlsx/csv import + TSV paste) and even Jira's built-in CSV import on day one.
2. **Jira API integration next** — definite, committed scope (Phase 3). The architecture below is shaped so the Jira adapter drops in without rework.

## 2. Goal + scope

In scope (Phases 1–2):

- Export any generated artifact to `.xlsx`, `.csv`, `.tsv` via save dialog.
- "Copy as TSV" clipboard action — paste straight into Google Sheets, no auth.
- All five artifact types: `context_md`, `test_plan`, `test_cases`, `defect_report`, `bug_report`.

Out of scope:

- Google Sheets native OAuth API — covered by xlsx/csv import + TSV paste; revisit only on user demand.
- Jira API push — Phase 3 (see §9).
- Bulk/multi-artifact export — single artifact per export in v1.

## 3. Architecture

Flow (Rust-side generation — no large JS deps, no base64 shuttling, respects commands → services layering):

```
FE save dialog (@tauri-apps/plugin-dialog save(), pattern in lib/export-markdown.ts)
  → IPC export_artifact(artifactId, format, destPath)
    → services/export: fetch artifact → mapper → ExportDoc IR → format writer → write file
```

### The IR — single seam for every destination

One mapper per artifact type produces a writer-agnostic `ExportDoc`. Every output format (csv, tsv, xlsx — and later the Jira adapter) consumes only the IR. This kills the N×M (artifact types × destinations) problem.

```rust
pub struct ExportDoc {
    pub title: String,
    pub sections: Vec<ExportSection>,
}

pub enum ExportSection {
    Table(ExportTable),          // tabular artifacts
    KeyValues(KeyValueSection),  // prose artifacts
}

pub struct ExportTable {
    pub name: String,            // sheet name / csv sibling suffix
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,  // cells pre-flattened to strings
}

pub struct KeyValueSection {
    pub name: String,
    pub entries: Vec<(String, String)>,
}
```

Cells are pre-flattened `String`: arrays become numbered newline-joined lines (`"1. …\n2. …"`); nested objects (e.g. bug `root_cause`) flatten to labelled lines.

### Per-type mapping

Artifact type enum: `artifact_repo.rs:24-44`. Payload shapes: `prompts/*_v1.rs`.

| Artifact | IR shape |
|---|---|
| `test_cases` | Table "Test Cases": ID, Title, Priority, Preconditions, Steps, Expected Result, Traceability. Optional second Table "Files" when runnable workspace present |
| `defect_report` | Table "Findings": #, Severity, Category, Location, Description, Suggested Fix |
| `bug_report` | Table "Bugs": ID, Title, Severity, Environment, Steps to Reproduce, Expected Behavior, Actual Behavior, Root Cause (flattened), Evidence |
| `test_plan` | KeyValues "Test Plan": Summary, Strategy single entries; Objectives / Scope In / Scope Out / Environments / Risks / Entry / Exit Criteria bullet-joined |
| `context_md` | KeyValues "Context": Summary, Architecture Notes, Key Modules, Data Flows, Known Risks |

Mappers deserialize `structured_data` into local structs with `#[serde(default)]` so missing/extra fields never panic (legacy-row tolerant). A fully empty payload (`null` / `{}`) returns `AppError::InvalidInput("artifact has no structured data to export")`; the FE toast suggests "Export markdown" instead.

## 4. Module layout

Rust — `apps/desktop/src-tauri/src/`:

| File | Action | Contents |
|---|---|---|
| `services/export/mod.rs` | new | orchestration (`export_artifact`), `ExportFormat` enum, dest-path validation |
| `services/export/ir.rs` | new | `ExportDoc` + section structs |
| `services/export/mappers.rs` | new | per-type payload structs + `build_export_doc()` |
| `services/export/csv_writer.rs` | new | CSV/TSV writer over `impl io::Write` + injection escaping + `render_tsv()` |
| `services/export/xlsx_writer.rs` | new | workbook writer, sheet-name sanitizer, column-width heuristic |
| `services/mod.rs` | modify | `pub mod export;` |
| `commands/exports.rs` | new | thin `export_artifact` + `get_artifact_tsv` — owned args, `Result<T, String>`, `.map_err(\|e\| e.to_string())` (mirror `commands/artifacts.rs`) |
| `commands/mod.rs`, `lib.rs` | modify | register module + handlers in `generate_handler!` |
| `Cargo.toml` | modify | add `rust_xlsxwriter`, `csv = "1"` |

TypeScript:

| File | Action | Contents |
|---|---|---|
| `packages/shared/src/schemas/export.schema.ts` | new | `ExportFormatSchema` (`'xlsx' \| 'csv' \| 'tsv'`), `ExportOutcomeSchema` (`{ files: string[] }` — CSV can emit siblings) + contract test; export from `index.ts` |
| `apps/desktop/src/lib/ipc/exports.ts` | new | `exportArtifact()`, `getArtifactTsv()` typed wrappers |
| `apps/desktop/src/lib/export-artifact.ts` | new | save-dialog orchestration per format; generalize filename slug from `export-markdown.ts:7-17` into `buildExportFilename(title, ext)` |
| `apps/desktop/src/components/ai-panel/artifact-detail-drawer.tsx` | modify | replace "Export markdown" button (footer, ~line 381) with dropdown: Markdown / Excel (.xlsx) / CSV / Copy as TSV |

Copy-as-TSV: FE calls `get_artifact_tsv` then `navigator.clipboard.writeText` (existing pattern in boards components — no plugin needed). TSV always rendered Rust-side so mapping logic never duplicates in TS.

### Command signatures

```rust
// services/export/mod.rs
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat { Xlsx, Csv, Tsv }

pub async fn export_artifact(pool: &SqlitePool, artifact_id: &str,
    format: ExportFormat, dest_path: &Path) -> AppResult<Vec<PathBuf>>;
pub fn build_export_doc(artifact: &Artifact) -> AppResult<ExportDoc>; // pure — the Jira seam
pub fn render_tsv(doc: &ExportDoc) -> String;
```

```ts
// lib/ipc/exports.ts
exportArtifact(artifactId: string, format: ExportFormat, destPath: string): Promise<ExportOutcome>
getArtifactTsv(artifactId: string): Promise<string>
```

## 5. Format specifics

CSV:

- UTF-8 BOM (`EF BB BF`) — Excel on Windows misdetects BOM-less UTF-8; Sheets ignores it.
- CRLF line endings (RFC 4180 / Excel), csv-crate default quoting.
- Multi-section docs: save dialog yields one path; additional sections written as `{stem}.{section-slug}.csv` siblings (slug `[a-z0-9-]`, Windows reserved names avoided). All written paths returned and listed in the success toast.
- KeyValues sections render as two-column `Field,Value` CSVs.

TSV (file + clipboard): same writer with `\t` delimiter; clipboard variant has no BOM; sections concatenate with a blank line.

xlsx (`rust_xlsxwriter` — pure Rust, `save_to_buffer()` for tests):

- One worksheet per section; sheet names sanitized (≤31 chars, strip `[]:*?/\`, dedupe with numeric suffix).
- Header row bold with fill, `set_freeze_panes(1, 0)`, autofilter over table range.
- Column widths clamped `[10, 60]` from max cell width over first ~100 rows; text-wrap on long columns.
- KeyValues sheets: bold Field column (~24), wrapped Value column (~80).
- Workbook properties: title = artifact title, application = "Tessera".

Security / robustness:

- **Formula injection**: apostrophe-prefix cells starting `=`, `+`, `-`, `@`, tab, CR in CSV/TSV writers. xlsx `write_string` never evaluates formulas — no prefix there, data stays clean.
- **Dest-path validation (Rust-side — any FE code can invoke the command)**: must be absolute; reject NUL; parent must exist; canonicalize parent + re-join filename to neutralize `..`; reject existing directory; enforce/append format extension; IO errors → `AppError::Io`, no `unwrap`.
- **Cell cap**: 32,767 chars (xlsx hard limit), truncate with `… (truncated)`; same cap in CSV for consistency.
- Empty arrays still emit the header row (valid empty table). Unicode/CJK/emoji covered by tests.

## 6. Testing (per `rules/rules.md` — 80% services coverage, same-file `#[cfg(test)]`)

- **Mappers** (highest value): feed sample `structured_data` JSON (reuse examples embedded in `prompts/*_v1.rs`) → `insta` yaml snapshots of `ExportDoc`; explicit null / `{}` / missing-field cases.
- **CSV/TSV writer**: write into `Cursor<Vec<u8>>`, assert exact bytes — BOM, CRLF, quoting, injection escaping, unicode round-trip.
- **xlsx (pragmatic)**: `save_to_buffer()` → non-empty + zip magic `PK\x03\x04`; unit-test pure helpers (sheet-name sanitizer, width heuristic, truncation). No xlsx-reading dev dependency.
- **Path validation**: table-driven (relative rejected, `..` neutralized, extension appended, directory rejected).
- **FE**: vitest for `buildExportFilename` + IPC wrapper parsing; shared contract tests for the new Zod schemas.
- **E2E**: deferred — new flow writes Rust-side, Playwright would need command mocking; unit coverage is the gate.

## 7. Phasing

### Phase 1 — Rust export engine

Cargo deps; `services/export/` (IR, all 5 mappers, CSV/TSV writers, xlsx writer, path validation); `commands/exports.rs` + registration; full unit/snapshot tests; clippy-pedantic clean.
**Done =** both commands callable and verified end-to-end from a Rust test.

### Phase 2 — Contract + frontend

`export.schema.ts` in `packages/shared`; `lib/ipc/exports.ts`; `lib/export-artifact.ts` (dialog filters per format); drawer footer export dropdown (Markdown / Excel / CSV / Copy as TSV) with loading state, success toast listing every written file, and "no structured data" fallback messaging; FE + shared tests.
**Done =** user exports each artifact type to xlsx/csv and pastes TSV into Google Sheets.

### Phase 3 — Jira (committed, design later)

Definite scope, separate design doc when picked up. Locked-in direction:

- Jira Cloud REST v3 adapter consuming the **same `ExportDoc` IR** (`build_export_doc` is the seam; extend IR with optional typed field metadata only if Jira requires it).
- Auth: email + API token (Basic) — standard for QA tooling, no OAuth app review; token encrypted at rest via existing AES-256-GCM key infra (`auth/`).
- `POST /rest/api/3/issue/bulk` (50/batch); markdown → ADF (Atlassian Document Format) converter for descriptions.
- Issue-key writeback per artifact (SQLite migration) → re-export updates instead of duplicating; artifact view shows linked key.
- Later option: Xray / Zephyr Scale adapters for native test-case entities (plain `Bug`/`Task` issues cover v1).
- Interim bridge available from Phase 1: "Jira-ready CSV" column template (Summary, Issue Type, Priority, Description) works with Jira's built-in External System Import — zero auth.

## 8. Risks / edge cases

| Risk | Mitigation |
|---|---|
| Formula injection via generated content | apostrophe-prefix in CSV/TSV (§5) |
| Legacy rows with null `structured_data` | serde defaults + explicit empty-doc error |
| Huge cells (long steps/evidence) | 32,767-char truncation with suffix |
| Sheet/filename collisions, illegal chars, Windows reserved names | sanitizers + numeric dedupe |
| Silent sibling-CSV overwrite (dialog confirms only primary path) | success toast lists every file written — accepted v1 tradeoff |
| Unicode mangling in Excel | UTF-8 BOM + CJK/emoji tests |
