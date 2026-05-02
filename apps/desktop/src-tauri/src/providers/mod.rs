//! External-service integrations: LLM providers + embedding providers.
//!
//! Per `rules.md` §5.2 + §12.2: services depend on the `LlmProvider` /
//! `EmbeddingProvider` traits, never concrete implementations. Concrete
//! providers (Ollama, `OpenAI`, Anthropic, `OpenRouter`) are selected by a
//! factory at runtime based on user configuration.
//!
//! Sub-modules added in Phase 2: `llm` (mod, ollama, openai, anthropic,
//! openrouter), `embeddings` (mod, ollama).
