# Local staged-review context API (#108)

This is an internal Rust seam, **not an end-to-end review runtime**. There are no
new commands, UI, provider calls, schemas, or persisted DTOs. #107 owns persisted
review types; #109 can adapt these local types to its prompt and validation.

## Capture and assemble

1. `git_change_service::capture(root)` pins HEAD, full index fingerprint, hunks,
   and exact old/new source bytes. Never replace these with working-tree source.
2. `capture_support(root, &snapshot, semantic_paths)` rechecks that identity before
   and after reading related **index blobs**. A changed HEAD/index is
   `INVALID_INPUT` (`capture_changed`); request a new review, never silently refresh.
   Later edits cannot mutate either owned capture. Support is bound to snapshot ID
   and index fingerprint; `assemble` rejects a different binding. `ReviewContext`
   retains snapshot ID, Git object format, optional base commit, and index fingerprint.
3. `review_context_service::assemble` receives the snapshot, supporting capture,
   optional `RetrievalHint`s, `ContextBudget`, a **trusted secret-redaction callback**,
   and a conservative token counter. The callback receives whole UTF-8 files,
   including multiline secrets, before excerpt selection. It must remove secrets
   or return `None`. There is deliberately no identity/default redactor in the API.
   This seam validates mapping, not secret detection. Callers must not send raw
   capture bytes or raw diff patches to a model.

Redaction preserves each raw logical line and its LF/CRLF terminator; transformations
that change that layout exclude the whole changed file (both sides) with
`redaction_unmappable`. Hashes always describe **raw bytes**, not redacted text.
`model_text()` labels JSON-escaped paths, old/new side, blob ID, raw SHA-256 and
original inclusive line range. Required code consists of exact hunk ranges and
intersecting AST declarations, merged without duplication. Mode-only changes and
exact renames retain manifest metadata; their unchanged new-side source is optional
and may be omitted for prompt budget. Deleted code stays old-side only; unavailable
or partial ASTs retain exact hunks and expose a limitation.

## Limited-context retrieval

No embeddings, paid service, or full reindex is needed. Selection ranks direct
relative imports (old and new source), filename-related tests, then optional
semantic paths; ties are lexical. This intentionally does not resolve aliases,
package imports, all reverse dependents, or transitive graphs. Missing related
tests and the no-embedding path are explicit `limitations`, not completeness claims.

Whole-change capture exclusions are final for that snapshot: a dependency excluded
because either its base or staged side is unsafe, ignored, binary, or oversized
cannot return as optional index evidence. Assembly also rejects reintroduction
through caller-supplied support or contradictory changed entries.

Cached retrieval is a path/hash hint, never prompt text. Exact index blobs are
re-read. A hash mismatch is recorded as `stale_retrieval_replaced` and a missing blob as
`stale_retrieval_unavailable`;
only successfully captured exact evidence can be cited. Semantic callers should
pass the same hinted paths to `capture_support` and hints to `assemble`.

## Limits and citations for #109

- Capture reuses path, sensitive-file, language, binary and pinned-ignore policies.
- At most 20 existing supporting candidates are read, with `context_limit` omissions.
  Missing semantic paths remain visible as `missing_index_path` without using a slot.
- Each source blob is at most 128 KiB. Required policies and distinct old/new/support
  blobs share the 1 MiB capture cap; total overflow fails, never clips a blob.
- Input fits `min(8000, model_window - output_reserve)` including the caller's
  `fixed_prompt_tokens` instructions/schema reserve. Lower provider limits win.
  The counter must cover the entire `model_text()` section including labels;
  `str::len` is a conservative UTF-8 byte fallback. The caller must budget/count
  its exact additional prompt wrappers as part of `fixed_prompt_tokens`.
- Optional whole supporting files drop from lowest rank first, with `prompt_budget`
  omissions and counted `truncation` entries. Required code overflow is
  `LIMIT_EXCEEDED`; stage a smaller change. Changed code is never silently shrunk.
- Validate references with `permits(path, side, raw_hash, start, end)`, which accepts
  only a positive ordered inclusive range wholly inside a supplied excerpt.
  Additionally validate snapshot/manifest identity in persisted DTOs. File-level
  references without lines need separate manifest validation; this API does not
  authorize arbitrary file-level citations. Never accept transformed coordinates.
- Treat all source/path strings as untrusted data in #109's instructions. Source
  delimiters are labels, not a defense against prompt injection. Validate model
  output independently; no source instructions may grant tools or change scope.

Offline unit and real-Git tests live in `services/review_context_service/tests.rs`.
