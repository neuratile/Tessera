//! Phase 7 — live Ollama integration tests.
//!
//! Per `rules.md` §7: integration tests live under `tests/` so they
//! compile against the public API only. These tests talk to a real
//! Ollama daemon and are **opt-in**: a default `cargo test` run does
//! not require Ollama to be installed or reachable.
//!
//! ## Running
//!
//! ```bash
//! ollama serve &                              # one terminal
//! ollama pull nomic-embed-text                # 274 MB
//! ollama pull qwen2.5-coder:1.5b              # smallest chat model
//!
//! OLLAMA_INTEGRATION=1 cargo test --test integration_ollama -- --nocapture
//! ```
//!
//! Override the endpoint or models via env (defaults shown):
//!
//! - `OLLAMA_BASE_URL` (default `http://localhost:11434`)
//! - `OLLAMA_TEST_CHAT_MODEL` (default `qwen2.5-coder:1.5b`)
//! - `OLLAMA_TEST_EMBED_MODEL` (default `nomic-embed-text`)
//!
//! When `OLLAMA_INTEGRATION` is unset, every test prints a skip
//! notice and exits 0 so the binary still passes in CI without a
//! local Ollama install.

use std::env;

use testing_ide_lib::providers::embeddings::{EmbeddingProvider, OllamaEmbeddingProvider};
use testing_ide_lib::providers::llm::types::{Content, GenerateRequest, Message, Role};
use testing_ide_lib::providers::llm::{ollama::OllamaProvider, LlmProvider};

const ENABLE_VAR: &str = "OLLAMA_INTEGRATION";
const BASE_URL_VAR: &str = "OLLAMA_BASE_URL";
const CHAT_MODEL_VAR: &str = "OLLAMA_TEST_CHAT_MODEL";
const EMBED_MODEL_VAR: &str = "OLLAMA_TEST_EMBED_MODEL";

const DEFAULT_BASE_URL: &str = "http://localhost:11434";
const DEFAULT_CHAT_MODEL: &str = "qwen2.5-coder:1.5b";
const DEFAULT_EMBED_MODEL: &str = "nomic-embed-text";

/// Returns `true` only when the opt-in env var is set to a truthy value.
/// Anything that is not `1` / `true` / `yes` (case-insensitive) is treated
/// as "off" so a stray `OLLAMA_INTEGRATION=0` does not flip the suite on.
fn integration_enabled() -> bool {
    match env::var(ENABLE_VAR) {
        Ok(v) => matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes"),
        Err(_) => false,
    }
}

fn skip(test_name: &str) {
    eprintln!(
        "[skip] {test_name}: set {ENABLE_VAR}=1 (and have Ollama running) to enable. \
         See apps/desktop/src-tauri/tests/integration_ollama.rs."
    );
}

fn base_url() -> String {
    env::var(BASE_URL_VAR).unwrap_or_else(|_| DEFAULT_BASE_URL.to_string())
}

fn chat_model() -> String {
    env::var(CHAT_MODEL_VAR).unwrap_or_else(|_| DEFAULT_CHAT_MODEL.to_string())
}

fn embed_model() -> String {
    env::var(EMBED_MODEL_VAR).unwrap_or_else(|_| DEFAULT_EMBED_MODEL.to_string())
}

#[tokio::test]
async fn ollama_embeddings_round_trip() {
    if !integration_enabled() {
        skip("ollama_embeddings_round_trip");
        return;
    }

    let model = embed_model();
    let provider = OllamaEmbeddingProvider::with_model(base_url(), &model, 768)
        .expect("build embedding provider");

    let inputs = vec![
        "the quick brown fox jumps over the lazy dog".to_string(),
        "fn main() { println!(\"hello, world\"); }".to_string(),
    ];

    let vectors = provider
        .embed(inputs.clone())
        .await
        .expect("embedding call must succeed against live Ollama");

    assert_eq!(vectors.len(), inputs.len(), "one vector per input text");
    for v in &vectors {
        assert_eq!(v.len(), 768, "nomic-embed-text returns 768-dim vectors");
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!(
            norm > 0.0 && norm.is_finite(),
            "embedding vector must be non-zero and finite (norm = {norm})"
        );
    }
}

#[tokio::test]
async fn ollama_chat_generate_round_trip() {
    if !integration_enabled() {
        skip("ollama_chat_generate_round_trip");
        return;
    }

    let provider = OllamaProvider::new(base_url()).expect("build llm provider");

    let request = GenerateRequest {
        model: chat_model(),
        messages: vec![Message {
            role: Role::User,
            content: vec![Content::Text {
                text: "Reply with the single word: PONG".to_string(),
            }],
        }],
        tools: Vec::new(),
        temperature: Some(0.0),
        max_tokens: Some(32),
        stop_sequences: Vec::new(),
    };

    let response = provider
        .generate(request)
        .await
        .expect("generate call must succeed against live Ollama");

    let text: String = response
        .content
        .iter()
        .filter_map(|c| match c {
            Content::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("");

    assert!(
        !text.trim().is_empty(),
        "expected non-empty completion, got: {text:?}"
    );
    assert!(
        response.usage.output_tokens > 0,
        "expected at least one output token, got usage = {:?}",
        response.usage
    );
}
