# Plan — JIRA Integration (one-way artifact push, Jira Cloud)

> Status: v1 shipped (Phases 1–2, #62) — Phase 3 (v2: bulk push, run comments, status refresh) remaining | Owner: TBD | Created: 2026-06-07
> Bridges Tessera's generated artifacts (Defect Report, Bug Report, Test Plan, Test Cases) into the team's real QA workflow.
> Replaces the former "Tessera Boards" plan that lived at this path (recoverable at commit `f7c9fc3`).

## 1. Goal

Tessera generates Defect Reports, Bug Reports, Test Plans and Test Cases as local
artifacts — today they dead-end inside the app while QA teams' actual workflow
lives in JIRA. Close that gap with a minimal, **write-mostly** integration:

**Tessera finds the bug → one click → real JIRA issue → team workflow takes over.**

Tessera *writes* to JIRA. It never tries to *mirror* it.

## 2. Non-goals (permanent, by design)

- **No two-way sync** — JIRA-side changes do not flow back automatically. Would
  need polling/webhooks/conflict handling: massive cost, tiny value for a
  desktop app.
- **No webhooks** — desktop app has no server to receive them.
- **No JQL browsing / issue lists in-app** — JIRA's UI exists; we link out.
- **No attachment upload** — privacy + size; only the text artifact is pushed.
- **No OAuth** — API token auth only; OAuth dance is bloat for a desktop app.

The only read operation ever added is the v2 on-demand status refresh (single
GET, user-triggered).

## 3. Core guarantee — must hold

Tessera is local-first: nothing leaves the machine without an explicit user
action.

- Push requires a **preview-before-push** confirmation — the user sees the exact
  issue payload (summary, description, priority, labels) before anything is sent.
- Credentials (API token) are encrypted at rest with the existing AES-256-GCM
  machinery (`utils/crypto.rs::CryptoKey`); list views return only
  `hasApiToken: bool`, never plaintext.
- No background network activity — every JIRA call is user-triggered.

## 4. Key design decisions

| Decision | Choice | Why |
|---|---|---|
| JIRA API version | REST **v2**, raw markdown string as `description` | v3 requires ADF JSON; an md→ADF converter is the single biggest bloat/bug risk for zero v1 value. v2 accepts plain strings on Jira Cloud, no announced sunset. Optional ~80-line `md_to_jira_wiki` helper deferred to v2 polish. |
| Auth | Jira Cloud API token (email + token, Basic auth) | Mirrors LLM key storage; simplest thing that works. |
| Config storage | New `tracker_configs` table — **not** `user_provider_configs` | JIRA needs `email`, `project_key`, `issue_type`, `severity_map_json`; none fit the LLM table, and a `jira` row would leak into the LLM provider list UI. Same encrypt pattern: `api_token_encrypted`/`api_token_nonce` BLOBs, `UNIQUE(user_id, tracker)`, masked views. |
| Abstraction | `IssueTracker` trait in `providers/trackers/` (sibling of `providers/llm/`), `JiraTracker` first impl, factory dispatch | Mirrors the `LlmProvider` pattern. GitHub Issues / Linear later = one new file, no rewiring. |
| Mapping | Deterministic pure function — **no LLM** in the path | Same input → same output. Testable, free, instant. |
| Idempotency | Local `external_links` lookup first, plus `tessera-<artifact-id>` label on the JIRA side | Push twice → "already linked", never a duplicate issue. |
| Severity → priority | Hardcoded default map, v2 makes it overridable via `severity_map_json` | Covers `DefectSeveritySchema` (4-level) and `BugSeveritySchema` (5-level) with one map. |

Default severity map: `blocker→Highest, critical→Highest, major→High,
minor→Medium, trivial→Low`. Priority is sent by name
(`{"priority":{"name":"High"}}`); on a 400, retry once *without* priority —
some JIRA projects disable the field.

## 5. Architecture (fits existing layering)

Mirror the `LlmProvider` pattern: trait with pluggable impls, one service as the
sole entry point.

```
commands/trackers.rs                  Tauri IPC handlers — thin, validate, delegate
services/tracker_config_service.rs    Config CRUD — encrypt/decrypt just-in-time
services/jira_push_service.rs         Sole push entry point — map, dedupe, create, link
providers/trackers/mod.rs             IssueTracker async trait + shared types
providers/trackers/jira.rs            Jira Cloud impl (reqwest, Basic auth)
providers/trackers/error.rs           TrackerError → AppError bridge
providers/trackers/factory.rs         build_tracker(config) -> Arc<dyn IssueTracker>
repositories/tracker_config_repo.rs   SQL only — config rows
repositories/external_link_repo.rs    SQL only — artifact ↔ issue links
db migration 0005_jira_integration.sql
```

Frontend:

```
packages/shared/src/schemas/tracker.schema.ts    Zod contract (FE/BE)
apps/desktop/src/lib/ipc/trackers.ts             Typed IPC wrapper (Zod-validated)
apps/desktop/src/components/jira-config-panel.tsx        Settings (mirror provider-config-panel)
apps/desktop/src/components/ai-panel/jira-push-dialog.tsx Preview-before-push modal
```

**Push flow:** `jira_push_service::push(artifact_id)` → load artifact →
deterministic mapping → `external_link_repo` idempotency check →
`IssueTracker::create_issue` → persist link → return issue key/URL → FE badge.

### `IssueTracker` trait (shape, not final code)

```rust
pub struct NewIssue {
    pub project_key: String,
    pub issue_type: String,
    pub summary: String,               // truncate to 255 (Jira hard limit)
    pub description: String,           // raw markdown, v2 string field
    pub priority: Option<String>,      // Jira priority NAME
    pub labels: Vec<String>,           // includes "tessera-<artifact-id>"
    pub parent_key: Option<String>,    // v2 epic parent
}

#[async_trait]
pub trait IssueTracker: Send + Sync {
    fn name(&self) -> &'static str;
    async fn test_connection(&self) -> Result<TrackerUser, TrackerError>;      // GET /myself
    async fn create_issue(&self, issue: NewIssue) -> Result<CreatedIssue, TrackerError>;
    async fn bulk_create(&self, issues: Vec<NewIssue>) -> Result<BulkCreateResult, TrackerError>; // v2
    async fn add_comment(&self, issue_key: &str, body: &str) -> Result<(), TrackerError>;          // v2
    async fn get_issue_status(&self, issue_key: &str) -> Result<IssueStatus, TrackerError>;        // v2
}
```

`TrackerError` variants: `AuthFailed` / `RateLimited` / `NotFound` /
`InvalidRequest` / `Transport`, bridged into `AppError` via `#[from]`, reusing
the `map_http_error` status-mapping pattern from
`providers/llm/openai_compat.rs` (401/403 → AuthFailed, 429 → RateLimited,
5xx → Transport).

## 6. Data model — migration `0005_jira_integration.sql`

```sql
CREATE TABLE tracker_configs (
    id                  TEXT PRIMARY KEY NOT NULL,
    user_id             TEXT NOT NULL,
    tracker             TEXT NOT NULL,              -- 'jira'
    site_url            TEXT NOT NULL,              -- https://acme.atlassian.net
    email               TEXT NOT NULL,
    api_token_encrypted BLOB,
    api_token_nonce     BLOB,
    project_key         TEXT NOT NULL,
    issue_type          TEXT NOT NULL DEFAULT 'Task',
    severity_map_json   TEXT,                       -- v2 override; NULL = built-in default
    is_active           INTEGER NOT NULL DEFAULT 1,
    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL,
    UNIQUE (user_id, tracker)
);

CREATE TABLE external_links (
    id                TEXT PRIMARY KEY NOT NULL,
    artifact_id       TEXT NOT NULL,
    tracker           TEXT NOT NULL,                -- 'jira'
    item_ref          TEXT NOT NULL DEFAULT '',     -- '' = whole artifact; test-case id for v2 children
    issue_key         TEXT NOT NULL,                -- 'PROJ-123'
    issue_url         TEXT NOT NULL,
    issue_type        TEXT,                         -- 'Task' | 'Epic' | ...
    last_status       TEXT,                         -- v2 on-demand refresh cache
    status_fetched_at TEXT,
    created_at        TEXT NOT NULL,
    updated_at        TEXT NOT NULL,
    FOREIGN KEY (artifact_id) REFERENCES artifacts(id) ON DELETE CASCADE,
    UNIQUE (artifact_id, tracker, item_ref)
);

CREATE INDEX idx_tracker_configs_user_tracker ON tracker_configs(user_id, tracker);
CREATE INDEX idx_external_links_artifact_id ON external_links(artifact_id);
CREATE INDEX idx_external_links_tracker_key ON external_links(tracker, issue_key);
```

Notes:

- `item_ref` discriminator: `''` = whole artifact; a test-case id for v2 epic
  children. The UNIQUE constraint is `(artifact_id, tracker, item_ref)` — not
  just artifact+tracker — so a test-plan Epic plus N child Tasks can hang off
  one artifact. `''` default (not NULL) because SQLite treats NULLs as distinct
  in UNIQUE constraints.
- Follows 0001 conventions: TEXT UUID PKs, RFC-3339 timestamps, FK indexes.

## 7. Feature set

### v1 — write-only, minimal

1. **Settings** — `JiraConfigPanel`: site URL, email, API token (encrypted),
   default project key + issue type, test-connection button (`GET /myself`,
   shows display name on success).
2. **"Push to Jira" button** in the artifact detail drawer footer
   (defect-report / bug-report only), opening a **preview-before-push** dialog:
   rendered summary, description, priority, labels → confirm → push → issue
   key + link shown.
3. **Deterministic mapping** — `title` → summary (truncated to 255),
   `content_md` → description, max severity across `findings[].severity` →
   priority, labels `["tessera", "tessera-<artifact-id>"]`.
4. **Link persistence** — `external_links` row on success; badge `PROJ-123 ↗`
   on the artifact card, click opens browser; duplicate-push guard shows
   "already linked" instead of creating a second issue.

### v2

1. **Test Plan → Epic hierarchy** — Test Plan artifact becomes an Epic, each
   test case a child Task via `POST /rest/api/2/issue/bulk` (one call, no N+1),
   per-case `item_ref` links.
2. **Sandbox results → comments** — post run outcome on the linked issue:
   "Automated run 2026-06-07: PASS — 12/14 passed, coverage 84%". Failing run
   on a defect issue = reproduction evidence on the ticket.
3. **On-demand status refresh** — button on the link badge, single GET, caches
   `last_status` + `status_fetched_at`. Pull-only; explicitly NOT sync.
4. **Bulk push** — multi-select artifacts, push all, dedupe summary:
   "3 created, 1 skipped (already linked)". Partial failures surfaced per-item.
5. **Severity map editor** — `severity_map_json` as a Zod-validated JSON
   textarea in `JiraConfigPanel`; optional `md_to_jira_wiki` helper (pure
   function: `## H` → `h2.`, fenced code → `{code}`, `**b**` → `*b*`).

## 8. New / modified files

New — backend (`apps/desktop/src-tauri/`):

- `migrations/0005_jira_integration.sql`
- `src/providers/trackers/{mod,error,jira,factory}.rs`
- `src/repositories/{tracker_config_repo,external_link_repo}.rs`
- `src/services/{tracker_config_service,jira_push_service}.rs` — mirror
  `provider_config_service.rs` (encrypt/decrypt just-in-time, masked views) and
  `generation_service.rs` (Deps struct: pool + `Arc<dyn IssueTracker>`)
- `src/commands/trackers.rs` — thin commands: `save_tracker_config`,
  `get_tracker_config`, `delete_tracker_config`, `test_tracker_connection`,
  `preview_jira_push`, `push_artifact_to_jira`, `list_external_links`;
  v2 adds `push_test_plan_to_jira`, `bulk_push_artifacts`, `post_run_comment`,
  `refresh_issue_status`

New — frontend (`apps/desktop/src/`):

- `lib/ipc/trackers.ts` (via `invokeAndParse`)
- `components/jira-config-panel.tsx` (mirror `provider-config-panel.tsx`)
- `components/ai-panel/jira-push-dialog.tsx`
- tracker slice in the store

New — shared (`packages/shared/src/schemas/`):

- `tracker.schema.ts` — `TrackerConfigViewSchema` (masked `hasApiToken`,
  never the token), `ExternalLinkSchema`, `PushPreviewSchema`. Rust serde is
  the source of truth; Zod mirrors (rules.md §12.3.1).

Modified:

- `src-tauri/src/lib.rs` — invoke_handler registration (CryptoKey already managed)
- `src-tauri/src/{providers,repositories,services,commands}/mod.rs`,
  `src-tauri/src/error.rs` — module wiring + `#[from] TrackerError` arm
- `src/components/ai-panel/artifact-detail-drawer.tsx` — footer button after
  "Export markdown", gated to defect/bug types + active config
- `src/components/ai-panel/ai-panel.tsx` — link badge in `ArtifactRow` after
  the status badge
- settings page hosting the provider panel — add Jira section
- `packages/shared/src/index.ts` + `contract-schemas.test.ts` — exports +
  round-trip tests

## 9. Phases (3 large)

### Phase 1 — Backend v1

Migration 0005; `TrackerError`; `IssueTracker` trait + `JiraTracker`; factory;
both repos; both services (push pipeline: load artifact → map → idempotency
check → create → persist link); commands + lib.rs registration; error bridge.

Tests: `ScriptedTracker` mock implementing `IssueTracker` with queued responses
(mirror the `ScriptedLlm` pattern in `generation_service.rs` tests) — mapping
determinism, severity→priority for all enum values, summary truncation,
idempotent skip. Repo tests on temp SQLite (pattern from
`provider_config_repo.rs` tests) — upsert, unique constraint, cascade delete.
Crypto round-trip on the token. HTTP status → `TrackerError` mapping tests
(no network).

**Exit:** `cargo test` + clippy pedantic green; commands callable.

### Phase 2 — Frontend v1

`tracker.schema.ts` + index exports + contract round-trip tests;
`lib/ipc/trackers.ts`; `JiraConfigPanel` (form, save, test-connection, masked
token); `jira-push-dialog.tsx` (preview → confirm → push → issue link,
"already linked" state); drawer button; ArtifactRow badge (opens browser via
Tauri opener); tracker store slice.

**Exit:** full manual flow works against a real Jira Cloud sandbox; FE tests pass.

### Phase 3 — v2

Epic/child bulk push (`parent` field, per-case `item_ref` links); sandbox-run
comments (formats counts from `test_runs`); on-demand status refresh + badge
tooltip; multi-select bulk push with dedupe summary; severity-map editor;
optional `md_to_jira_wiki`.

**Exit:** all v2 flows green; bulk partial failures surfaced per-item.

## 10. Verification

### Automated

```bash
pnpm guard:pre-push   # typecheck → lint → test → clippy
```

Rust unit tests must cover: mapping determinism, severity→priority for every
enum value (both 4-level defect and 5-level bug scales), 255-char summary
truncation, idempotent skip, bulk partial-failure aggregation, crypto
round-trip, HTTP status → `TrackerError` mapping.

### Manual

1. Fresh-DB launch applies 0005 cleanly; upgrade from an existing 0004 DB also
   verified.
2. Against a free Jira Cloud site: save config → test connection (displays
   `/myself` displayName) → push a defect report → verify summary /
   description / priority / label in JIRA → push again → "already linked" →
   badge shows key and opens browser.
3. v2: plan→epic hierarchy visible in the JIRA backlog; run comment renders
   counts; status refresh reflects a JIRA-side transition; bulk push of 3
   artifacts with 1 pre-linked reports "2 created, 1 skipped".

## 11. Risks

| Risk | Mitigation |
|---|---|
| JIRA project disables the priority field → 400 on create | Retry once without priority; surface warning, not failure |
| REST v2 deprecation on Jira Cloud | Trait isolates the HTTP surface; swap to v3+ADF inside `jira.rs` only |
| Markdown renders poorly in JIRA description | Acceptable v1 (mostly readable); `md_to_jira_wiki` helper in v2 polish |
| Duplicate issues from concurrent pushes | DB UNIQUE constraint + remote `tessera-<artifact-id>` label |
| Scope creep toward sync | Non-goals fenced in §2; only read ever added is on-demand status GET |
