# Plan: Explicit Connection Selection (no auto-switching)

> Design record: implementation status varies by section. See [current status](../../../docs/PROJECT_STATUS.md) and the [active roadmap](../../ROADMAP.md) before choosing new work.

## Context

**Problem.** When generating an artifact, the app silently decides which LLM
provider/connection to use. The user wants to **manually choose** which of the
already-configured connections is used, and have the system honor that choice
only — no automatic detection/switching.

**Root cause.** `is_active` is stored **per provider row** in
`user_provider_configs`, not as one global selection. Saving/activating one
provider (e.g. OpenAI) does **not** deactivate the others, so multiple rows can
hold `is_active = 1` at once. The frontend then resolves the "active" provider
with `pickActiveProvider()`, which does `list.find(isActive) ?? list[0]` —
i.e. it grabs the **first matching (or first overall) row** in
`ORDER BY provider ASC`. That first-row grab is the "automatic switching /
undetermined connection" behavior. There is no single source of truth for
"the connection to use."

**Intended outcome.**
1. Exactly **one** connection is active at any time (singleton).
2. The user's manual pick in the status-bar switcher is authoritative.
3. No silent first-row fallback. If no connection is explicitly selected,
   generation is blocked with a clear prompt to pick one.

**Decisions locked with user.**
- "Connection" = the already-saved provider configs (one per provider kind:
  ollama, ollama-cloud, openai, openrouter, anthropic, gemini). **No** new
  named-connection data model, **no** DB migration.
- Selection surface = **reuse the existing status-bar `ProviderSwitcher`**.
- When nothing is selected → **force explicit choice** (block generation).

## Approach

Make the active connection a **singleton via mutual exclusion** at save time,
then remove the frontend's silent first-row fallback. No schema migration, no
IPC contract change.

---

### Step 1 — Backend: enforce a single active connection

**File:** `apps/desktop/src-tauri/src/repositories/provider_config_repo.rs`
(`upsert`, ~line 45)

When `row.is_active == true`, clear the flag on every other row for the user
before writing the target row, inside one transaction:

- Open a transaction (`pool.begin()`), or pass a `&mut Tx` through the existing
  queries.
- If `is_active`: `UPDATE user_provider_configs SET is_active = 0, updated_at = ?
  WHERE user_id = ?` (clears all), then run the existing insert/update binding
  the target as `is_active = 1`.
- If `!is_active`: keep current behavior (just upsert the row as inactive).
- Commit.

This guarantees ≤ 1 active row at all times. The manual pick wins; configuring a
new provider as active transparently deactivates the previous one — no automatic
switching among several "active" rows.

Reuse existing pieces — `ProviderConfigUpsert` struct, `DEFAULT_USER_ID`,
`fetch_for_user_provider`. `fetch_active` (line 156) and
`commands/generation.rs:93` need **no change** — they already query
`WHERE provider = ? AND is_active = 1`, which is now unambiguous.

**Test (same file, `#[cfg(test)]`):** save provider A active → save provider B
active → assert `list_for_user` shows A inactive, B active (exactly one active).
Follow the existing in-file async test pattern (`tmp_db()` / `init_pool_at`).

### Step 2 — Frontend: drop silent fallback, force explicit choice

**File:** `apps/desktop/src/lib/provider.ts` (line 14)
- Change `pickActiveProvider` to `return list.find((c) => c.isActive) ?? null;`
  (remove the `?? list[0]` first-row fallback). Update the JSDoc to state it no
  longer defaults to the first entry.

**File:** `apps/desktop/src/components/ai-panel/ai-panel.tsx`
- `canGenerate` (line 175) already requires `activeProvider !== null` — keep.
- When `activeProvider === null`, render a clear inline message
  ("Select a connection to generate") with an action that opens the connection
  switcher / settings (reuse `useUiStore().setSettingsOpen` or focus the
  status-bar switcher). No silent default is chosen.

**File:** `apps/desktop/src/components/layout/status-bar.tsx`
- Remove the stray `console.log("DEBUG: StatusBar rendering, project is:", project)`
  at line 22 (violates the no-`console.log` rule).

### Step 3 — Status-bar switcher = the single selection surface

**File:** `apps/desktop/src/components/layout/status-bar.tsx`
(`ProviderSwitcher`, ~line 87) — mostly already correct.
- `handleSwitch` (line 119) already calls
  `saveProviderConfig({ provider, isActive: true })`, then re-lists and sets the
  active provider. With Step 1, this now deactivates all other rows, so the
  check-mark reflects the one true active connection.
- Polish (optional): when none is active, show "Select connection" instead of
  "none"; label the control "Connection".

---

## What is NOT changing
- No DB migration. `user_provider_configs` schema is unchanged.
- No IPC/Zod contract change — `saveProviderConfig`, `listProviderConfigs`,
  `generateArtifact` signatures stay the same.
- No new "named connections per provider" concept.
- `generation_service` / `factory` / `commands/generation.rs` untouched.

## Files touched (summary)
- `apps/desktop/src-tauri/src/repositories/provider_config_repo.rs` — singleton
  upsert + test.
- `apps/desktop/src/lib/provider.ts` — drop first-row fallback.
- `apps/desktop/src/components/ai-panel/ai-panel.tsx` — block + prompt when none.
- `apps/desktop/src/components/layout/status-bar.tsx` — remove debug log, polish
  labels.

## Verification

**Rust unit test (new):**
```
cargo test --lib --manifest-path apps/desktop/src-tauri/Cargo.toml upsert
```
Assert only one row active after activating two providers in sequence.

**Frontend typecheck + tests:**
```
pnpm typecheck
pnpm --filter @testing-ide/desktop run test:frontend
```

**Manual end-to-end (run the app):**
```
pnpm --filter @testing-ide/desktop run dev
```
1. Configure ≥ 2 providers (e.g. ollama + openai).
2. Open the status-bar switcher → pick OpenAI → confirm check-mark moves, Ollama
   no longer active (re-open popover to verify single active).
3. Confirm `Generate` uses the picked connection (no silent switch).
4. Deactivate / start with no active connection → confirm `Generate` is blocked
   and the "Select a connection" prompt appears.
5. Restart app → confirm the last explicitly-picked connection is restored
   (persisted via DB `is_active`), not a first-row guess.

## Risks / notes
- Existing databases may already have multiple `is_active = 1` rows. The first
  activate-save after this change collapses them to one; until then,
  `pickActiveProvider` returning the first truthy match is acceptable, and the
  next explicit pick normalizes state. Optional one-time normalization can be
  skipped given low blast radius (few configs per user).
- Keep typed `AppError` inside the repo; map at the command boundary as today.
