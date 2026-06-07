# Embedding Provider Selection — Ollama / OpenAI / Gemini / Hugging Face

> Status: **implemented** · Owner: core · Created: 2026-06-07 · Branch: `feat/embedding-provider-select`
> Two phases. Phase 1 = backend (config + providers + resolver). Phase 2 = frontend (settings UI + re-index UX).
> Decisions locked: HF **cloud Inference API only** (no local HF runtime), **Gemini included**, re-index is **manual** (button + stale banner, no auto-trigger).
>
> **Implementation deviations from this plan:**
> - §2/§5.6: the "latent cross-model bug" does not exist — `analysis_service.rs:212` already writes the composite `"{name}-{model}"` string into `embedding_provider`, so model identity is baked into the search scope. The composite convention is kept; the `search_similar` signature change and index recreation in migration 0005 were **dropped** (migration ships the new table only).
> - §5.7: the generation-stream stale hint was skipped entirely (not even tracing) — the search-scope composite makes stale retrieval return 0 hits safely, and the banner is the user-facing signal.
> - §7.1: the panel uses component-local state inside the existing `settings-sheet.tsx` pattern; the only Zustand addition is `embedding-store.ts` for the shared stale-index status.
> - §10: FE coverage is the `packages/shared` contract tests + e2e mock wiring; no dedicated Vitest component test (settings-sheet itself has none — followed the file's convention).

## 1. Why

Embeddings are currently hardcoded to local Ollama (`nomic-embed-text`). Users with cloud LLM keys (OpenAI/Gemini) or no local GPU should be able to pick a cloud embedding model; users with HF tokens should be able to use HF Inference API models. Embedding choice must be **independent of LLM choice** — someone can chat via Anthropic while embedding via Ollama.

## 2. Current state (verified facts, file:line)

| Fact | Location |
|---|---|
| `EmbeddingProvider` trait: `name()`, `dimension()`, `model_id()`, `embed(Vec<String>) -> Result<Vec<Vec<f32>>, LlmError>` | `providers/embeddings/mod.rs:24-48` |
| Sole impl: `OllamaEmbeddingProvider` — OpenAI wire format (`{base}/v1/embeddings`), optional Bearer key, dimension validated per response item | `providers/embeddings/ollama.rs` |
| `factory::build_embedding_provider` exists but is **never called in production** — only in its own tests | `providers/factory.rs:191-223` |
| **Both production call-sites hardcode local Ollama**, ignoring factory and user config | `commands/analysis.rs:23-25`, `commands/generation.rs:104-106` |
| Chunks store `embedding BLOB`, `embedding_dim`, `embedding_provider`, `embedding_model` per row (all-or-none constraint) | `migrations/0001_init.sql:114-165` |
| `search_similar` scopes by `(project_id, embedding_provider, embedding_dim)` — **`embedding_model` missing from scope** (latent bug: two 768-dim models from one provider would cross-match) | `repositories/chunk_repo.rs:171-269` |
| Embedding input truncated to `EMBEDDING_INPUT_CHAR_CAP` = 2000 bytes, batches of 32 | `services/analysis_service.rs:30-43` |
| Provider credentials: `user_provider_configs` (AES-256-GCM key + nonce), decrypt via `provider_config_service::build_provider_config` | `repositories/provider_config_repo.rs`, `services/provider_config_service.rs:113-140` |
| Single-user app: `DEFAULT_USER_ID = "00000000-0000-4000-8000-000000000001"` | `commands/generation.rs:30` |
| FE provider settings: `provider-config-panel.tsx`; Zod contracts in `packages/shared/src/schemas/` | investigated, mirrors below |

Schema is already future-proof: per-chunk provider/model/dim metadata means switching providers cannot corrupt anything — old vectors simply stop matching the search scope.

## 3. Goal + scope

In scope:

- New **embedding-specific config** (provider + model + dimension + base URL + optional key), separate from LLM provider config.
- Provider impls: **Ollama local** (exists), **Ollama Cloud** (exists), **OpenAI**, **Gemini**, **Hugging Face Inference API**.
- Settings UI section "Embeddings": provider dropdown, model presets + custom model, test-connection (auto-detects dimension), save.
- Manual re-index: stale-index detection + banner + "Re-index project" button.
- Fix `search_similar` scope to include `embedding_model`.
- Kill the hardcoded-Ollama call-sites; single resolver used everywhere.

Out of scope:

- Local HF models (ONNX/candle runtime) — cloud only. Self-hosted TEI still works via custom base URL on the HF provider.
- Voyage/Cohere/Mistral embeddings — same trait shape, add later on demand.
- Auto re-index on config change — manual by decision.
- sqlite-vec migration (ADR-0001 defers to Phase 3 of that ADR; unrelated).
- Per-project embedding config — one global (per-user) selection.

## 4. API research (wire formats)

| Provider | Endpoint | Request | Response | Auth |
|---|---|---|---|---|
| Ollama local/cloud | `{base}/v1/embeddings` | `{model, input: [..]}` | `{data: [{embedding: [f32]}]}` | none / Bearer |
| OpenAI | `https://api.openai.com/v1/embeddings` | `{model, input: [..], dimensions?}` | same as above | Bearer `sk-…` |
| Gemini (OpenAI-compat layer) | `https://generativelanguage.googleapis.com/v1beta/openai/embeddings` | `{model, input: [..]}` | same as above | Bearer (AIza… key) |
| Hugging Face Inference | `https://router.huggingface.co/hf-inference/models/{model}/pipeline/feature-extraction` | `{"inputs": [..], "normalize": true, "truncate": true}` | `[[f32], …]` (raw nested array) | Bearer `hf_…` |

Notes:

- Ollama, OpenAI, Gemini all speak the **same OpenAI wire format** → one shared `openai_compat` embedding impl covers three of four.
- HF is the odd one: different URL shape (model in path), different body (`inputs`), different response (bare nested array). Always send `inputs` as an **array** (a bare string input makes some models return `[f32]` instead of `[[f32]]` — array input avoids the ambiguity). `truncate: true` guards against context overflow (our 2000-byte cap usually suffices, belt-and-braces).
- HF cold model → HTTP 503 "model is currently loading" with `estimated_time` — map to `LlmError::ProviderUnavailable` with a "model warming up, retry in ~Ns" message.
- OpenAI `dimensions` param only valid on `text-embedding-3-*` — send it only when user picked non-native dimension; omit otherwise.
- Gemini OpenAI-compat embeddings endpoint accepts `gemini-embedding-001` and `text-embedding-004`. Default dims: `gemini-embedding-001` = 3072, `text-embedding-004` = 768.

### Model presets (curated, per provider — custom escape hatch always available)

| Provider | Model | Dim |
|---|---|---|
| ollama | `nomic-embed-text` (default) | 768 |
| ollama | `mxbai-embed-large` | 1024 |
| ollama | `snowflake-arctic-embed` | 1024 |
| ollama | `all-minilm` | 384 |
| ollama | `bge-m3` | 1024 |
| openai | `text-embedding-3-small` (default) | 1536 |
| openai | `text-embedding-3-large` | 3072 |
| gemini | `gemini-embedding-001` (default) | 3072 |
| gemini | `text-embedding-004` | 768 |
| huggingface | `BAAI/bge-m3` (default) | 1024 |
| huggingface | `sentence-transformers/all-MiniLM-L6-v2` | 384 |
| huggingface | `intfloat/multilingual-e5-large` | 1024 |
| huggingface | `BAAI/bge-large-en-v1.5` | 1024 |

Presets live in **one Rust const table** (`providers/embeddings/presets.rs`) exposed via a command, so FE never hardcodes them — single source of truth, FE renders what backend reports.

## 5. Architecture

### 5.1 Data model — migration `0005_embedding_config.sql`

New table (mirrors `user_provider_configs` shape; embedding-specific columns added):

```sql
CREATE TABLE user_embedding_configs (
    id                TEXT PRIMARY KEY,
    user_id           TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    provider          TEXT NOT NULL,           -- 'ollama' | 'ollama-cloud' | 'openai' | 'gemini' | 'huggingface'
    model             TEXT NOT NULL,
    dimension         INTEGER NOT NULL,
    base_url          TEXT,                    -- NULL = provider default
    api_key_encrypted BLOB,                    -- AES-256-GCM, same scheme as user_provider_configs
    api_key_nonce     BLOB,
    is_active         INTEGER NOT NULL DEFAULT 1,
    created_at        TEXT NOT NULL,
    updated_at        TEXT NOT NULL,
    UNIQUE (user_id, provider)
);
```

Also in `0005`: recreate chunk search index to include model:

```sql
DROP INDEX IF EXISTS idx_code_chunks_search_scope;
CREATE INDEX idx_code_chunks_search_scope
    ON code_chunks (project_id, embedding_provider, embedding_model, embedding_dim);
```

One row per (user, provider) so switching back to a previously-configured provider keeps its key/model. "Active" selection = a single `is_active = 1` row per user (save command flips the others to 0 in one transaction — same pattern as `user_provider_configs`).

**Key resolution order** (in service, documented in code):
1. `user_embedding_configs.api_key_encrypted` for the active row, if set.
2. Fallback: `user_provider_configs` row for the same provider string (so an existing OpenAI/Gemini LLM key is reused without re-entry; `huggingface` never matches — HF key always lives in step 1).
3. Neither + provider requires key → `LlmError::AuthFailed` (existing error shape).

**No active row at all** → default to local Ollama `nomic-embed-text`/768 with `config.ollama_base_url` — exact current behavior, zero-migration back-compat.

### 5.2 Rust enum — `EmbeddingProviderKind` (new, in `factory.rs` or `embeddings/mod.rs`)

Deliberately **separate** from `ProviderKind`: LLM list (anthropic, openrouter) ≠ embedding list (huggingface). Fusing them forces `Unsupported` arms both ways forever.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EmbeddingProviderKind {
    #[serde(rename = "ollama")]       Ollama,
    #[serde(rename = "ollama-cloud")] OllamaCloud,
    #[serde(rename = "openai")]       OpenAi,
    #[serde(rename = "gemini")]       Gemini,
    #[serde(rename = "huggingface")]  HuggingFace,
}
```

With `as_str()` / `from_str_value()` / `requires_api_key()` (false only for `Ollama`) — mirror `ProviderKind` exactly, including the serde round-trip test (`factory.rs:252` pattern).

### 5.3 Provider impls — `providers/embeddings/`

```
embeddings/
  mod.rs           trait (unchanged) + re-exports
  ollama.rs        existing — UNTOUCHED (keeps the 404 `ollama pull` hint + 400 context-window specialization)
  openai_compat.rs NEW — generic OpenAI-wire impl: base_url, model, dimension, Option<api_key>,
                   Option<u32> request `dimensions` param, configurable provider name for errors
  huggingface.rs   NEW — feature-extraction wire format
  presets.rs       NEW — const preset table + lookup fn
```

`openai_compat.rs` — copy the proven `ollama.rs` skeleton (request/response structs, status mapping, per-item dimension validation, empty-input short-circuit), parameterize:

- `provider_name: &'static str` (`"openai"` / `"gemini"` — needed because `LlmError` carries `&'static str`).
- `endpoint = {base}/embeddings` when base already ends in `/v1`-style path, else `{base}/v1/embeddings` — simplest rule: store the **full base including version path** as provider default consts (`https://api.openai.com/v1`, `https://generativelanguage.googleapis.com/v1beta/openai`) and always append `/embeddings`.
- `dimensions: Option<u32>` serialized with `skip_serializing_if = "Option::is_none"`.
- Status map: 401/403 → `AuthFailed`, 429 → `RateLimited` (parse `Retry-After` header when present), 5xx → `ProviderUnavailable`, else `InvalidResponse`. Reuse `LlmError::from_reqwest`.

`huggingface.rs`:

- Constructor: `(api_key, model, dimension)` + `with_base_url` for TEI self-host (default `https://router.huggingface.co/hf-inference`).
- URL: `{base}/models/{model}/pipeline/feature-extraction`. Model id goes in the **path** — percent-encode is unnecessary (HF ids are `[A-Za-z0-9._/-]`) but reject whitespace/`?`/`#` in model id at construction (`LlmError::InvalidResponse`-style validation, or `AuthFailed`-adjacent `Unsupported`? → use `LlmError::InvalidResponse { message: "invalid model id" }`).
- Body: `{"inputs": <Vec<String>>, "normalize": true, "truncate": true}`.
- Response: `Vec<Vec<f32>>` directly. Validate `len == inputs.len()` and each inner `len == dimension` (same loop as ollama.rs:203-217).
- 503 + body containing `estimated_time`/`loading` → `ProviderUnavailable` with warm-up message.

Both new impls: `Client::builder().timeout(Duration::from_secs(60))`, no `unwrap`/`expect` outside tests, full mockito test suite mirroring `ollama.rs` tests (success, bearer header asserted, dimension mismatch, 401, 429, 5xx, empty input no-HTTP, HF 503 warm-up).

### 5.4 Factory + resolver

`factory.rs`:

```rust
pub struct EmbeddingConfig {
    pub kind: EmbeddingProviderKind,
    pub model: String,
    pub dimension: usize,
    pub base_url: Option<String>,
    pub api_key: Option<String>,   // decrypted by service, never persisted plaintext
}

pub fn build_embedding_provider(cfg: &EmbeddingConfig) -> Result<Arc<dyn EmbeddingProvider>, LlmError>
```

Match arms: Ollama → `OllamaEmbeddingProvider::with_model` (+ key for cloud); OpenAi/Gemini → `OpenAiCompatEmbeddingProvider` with respective default base + name; HuggingFace → `HuggingFaceEmbeddingProvider`. **Delete the old `build_embedding_provider(&ProviderConfig)` signature** and its tests (replaced, not deprecated — it has zero production callers).

New `services/embedding_config_service.rs` (commands → services → repos layering, no SQL in service — repo does SQL):

- `get_active(pool, crypto, user_id) -> AppResult<EmbeddingConfig>` — fetch active row → decrypt key (resolution order §5.1) → fall back to Ollama default when no row.
- `resolve_provider(pool, crypto, user_id, ollama_base_url) -> AppResult<Arc<dyn EmbeddingProvider>>` — `get_active` + `build_embedding_provider`. **This is the single production entry point.**
- `save(pool, crypto, user_id, args) -> AppResult<EmbeddingConfigView>` — validate kind/model/dimension (`dimension >= 1 && <= 8192`; model non-empty, trimmed), encrypt key if provided, upsert, flip `is_active` exclusively, return view (never the key).
- `test_connection(cfg) -> AppResult<TestResult { latency_ms, detected_dimension }>` — build provider with **dimension = 0 sentinel? No** — build with user-claimed dimension but call a dedicated probe: construct provider, `embed(vec!["tessera dimension probe"])`, measure latency, return `detected_dimension = result[0].len()`. To avoid the impl's own dimension validation rejecting the probe, probe constructs the provider with the dimension **taken from the first response** — concretely: add `embed_unchecked` no; simplest: service catches `InvalidResponse` containing "expected N dimensions, got M", parses M? Fragile. **Decision: probe path builds provider with `dimension = 0` and impls skip per-item validation when `self.dimension == 0`** (document on trait: 0 = unvalidated probe mode; `chunk_repo` never sees dim 0 because save persists the detected value and resolver refuses dim 0 rows). One `if self.dimension != 0` guard per impl, explicit and testable.
- FE flow: Test button → returns detected dim → FE fills dimension field → Save persists it. Save also re-runs a probe when dimension was never tested? No — keep save dumb; UI nudges to Test first, but save accepts manual dim (TEI/custom models).

### 5.5 Call-site replacement (kills hardcoded Ollama)

- `commands/analysis.rs:23-25` → `embedding_config_service::resolve_provider(&pool, &crypto, DEFAULT_USER_ID, &config.ollama_base_url)`. Command gains `crypto: State<'_, CryptoKey>` param.
- `commands/generation.rs:104-106` → same call. (`DEFAULT_USER_ID` already there.)
- Move `DEFAULT_USER_ID` to a shared location (`commands/mod.rs` or `config`) — now used by 3+ commands.

### 5.6 Search scope fix

`chunk_repo::search_similar` — add `embedding_model` to the WHERE clause alongside provider + dim; signature gains `model: &str`. Callers (`generation_service::retrieve_chunks` ~`generation_service.rs:1100-1180`) pass `deps.embeddings.model_id()`. Update `chunk_repo` tests + any `generation_service` tests pinning the scope.

### 5.7 Stale-index detection (manual re-index UX)

New command `get_index_status(project_id)`:

```
SELECT embedding_provider, embedding_model, embedding_dim, COUNT(*)
FROM code_chunks WHERE project_id = ? AND embedding IS NOT NULL
GROUP BY 1, 2, 3
```

Compare against active config → response:

```ts
{ projectId, embeddedChunks, indexedWith: { provider, model, dimension } | null,
  activeConfig: { provider, model, dimension }, isStale: boolean }
```

`isStale = indexedWith != null && indexedWith != activeConfig`. Service-level logic in `embedding_config_service` (or `analysis_service`), repo gets the GROUP BY query.

Re-index = existing `analyze_project` re-run (it rebuilds chunks + embeddings). Verify during Phase 1 that `analysis_service::analyze` clears/overwrites prior chunks for the project — if it skips already-embedded chunks, add a `force` flag that wipes `code_chunks` rows for the project first (repo fn `delete_by_project`).

Silent-degradation guard: in `generation_service::retrieve_chunks`, when search returns 0 chunks **and** the project has chunks embedded under a different (provider, model, dim), emit `tracing::warn!` and surface a structured hint in the generation event stream so FE can toast "Index stale — re-index in Settings". (Minimal version: tracing only, FE relies on banner; pick during implementation, banner is the must-have.)

## 6. IPC + shared contracts

### 6.1 Tauri commands (`commands/embeddings.rs`, registered in `lib.rs` invoke_handler)

| Command | Args | Returns |
|---|---|---|
| `get_embedding_config` | — | `EmbeddingConfigView` (active or ollama default, `hasApiKey` bool, never key) |
| `save_embedding_config` | `SaveEmbeddingConfigArgs` | `EmbeddingConfigView` |
| `test_embedding_connection` | same args shape (test-before-save; key optional — falls back to stored/LLM key) | `{ latencyMs, detectedDimension }` |
| `list_embedding_presets` | — | `EmbeddingPreset[]` (provider, model, dimension, isDefault) |
| `get_index_status` | `projectId` | `IndexStatus` (§5.7) |

All: owned arg types, `Result<T, String>` with `.map_err(|e| e.to_string())` at boundary, `#[allow(clippy::needless_pass_by_value)]` with comment — house style per `commands/providers.rs`.

### 6.2 Zod (`packages/shared/src/schemas/embedding-config.schema.ts`) — Rust serde is source of truth

```ts
EmbeddingProviderIdSchema = z.union([ z.literal('ollama'), z.literal('ollama-cloud'),
  z.literal('openai'), z.literal('gemini'), z.literal('huggingface') ])
EmbeddingConfigViewSchema = z.object({ id, provider, model, dimension: z.number().int().positive(),
  baseUrl: z.string().nullable(), hasApiKey: z.boolean(), isActive: z.boolean() })
SaveEmbeddingConfigArgsSchema = z.object({ provider, model: z.string().min(1),
  dimension: z.number().int().min(1).max(8192), baseUrl: z.string().url().optional(),
  apiKey: z.string().optional() })
TestEmbeddingResultSchema = z.object({ latencyMs: z.number(), detectedDimension: z.number().int().positive() })
EmbeddingPresetSchema = z.object({ provider, model, dimension, isDefault: z.boolean() })
IndexStatusSchema = z.object({ projectId, embeddedChunks: z.number().int(),
  indexedWith: z.object({ provider: z.string(), model: z.string(), dimension: z.number().int() }).nullable(),
  activeConfig: …, isStale: z.boolean() })
```

Note `indexedWith.provider` is plain `z.string()` (not the union) — DB may hold a provider string from a removed variant; status display must not explode on legacy rows.

Round-trip contract test in `packages/shared` per house rule (§12.3.1): serialize Rust-shaped fixtures, parse with Zod, assert equality. camelCase over the wire (serde `rename_all = "camelCase"` on all IPC structs — match existing `GenerateArgs` pattern).

### 6.3 FE IPC wrappers — `src/lib/ipc/embeddings.ts`

One typed wrapper per command, Zod-validate every response, no raw `invoke()` elsewhere. Mirror `lib/ipc/providers.ts` style.

## 7. Frontend (Phase 2)

### 7.1 Settings — new `embedding-config-panel.tsx` (sibling of `provider-config-panel.tsx`)

- Provider dropdown: Ollama (Local) / Ollama Cloud / OpenAI / Google Gemini / Hugging Face.
- Model: preset `<select>` populated from `list_embedding_presets` for the chosen provider + "Custom…" option revealing free-text model input.
- Dimension: read-only display, filled by preset or by Test; editable only when Custom model chosen.
- API key: password input; hidden for Ollama Local; helper text "Falls back to your {provider} LLM key" for openai/gemini/ollama-cloud when a matching LLM key exists (`hasApiKey` from provider list); always required-or-stored for HF.
- Base URL: collapsed "Advanced" field (TEI self-host, proxies).
- Buttons: **Test** (calls `test_embedding_connection`, shows latency + detected dim, auto-fills dimension, error toast with provider message on failure) and **Save** (disabled until valid; on success toast "Saved — existing project indexes may be stale").
- State: Zustand slice (`stores/`) holding config + presets + test status; all IPC via wrappers.

### 7.2 Stale-index banner + manual re-index

- On project open (and after settings save), call `get_index_status`.
- `isStale` → non-blocking banner on project/artifact view: "Code index was built with {old model}; current embedding model is {new model}. RAG retrieval is degraded until you re-index." + **Re-index now** button.
- Re-index button → existing analyze flow (`analyze_project`), with progress UI the analysis flow already has; banner clears when status refreshes clean.
- No auto re-index, no modal nag — banner persists until re-indexed (decision: manual).

### 7.3 No regressions

- Existing LLM provider panel untouched except possible shared subcomponents (key input) — extract only if trivially clean, else duplicate.
- `console.log` forbidden; UI copy goes through whatever i18n/strings convention the settings panel already uses (match in-file literals if that's the current style).

## 8. Security

- API keys: AES-256-GCM via existing `utils/crypto.rs` `CryptoKey`, random 96-bit nonce per encryption, ciphertext+nonce in DB, decrypt only in-memory in service layer. Keys never serialized over IPC; views expose `hasApiKey: boolean` only.
- Keys never logged (no `tracing` of config structs containing decrypted keys — `EmbeddingConfig` gets a manual `Debug` impl redacting `api_key`, or `#[derive(Debug)]` is omitted; pick redacted manual Debug, matching how `ProviderConfig` handles it — check and mirror).
- Embedding calls send **code chunks to the configured cloud provider** — this changes the "static analysis only, no remote code upload" default-path story. UI must state it: one-line notice under cloud provider selection: "Code snippets from your project will be sent to {provider} for embedding." Local Ollama remains default. README/docs touch-up in Phase 2.
- HF model id is path-interpolated — validate charset at construction (§5.3) to prevent URL splicing.

## 9. Edge cases (handle + test explicitly)

1. Empty input batch → `Ok(vec![])` without HTTP (all impls — pattern exists in ollama.rs:112).
2. Response vector count ≠ input count → `InvalidResponse`.
3. Per-item dimension mismatch → `InvalidResponse` (skip when probe-mode dim 0).
4. HF single-string vs array response ambiguity → always send array (§4).
5. HF 503 model-loading → `ProviderUnavailable` + warm-up message.
6. 429 with `Retry-After` → `RateLimited { retry_after_seconds: Some(n) }` when parseable.
7. No active embedding config → Ollama default (back-compat; fresh installs unchanged).
8. Active config row for provider whose key was deleted from LLM configs (fallback gone) → `AuthFailed` at resolve time, surfaced as toast on analyze/generate.
9. Stale index → 0 RAG hits, generation still succeeds (RAG already degrades gracefully) + warn log + banner.
10. Project never indexed (`indexedWith: null`) → `isStale: false` (nothing to be stale).
11. Dimension 0 rows must never persist — `save` validates ≥ 1; resolver rejects (defense in depth).
12. Re-running analyze after provider switch must not leave mixed-provider chunks for one project → verify analyze wipes/overwrites; add `delete_by_project` if needed (§5.7).
13. Custom Ollama model not pulled → existing 404 "`ollama pull`" hint preserved (ollama.rs untouched).
14. `base_url` with trailing slash → normalize (reuse/extend `utils/provider_base_url.rs`).

## 10. Testing matrix

| Layer | Tests |
|---|---|
| `openai_compat.rs` | mockito: success, bearer header, `dimensions` param present/absent in body, 401, 429+Retry-After, 5xx, dim mismatch, empty input, probe mode (dim 0 skips validation) |
| `huggingface.rs` | mockito: success (nested array), URL contains model path, normalize/truncate in body, 503 warm-up, 401, dim mismatch, count mismatch, invalid model id rejected |
| `factory.rs` | each `EmbeddingProviderKind` builds; key required per kind; serde round-trip for new enum; default base URLs |
| `embedding_config_service` | save/get round-trip, exclusive `is_active`, key encryption (ciphertext ≠ plaintext, decrypts back), fallback-to-LLM-key resolution, no-row → Ollama default, dim validation |
| `embedding_config_repo` | upsert, fetch_active, unique (user, provider) |
| `chunk_repo` | `search_similar` excludes other model same provider+dim (new), existing scope tests updated |
| `generation_service` | `ScriptedEmbeddings` unchanged; retrieve passes model_id into search |
| index status | grouped-status query: clean, stale, never-indexed, legacy-provider-string row |
| `packages/shared` | round-trip contract tests for all new schemas |
| FE Vitest | panel renders per provider (key field visibility), test-button flow fills dimension, save validation, stale banner render + dismiss-on-clean |
| Playwright (if cheap) | settings open → switch provider → save → banner appears on project |

Coverage target: 80% on `embedding_config_service` + new providers (house rule).

## 11. Phases

### Phase 1 — Backend (everything compiles, tested, callable over IPC)

1. Migration `0005_embedding_config.sql` (table + index recreate) — §5.1, §5.6.
2. `EmbeddingProviderKind` + `EmbeddingConfig` + new `build_embedding_provider`; delete old signature + its tests — §5.2, §5.4.
3. `openai_compat.rs`, `huggingface.rs`, `presets.rs` + full test suites — §5.3.
4. Probe mode (dim 0) added to `ollama.rs` validation guard (only touch: one `if` + doc line) — §5.4.
5. `embedding_config_repo.rs` + `embedding_config_service.rs` (get/save/resolve/test/key-resolution) — §5.4.
6. `chunk_repo::search_similar` model scoping + caller updates — §5.6.
7. `commands/embeddings.rs` (5 commands) + register in `lib.rs`; replace hardcoded call-sites in `commands/analysis.rs` + `commands/generation.rs`; hoist `DEFAULT_USER_ID` — §5.5, §6.1.
8. Index-status query + service fn — §5.7. Verify analyze-overwrites-chunks; add `force`/wipe if not.
9. Zod schemas + contract tests in `packages/shared` — §6.2.
10. Gate: `pnpm guard:pre-push` green (typecheck, lint, tests, clippy pedantic).

### Phase 2 — Frontend + UX

1. IPC wrappers `lib/ipc/embeddings.ts` — §6.3.
2. Zustand slice + `embedding-config-panel.tsx` in Settings — §7.1.
3. Cloud-data notice copy (§8) + docs touch (README privacy paragraph, CLAUDE.md "What This Is" sentence).
4. Stale banner + re-index button on project view — §7.2.
5. FE tests (Vitest) + optional Playwright path — §10.
6. Gate: `pnpm guard:pre-push` green; manual smoke per §12.

### Out (committed follow-ups, not this branch)

- Generation-stream stale hint event (minimal tracing-only ships in Phase 1).
- Voyage/Cohere providers; sqlite-vec.

## 12. Manual verification script (post-Phase 2)

1. Fresh DB → analyze project → chunks embedded `ollama/nomic-embed-text/768` (default unchanged).
2. Settings → Embeddings → OpenAI → preset `text-embedding-3-small` → Test (expect ~1536 detected) → Save.
3. Project shows stale banner → Re-index → banner clears → `code_chunks` rows show `openai/text-embedding-3-small/1536`.
4. Generate artifact → RAG hits present (warn log absent).
5. Switch to HF `BAAI/bge-m3` with hf_ token → Test latency OK → Save → banner reappears → generate WITHOUT re-index → artifact still generates, warn logged, banner visible.
6. Delete HF key → analyze → clean `AuthFailed` toast, no panic.
7. Gemini path: key reused from existing Gemini LLM config without re-entering.

## 13. Acceptance criteria

- [ ] User can select embedding provider (5 kinds) + model independently of LLM provider.
- [ ] Hardcoded `OllamaEmbeddingProvider` constructions removed from commands; single resolver path.
- [ ] Keys encrypted at rest, never over IPC, never logged.
- [ ] `search_similar` scoped by model; no cross-model matches.
- [ ] No config → behavior identical to today (local Ollama default).
- [ ] Stale index detected + manual re-index works; generation never hard-fails on stale.
- [ ] All gates green: `pnpm guard:pre-push` (typecheck, ESLint, Vitest, cargo test, clippy pedantic `-D warnings`).
- [ ] Contract tests cover every new schema; Rust serde ↔ Zod literal strings identical.
