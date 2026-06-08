# Test Case Table — fixed 9-column tabular view

**Status:** proposed
**Owner:** TBD
**Created:** 2026-06-08

## 1. Goal

Render the **Test Cases** artifact as a proper table with a fixed, industry-standard
column set instead of the current card layout. The LLM decides how many cases (rows)
to emit; the columns are fixed. Two of the columns are execution outcomes the LLM
cannot know at generation time — they are filled by the sandbox test runner or typed
by a human tester, and must survive artifact regeneration.

### Target columns (fixed, in order)

| # | Column              | Origin                                            |
|---|---------------------|---------------------------------------------------|
| 1 | Sr no               | auto row index (1..N), not stored                 |
| 2 | Test case ID        | `case.id`                                          |
| 3 | Description         | `case.title`                                      |
| 4 | Precondition        | `case.preconditions[]` (numbered lines in cell)   |
| 5 | Steps to reproduce  | `case.steps[].action` (numbered lines in cell)    |
| 6 | Input steps         | `case.testData`                                   |
| 7 | Expected output     | `case.steps[].expectedResult` (aggregated)        |
| 8 | Actual output       | **sidecar** — sandbox run or manual entry         |
| 9 | Result and remarks  | **sidecar** — sandbox run or manual entry         |

### Optional toggle columns (off by default, kept in data)

`type`, `priority`, `traceability` — not in the core 9 but still generated, stored,
and exported. A view toggle reveals them as extra trailing columns.

## 2. Decisions (locked)

1. **Cols 8–9 = editable cells + sandbox auto-fill.** Blank (`—`) at generation. A
   tester can type Actual output / set Result (Pass/Fail/Blocked) + remarks; an opt-in
   Docker run auto-fills the same cells.
2. **Extra fields (type/priority/traceability) = optional toggle columns.** Stay in the
   schema, prompt, storage, and export — hidden from the core table by default.
3. **Table replaces the card view.** `TestCasesView` card layout is removed; the table
   is the only test-cases display.

## 3. Constraints / non-goals

- **LLM prompt schema (`test_cases_v2`) is unchanged** except one additive rule (§5.4)
  so sandbox results can be matched to cases. No new generated fields.
- `generation_service` flow, sandbox security model, and the runnable-`files[]` contract
  are untouched.
- This is a presentation change + a results sidecar + a name→id bridge. Not a generation
  rewrite.

## 4. Architecture

### 4.1 Why a sidecar (not structured_data)

Cols 8–9 are mutable, user/runtime-owned data. The LLM artifact's `structured_data`
JSON is **regenerated** on every re-run, which would wipe manual edits and sandbox
results. So outcomes live in a separate `test_case_results` table keyed by
`(artifact_id, case_id)`. The table view is the LLM cases **LEFT JOIN** this sidecar.
Regeneration keeps the same `case.id` values → results re-attach automatically.

### 4.2 The name→id bridge (critical gotcha)

`test_run_cases.name` holds the **vitest assertion title**, not the `TC-…` id. There is
currently no link from an executed assertion back to the test case it exercises. To
auto-fill cols 8–9 from a sandbox run we need that link.

**Chosen approach:** require every generated spec to name its top-level test with the
case id as a leading token, e.g. `it('TC-LOGIN-01 rejects empty password', …)`. The
sandbox-result mapper parses the leading `^TC-[A-Z0-9_-]+` token from
`test_run_cases.name`; unmatched assertions are ignored for auto-fill (still recorded as
today in `test_run_cases`). If multiple assertions share a TC-id, the case is `failed`
if any fail, else `passed`; `actual_output` = concatenated `failure_message`s (or
"All N assertions passed.").

This is the only prompt-side change and it is additive (a naming rule, no schema field).

### 4.3 Data flow

```
generate ─► structured_data.cases[]          (LLM, unchanged)
                     │
   table view  ◄─────┼──── LEFT JOIN ────► test_case_results            (sidecar)
                     │                          ▲          ▲
manual edit ─────────┼──────────────────────────┘          │ source=manual
                     │                                      │
sandbox run ─► test_run_cases (name) ─► name→id bridge ─────┘ source=sandbox
```

