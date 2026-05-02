# ADR-0003: LlmProvider trait shape and streaming model

- **Status**: Accepted
- **Date**: 2026-05-02
- **Authors**: Backend / AI Pipeline (Student 2)
- **Supersedes**: none
- **Superseded by**: none

## Context

Phase 2 introduces the LLM abstraction that every downstream service
(context summarization, test plan generation, defect analysis,
embedding) consumes. The abstraction has to satisfy four hard
constraints from `rules.md`:

1. **§5.2 — Provider abstraction**: services see a trait, never an SDK.
   Swapping providers must not require a service-layer rewrite.
2. **§12.2 — Streaming is first-class**: every provider exposes a
   stream method; the artifact-generation UX depends on per-token
   feedback.
3. **§5.3 — Typed errors**: every fallible path returns a typed
   error with a stable IPC code. Generic `String` errors are not
   acceptable.
4. **§9 — Local-first, BYO key**: at least one implementation
   (Ollama Local) must work without any API key, so dev/test
   environments need no paid credentials.

Five providers ship in Phase 2:

- **OllamaProvider** — local OpenAI-compatible endpoint at
  `${OLLAMA_BASE_URL}/v1/chat/completions`. No auth, no
  rate limit signaling, plain-text SSE.
- **OpenAIProvider** — `https://api.openai.com/v1/chat/completions`.
  Bearer auth, OpenAI SSE format, `429` with `Retry-After` header.
- **OpenRouterProvider** — `https://openrouter.ai/api/v1/chat/completions`.
  Same shape as OpenAI; minor differences in usage reporting.
- **AnthropicProvider** — `https://api.anthropic.com/v1/messages`.
  Different request body (system at top level, content blocks),
  different stream event format (`event: content_block_delta`,
  `event: message_stop`), different auth header (`x-api-key` +
  `anthropic-version`).
- **OllamaEmbeddingProvider** — `${OLLAMA_BASE_URL}/v1/embeddings`,
  same hosting story as the chat endpoint.

Three of the four chat providers use the OpenAI request/response
shape. AnthropicProvider differs but the stream still ultimately
yields incremental text deltas plus a final usage block. The trait
must be wide enough to fit Anthropic's content-block model without
forcing OpenAI/Ollama/OpenRouter to wrap their flat strings in
ceremony.

## Decision

A single trait `LlmProvider` covering chat-style generation, plus a
parallel trait `EmbeddingProvider` for vector generation. Concrete
shape:

```rust
#[async_trait]
pub trait LlmProvider: Send + Sync {
    fn name(&self) -> &'static str;
    fn capabilities(&self) -> &ProviderCapabilities;
    fn count_tokens(&self, text: &str) -> usize;

    async fn generate(
        &self,
        request: GenerateRequest,
    ) -> Result<GenerateResponse, LlmError>;

    fn stream(
        &self,
        request: GenerateRequest,
    ) -> Pin<Box<dyn Stream<Item = Result<Chunk, LlmError>> + Send>>;
}

#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    fn name(&self) -> &'static str;
    fn dimension(&self) -> usize;
    fn model_id(&self) -> &str;

    async fn embed(
        &self,
        inputs: Vec<String>,
    ) -> Result<Vec<Vec<f32>>, LlmError>;
}
```

### Request/response types

- `GenerateRequest` — model, messages, optional tools (JSON schema
  function-calling per `rules.md` §12.1), temperature, max_tokens,
  stop_sequences.
- `Message` — role + content. `Content` is an enum: `Text(String)`,
  `ToolUse { id, name, args }`, `ToolResult { id, content }`. Lets
  Anthropic's content-block model and OpenAI's flat strings live in
  the same shape — providers translate at the wire boundary.
- `Role` — `System | User | Assistant | Tool`.
- `GenerateResponse` — final text, optional tool calls, usage stats
  (`input_tokens`, `output_tokens`).
- `Chunk` (streaming) — `TextDelta(String)`, `ToolCallStart { id,
  name }`, `ToolCallArgsDelta { id, json_fragment }`,
  `Done { usage }`.
- `ProviderCapabilities` — `supports_tools: bool`,
  `supports_streaming: bool`, `max_context_tokens: u32`,
  `max_output_tokens: u32`.

### Streaming model

Return `Pin<Box<dyn Stream<Item = Result<Chunk, LlmError>> + Send>>`
rather than an associated type. Reasoning:

