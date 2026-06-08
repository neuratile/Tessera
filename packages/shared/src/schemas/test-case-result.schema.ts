import { z } from 'zod';

/**
 * Per-case execution outcome sidecar for the 9-column Test Case table
 * (plan/TEST_CASE_TABLE.md §4). Columns 8–9 (Actual output / Result and
 * remarks) are mutable, user- and runtime-owned data kept out of the LLM
 * artifact's `structured_data` so a regeneration cannot wipe them.
 *
 * Rust serde is the source of truth (rules §12.3.1); these schemas mirror
 * the structs + enums in
 * `apps/desktop/src-tauri/src/repositories/test_case_result_repo.rs`.
 * Enum literals must match the `snake_case` serde output and the TEXT
 * stored in `test_case_results` exactly. Nullable string columns
 * (`actual_output`, `remarks`, `run_id`) serialize as JSON `null`, so the
 * mirror is `.nullable()`, never `.optional()`.
 */

/**
 * Manual-test result of one case — mirrors the Rust
 * `TestCaseResultStatus` enum and the `result` column. `not_run` is the
 * default for a case with no recorded outcome yet.
 */
export const TestCaseResultResultSchema = z.union([
  z.literal('pass'),
  z.literal('fail'),
  z.literal('blocked'),
  z.literal('not_run'),
]);

export type TestCaseResultResult = z.infer<typeof TestCaseResultResultSchema>;

/**
 * Who produced the current outcome — mirrors the Rust
 * `TestCaseResultSource` enum and the `source` column. `sandbox` rows are
 * auto-filled by a Docker run; `manual` rows are typed by a tester.
 */
export const TestCaseResultSourceSchema = z.union([
  z.literal('manual'),
  z.literal('sandbox'),
]);

export type TestCaseResultSource = z.infer<typeof TestCaseResultSourceSchema>;

/**
 * One stored outcome row, returned by `list_test_case_results`. Mirrors
 * the Rust `TestCaseResultRow` serde struct (`camelCase`).
 */
export const TestCaseResultSchema = z.object({
  id: z.string().uuid(),
  artifactId: z.string().uuid(),
  caseId: z.string().min(1),
  actualOutput: z.string().nullable(),
  result: TestCaseResultResultSchema,
  remarks: z.string().nullable(),
  source: TestCaseResultSourceSchema,
  runId: z.string().uuid().nullable(),
  createdAt: z.string(),
  updatedAt: z.string(),
});

export type TestCaseResult = z.infer<typeof TestCaseResultSchema>;

/**
 * Manual upsert payload for `upsert_test_case_result`. Mirrors the Rust
 * `UpsertTestCaseResultInput` command struct. `source` is always `manual`
 * on this path (the backend sets it), so it is not part of the wire shape.
 * Optional string fields may be omitted — the Rust `Option<String>`
 * fields default to `None` (rules §12.3.1: the mirror is not stricter
 * than the serde source).
 */
export const UpsertTestCaseResultInputSchema = z.object({
  artifactId: z.string().uuid(),
  caseId: z.string().min(1),
  actualOutput: z.string().optional(),
  result: TestCaseResultResultSchema,
  remarks: z.string().optional(),
});

export type UpsertTestCaseResultInput = z.infer<typeof UpsertTestCaseResultInputSchema>;