## 5. Implementation phases

> Sizing per repo convention: 2–3 large phases, each independently codeable by an agent.

### Phase 1 — Backend: results sidecar + sandbox bridge

**Migration** `apps/desktop/src-tauri/migrations/0007_test_case_results.sql`
(next free number; 0006 is the latest). Follow 0004 conventions exactly: TEXT UUID PK,
`created_at`/`updated_at` RFC-3339, FK to `artifacts(id) ON DELETE CASCADE`, index every
FK and every WHERE/ORDER BY column, parameterized SQL only.

```sql
CREATE TABLE test_case_results (
    id            TEXT PRIMARY KEY NOT NULL,
    artifact_id   TEXT NOT NULL,
    case_id       TEXT NOT NULL,              -- the TC-… id from structured_data
    actual_output TEXT,
    -- pass | fail | blocked | not_run
    result        TEXT NOT NULL DEFAULT 'not_run',
    remarks       TEXT,
    -- manual | sandbox
    source        TEXT NOT NULL DEFAULT 'manual',
    run_id        TEXT,                        -- nullable FK to test_runs for sandbox rows
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL,
    FOREIGN KEY (artifact_id) REFERENCES artifacts(id) ON DELETE CASCADE,
    FOREIGN KEY (run_id) REFERENCES test_runs(id) ON DELETE SET NULL,
    UNIQUE (artifact_id, case_id)             -- one current outcome per case
);
CREATE INDEX idx_tc_results_artifact ON test_case_results(artifact_id);
CREATE INDEX idx_tc_results_run ON test_case_results(run_id);
```

`UNIQUE(artifact_id, case_id)` → upsert semantics: a manual edit and a later sandbox run
overwrite the same row (last writer wins; `source` records which).

**Repository** `repositories/test_case_result_repo.rs` (SQL only, no business logic):
- `upsert(artifact_id, case_id, fields…)` — `INSERT … ON CONFLICT(artifact_id, case_id) DO UPDATE`.
- `list_by_artifact(artifact_id) -> Vec<TestCaseResultRow>` — single query, no N+1.
- `batch_upsert(rows)` — used by the sandbox mapper to write all cases of a run at once.

**Commands** `commands/test_case_results.rs` (thin; owned args; `Result<T, String>`):
- `list_test_case_results(artifact_id: String)`
- `upsert_test_case_result(input: UpsertTestCaseResultInput)`
- Register in the Tauri builder. Silence `needless_pass_by_value` at each fn per §4.2.1.

**Service hook** — extend `sandbox_service.rs` so that after a run completes it:
1. loads the artifact's `cases[]` ids,
2. parses leading `TC-…` token from each `test_run_cases.name` (§4.2),
3. folds assertions per case → `result` + `actual_output`,
4. `batch_upsert` with `source = 'sandbox'`, `run_id = <run>`.
Keep this in the service layer (no SQL in service — call the repo).

**Shared schema** `packages/shared/src/schemas/test-case-result.schema.ts`:
- `TestCaseResultResultSchema = z.union([z.literal('pass'), z.literal('fail'), z.literal('blocked'), z.literal('not_run')])`
- `TestCaseResultSourceSchema = z.union([z.literal('manual'), z.literal('sandbox')])`
- `TestCaseResultSchema` mirroring the serde struct (Rust serde = source of truth).
- `UpsertTestCaseResultInputSchema`.
- Round-trip contract test alongside, per §12.3.1.

**Rust serde struct** for the row + upsert input, `#[serde(rename_all = "camelCase")]` to
match the Zod camelCase. Enum discriminators must equal the Zod literals exactly.

**Tests:** repo upsert/conflict test (in-file `#[cfg(test)]`); sandbox mapper unit test
with a scripted `test_run_cases` set covering: matched single, matched multi (one fail),
unmatched assertion ignored.

### Phase 2 — Frontend: 9-column table view