- Trait objects need a concrete return type. An `impl Stream`
  associated type would force every consumer to be generic over the
  provider, breaking `Arc<dyn LlmProvider>` and the factory pattern.
- The boxing cost (one heap allocation per stream) is dominated by
  network I/O and LLM inference time. Not worth optimizing.
- `Send` bound is mandatory — streams cross await points that may
  be polled by multiple tokio worker threads.

### Token counting

`count_tokens` uses a heuristic (`text.chars().count() / 4`) for now.
Reasoning:

- Accurate tokenization requires per-model BPE tables (`tiktoken-rs`,
  Anthropic's `tokenizer`). That is a 5–10 MB binary asset per model
  and adds a dependency.
- Phase 2 callers use the count for budget planning, not billing.
  10–20% over/under is acceptable.
- Each provider can override with a more accurate implementation
  later (Phase 3+) without changing the trait shape.

### Error model

`LlmError` is a thiserror enum scoped to provider concerns:

```rust
pub enum LlmError {
    ConnectionFailed { provider: &'static str, source: reqwest::Error },
    AuthFailed { provider: &'static str, message: String },
    RateLimited { provider: &'static str, retry_after_seconds: Option<u64> },
    ContextExceeded { provider: &'static str, requested_tokens: u32, limit: u32 },
    InvalidResponse { provider: &'static str, message: String },
    SchemaValidationFailed { provider: &'static str, payload_preview: String },
    StreamInterrupted { provider: &'static str, message: String },
    ProviderUnavailable { provider: &'static str, message: String },
    Unsupported { provider: &'static str, feature: &'static str },
}
```

Bridges into `AppError::Llm(#[from] LlmError)` at the
service/command boundary. The existing
`AppError::LlmProvider(String)` variant is removed.

### Naming the bridge

`AppError::Llm(LlmError)` — short, explicit, no ambiguity with the
`Provider*` prefix used by config types.

## Consequences

### Positive

- One trait, four concrete providers, swappable at runtime via
  factory. Services never know which provider answered.
- Streaming is the primary path — the non-streaming `generate`
  helper is a thin loop over `stream` for callers who want the full
  text in one allocation.
- Errors are typed at every layer. Frontend gets a stable IPC code,
  user sees a redacted message, logs see the structured
  thiserror payload.
- Anthropic's content blocks fit the same `Content` enum as
  OpenAI's flat strings. The provider does the translation; the
  service does not branch on provider.
- Tool-calling uses JSON schema natively — interoperable with
  every provider that supports OpenAI-style or Anthropic-style
  tool calls.

### Negative

- Heap allocation per stream (boxed dyn). Negligible vs network +
  inference cost.
- Heuristic `count_tokens` is approximate. Will be tightened in
  Phase 3 once we have hard limits to enforce (`AppError::LimitExceeded`).
- Two traits to learn (`LlmProvider`, `EmbeddingProvider`). Worth
  the separation — one model emits text, the other emits vectors.

### Risks

- Provider quirks (Anthropic's `event: ping` chatter, OpenRouter's
  occasional non-standard usage block) bleed into the wire-format
  parsers. Mitigation: each provider gets its own SSE/JSON parser
  module, each with mockito tests covering the documented edge
  cases. No shared "universal" parser.

## Alternatives considered

1. **Single trait covering both chat and embeddings**: rejected.
   Forces a degenerate `embed` returning empty / `generate`
   returning empty per provider. Two traits is cleaner.
2. **Async trait via `impl Trait`**: rejected. Breaks dyn
   dispatch, breaks the factory pattern. Boxed streams are the
   pragmatic choice.
3. **Strings everywhere (no LlmError)**: rejected. Violates
   `rules.md` §5.3.
4. **Tokenization library per provider in Phase 2**: rejected.
   Cost (binary size, dep tree) too high for budget-planning
   accuracy that does not yet matter.

## References

- `rules.md` §5.2 (provider abstraction), §5.3 (typed errors),
  §12.1 (versioned prompts), §12.2 (streaming first-class),
  §13 (anti-pattern: SDK leaks)
- `plan/initial-plan.md` — Provider Configuration UI section
- `plan/tech-stack.md` — Approved providers
- ADR-0001 — embedding storage (downstream consumer of
  `EmbeddingProvider`)
- Future ADR-0002 — sqlite-vec vec0 migration trigger
