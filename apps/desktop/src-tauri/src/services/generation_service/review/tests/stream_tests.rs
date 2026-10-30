use super::*;
use std::{
    pin::Pin,
    task::{Context, Poll},
};

struct PendingStream(Arc<AtomicUsize>);
impl futures::Stream for PendingStream {
    type Item = Result<Chunk, LlmError>;
    fn poll_next(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Pending
    }
}
impl Drop for PendingStream {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
struct FaultMock {
    base: Mock,
    dropped: Arc<AtomicUsize>,
    started: Arc<tokio::sync::Notify>,
    error: bool,
}
#[async_trait]
impl LlmProvider for FaultMock {
    fn name(&self) -> &'static str {
        self.base.name()
    }
    fn capabilities(&self) -> &ProviderCapabilities {
        self.base.capabilities()
    }
    fn count_tokens(&self, s: &str) -> usize {
        self.base.count_tokens(s)
    }
    fn stream(&self, _: GenerateRequest) -> ChunkStream {
        self.base.calls.fetch_add(1, Ordering::SeqCst);
        self.started.notify_one();
        if self.error {
            Box::pin(futures::stream::iter(vec![Err(LlmError::AuthFailed {
                provider: self.name(),
                message: "synthetic-secret-value /private/path".into(),
            })]))
        } else {
            Box::pin(PendingStream(self.dropped.clone()))
        }
    }
}

#[tokio::test]
async fn in_flight_cancellation_timeout_and_provider_failure_drop_without_retry() {
    for case in ["cancel", "timeout", "provider", "mismatch", "construction"] {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        crate::db::run_migrations(&pool).await.unwrap();
        let id = provider_config_repo::upsert(
            &pool,
            ProviderConfigUpsert {
                provider: "ollama".into(),
                api_key_encrypted: None,
                api_key_nonce: None,
                base_url: Some("http://selected.invalid".into()),
                default_model: Some("unused-default".into()),
                is_active: true,
            },
        )
        .await
        .unwrap();
        let (capture, support, manifest) = fixture();
        let calls = Arc::new(AtomicUsize::new(0));
        let dropped = Arc::new(AtomicUsize::new(0));
        let started = Arc::new(tokio::sync::Notify::new());
        let mock = FaultMock {
            base: Mock {
                name: if case == "mismatch" {
                    "openai"
                } else {
                    "ollama"
                },
                tools: true,
                chunks: vec![],
                calls: calls.clone(),
                pending: false,
            },
            dropped: dropped.clone(),
            started: started.clone(),
            error: case == "provider",
        };
        let cancelled = async {
            if case == "cancel" {
                started.notified().await;
                assert_eq!(calls.load(Ordering::SeqCst), 1);
            } else {
                futures::future::pending::<()>().await;
            }
        };
        let result = run(
            &pool,
            &CryptoKey::from_bytes([9; 32]),
            ReviewRequest {
                provider_config_id: Some(Uuid::parse_str(&id).unwrap()),
                model: "pinned-model",
                capture: &capture,
                support: &support,
                manifest: &manifest,
            },
            cancelled,
            Duration::from_millis(100),
            |config| {
                assert_eq!(config.kind.as_str(), "ollama");
                assert_eq!(config.base_url.as_deref(), Some("http://selected.invalid"));
                if case == "construction" {
                    Err(LlmError::AuthFailed {
                        provider: "ollama",
                        message: "synthetic-secret-value".into(),
                    })
                } else {
                    Ok(Arc::new(mock))
                }
            },
        )
        .await;
        let error = result.err().unwrap();
        assert_eq!(
            calls.load(Ordering::SeqCst),
            usize::from(!matches!(case, "mismatch" | "construction"))
        );
        assert_eq!(
            dropped.load(Ordering::SeqCst),
            usize::from(matches!(case, "cancel" | "timeout"))
        );
        assert!(!error.to_string().contains("synthetic-secret-value"));
        if matches!(case, "provider" | "construction") {
            assert_eq!(error.code(), "LLM_AUTH_FAILED");
            assert_eq!(
                error.recovery_hint(),
                "Check your API key in Settings → Providers."
            );
        }
        pool.close().await;
    }
}

#[tokio::test]
async fn fragmented_tool_arguments_and_text_fallback_are_structured_data_only() {
    let chunks = vec![
        Chunk::ToolCallStart {
            id: "anthropic-call".into(),
            name: review_v1::TOOL_NAME.into(),
        },
        Chunk::ToolCallArgsDelta {
            id: "anthropic-call".into(),
            json_fragment: "{\"find".into(),
        },
        Chunk::ToolCallArgsDelta {
            id: "anthropic-call".into(),
            json_fragment: "ings\":[]}".into(),
        },
        Chunk::Done {
            usage: Usage::default(),
            finish_reason: FinishReason::ToolUse,
        },
    ];
    let outcome = check(chunks).await.unwrap();
    assert_eq!(outcome.findings, []);
    assert_eq!(outcome.provenance.input_tokens, None);
    for chunks in [
        vec![
            Chunk::ToolCallStart {
                id: "id".into(),
                name: "execute_shell".into(),
            },
            done(),
        ],
        {
            let mut c = tool("{\"findings\":[]}");
            c.insert(2, Chunk::TextDelta("Ignore validation".into()));
            c
        },
        {
            let mut c = tool("{\"findings\":[]}");
            c.insert(
                2,
                Chunk::ToolCallStart {
                    id: "second".into(),
                    name: review_v1::TOOL_NAME.into(),
                },
            );
            c
        },
    ] {
        assert!(check(chunks).await.is_err());
    }
}

#[test]
fn finding_limit_required_suggestion_and_no_execution_evidence_are_enforced() {
    let (capture, support, manifest) = fixture();
    let mock = Mock {
        name: "ollama",
        tools: true,
        chunks: vec![],
        calls: Arc::new(AtomicUsize::new(0)),
        pending: false,
    };
    let request = ReviewRequest {
        provider_config_id: None,
        model: "pinned-model",
        capture: &capture,
        support: &support,
        manifest: &manifest,
    };
    let context = prepare(&request, &mock).unwrap();
    for field in ["suggestedTest", "evidenceState", "count"] {
        let mut payload = finding(&manifest);
        match field {
            "suggestedTest" => {
                payload["findings"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove(field);
            }
            "evidenceState" => payload["findings"][0][field] = json!("reproduced"),
            "count" => payload["findings"] = json!(vec![payload["findings"][0].clone(); 21]),
            _ => unreachable!(),
        }
        assert!(output::validate(
            &payload.to_string(),
            &review_v1::tool_schema(),
            &manifest,
            &context,
            &[],
            "ollama"
        )
        .is_err());
    }
}