**IPC wrappers** in `src/lib/ipc/` for the two new commands — Zod-validate payloads
against `@testing-ide/shared`. No raw `invoke()` outside `ipc/`.

**Component** `TestCaseTable` (replaces `TestCasesView` in
`components/ai-panel/artifact-structured-view.tsx`):
- Real `<table>`; one row per `data.cases[]` entry. Sr no = render index + 1.
- Multi-line cells (Precondition, Steps to reproduce) render numbered lists inside the
  cell. Expected output = numbered `expectedResult`s, mirroring `mappers.rs` aggregation.
- Cols 8–9 are editable: Actual output = textarea; Result = select
  (Pass/Fail/Blocked/Not run) + remarks input. On change → debounced
  `upsertTestCaseResult` IPC. Optimistic local state; reconcile on response.
- On mount, `listTestCaseResults(artifactId)` → map `case_id → {actual, result, remarks, source}`.
  Badge sandbox-sourced rows so a tester sees machine vs hand-entered.
- Optional columns toggle (off by default): appends `type`, `priority`, `traceability`.
- Remove the old card `TestCasesView`; update the artifact-type switch to the new component.

**State:** small Zustand slice or local component state keyed by artifact id; results are
per-artifact and refetched when the open artifact changes.

**Tests:** Vitest render — N cases → N rows + header; editing Result fires one debounced
upsert; sandbox-sourced rows show the badge.

### Phase 3 — Export + prompt alignment

**Export** `services/export/mappers.rs::map_test_cases` — re-map the column set to the
exact 9 (rename `Title`→`Description`, `Test Data`→`Input steps`, `Steps`→`Steps to
reproduce`, `Expected Result`→`Expected output`) and **add** `Actual output` +
`Result and remarks`, pulling those two from a `test_case_results` join passed into the
mapper. Keep `type`/`priority`/`traceability` as trailing columns so export stays a
superset. Mirror header changes in `markdown_writer.rs::render_test_case`.
Update export snapshot/unit tests.

**Prompt** `prompts/test_cases_v2.rs` — add the §4.2 naming rule to the instruction text:
"each generated spec's top-level `it`/`test` title MUST begin with the case `id` token."
Bump the prompt `VERSION` and update the `insta` snapshot. No tool-schema field change.

## 6. Affected files (map)

| Area | File | Change |
|------|------|--------|
| Migration | `migrations/0007_test_case_results.sql` | new |
| Repo | `repositories/test_case_result_repo.rs` | new |
| Command | `commands/test_case_results.rs` | new + register |
| Service | `services/sandbox_service.rs` | name→id fold + batch upsert |
| Shared | `packages/shared/src/schemas/test-case-result.schema.ts` | new + round-trip test |
| FE IPC | `src/lib/ipc/test-case-results.ts` | new |
| FE view | `components/ai-panel/artifact-structured-view.tsx` | replace `TestCasesView` |
| Export | `services/export/mappers.rs` | remap headers + 2 cols |
| Export | `services/export/markdown_writer.rs` | mirror headers |
| Prompt | `prompts/test_cases_v2.rs` | naming rule + VERSION bump + snapshot |

## 7. Risks / open questions

- **Name→id reliability.** Depends on the model honoring the spec-naming rule. Mitigation:
  parse leniently (leading token only); unmatched assertions degrade to "no auto-fill",
  never an error. A future hardening could emit the TC-id mapping as run metadata instead
  of parsing names.
- **Regeneration churn.** If a regen drops or renames a `case.id`, its sidecar row
  orphans (kept, just unjoined). Acceptable; a cleanup pass can prune orphans by
  `artifact_id` if it becomes noise.
- **Result enum naming.** Using `pass|fail|blocked|not_run`. Confirm these match the
  team's manual-test vocabulary before locking the migration.

## 8. Out of scope (future)

- Bulk actions (mark all pass, clear results).
- Per-step actual/result granularity (current design is case-level).
- Exporting the sidecar as its own sheet vs inline columns.
