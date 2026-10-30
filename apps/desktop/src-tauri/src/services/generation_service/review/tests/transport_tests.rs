//! Exercise raw HTTP/SSE through the real factory and review entry point.
use super::*;
use mockito::{Matcher, Server};
use std::io::Write;
use std::sync::Mutex;
use tracing::instrument::WithSubscriber;

fn event(value: &Value) -> String {
    format!("data: {value}\n\n")
}

fn text_stream(reason: Option<&str>, sentinel: bool) -> String {
    let mut wire = event(&json!({"choices":[{"delta":{"content":"{\"findings\":[]}"}}]}));
    if let Some(reason) = reason {
        wire += &event(&json!({"choices":[{"delta":{},"finish_reason":reason}]}));
    }
    wire += &event(&json!({"choices":[],"usage":{"prompt_tokens":123,"completion_tokens":40}}));
    if sentinel {
        wire += "data: [DONE]\n\n";
    }
    wire
}

fn tool_stream() -> String {
    event(
        &json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_abc","function":{"name":review_v1::TOOL_NAME,"arguments":"{\"find"}}]}}]}),
    ) + &event(
        &json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"ings\":[]}"}}]}}]}),
    ) + &event(&json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}]}))
        + &event(&json!({"choices":[],"usage":{"prompt_tokens":123,"completion_tokens":40}}))
        + "data: [DONE]\n\n"
}

async fn real_review(provider: &str, base: &str) -> AppResult<ReviewGenerationOutcome> {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    crate::db::run_migrations(&pool).await.unwrap();
    let crypto = CryptoKey::from_bytes([9; 32]);
    let id = provider_config_service::save_config(
        &pool,
        &crypto,
        provider.into(),
        Some("transport-test-key".into()),
        Some(base.into()),
        Some("unused-default".into()),
        true,
    )
    .await
    .unwrap();
    let (capture, support, manifest) = fixture();
    let result = generate_review(
        &pool,
        &crypto,
        ReviewRequest {
            provider_config_id: Some(Uuid::parse_str(&id).unwrap()),
            model: "pinned-model",
            capture: &capture,
            support: &support,
            manifest: &manifest,
        },
        futures::future::pending(),
    )
    .await;
    pool.close().await;
    result
}

#[tokio::test]
async fn real_factory_accepts_text_and_fragmented_tools_with_one_completion_and_usage() {
    for provider in ["openai", "ollama-cloud", "ollama", "openrouter", "gemini"] {
        for body in [
            text_stream(Some("stop"), true),
            tool_stream().replace('\n', "\r\n"),
        ] {
            let mut server = Server::new_async().await;
            let path = if provider == "gemini" {
                "/v1beta/openai/chat/completions"
            } else {
                "/v1/chat/completions"
            };
            let mock = server
                .mock("POST", path)
                .match_body(Matcher::PartialJson(
                    json!({"model":"pinned-model","max_tokens":6000}),
                ))
                .with_status(200)
                .with_body(body)
                .expect(1)
                .create_async()
                .await;
            let result = real_review(provider, &server.url()).await.unwrap();
            assert_eq!(result.findings, []);
            assert_eq!(
                serde_json::to_value(result.provenance.provider).unwrap(),
                provider
            );
            assert_eq!(result.provenance.input_tokens, Some(123));
            assert_eq!(result.provenance.output_tokens, Some(40));
            mock.assert_async().await;
        }
    }
}

#[tokio::test]
async fn real_transport_rejects_cutoff_incomplete_and_post_completion_data() {
    for body in [
        text_stream(Some("length"), true),
        text_stream(Some("content_filter"), true),
        text_stream(None, false),
        text_stream(Some("stop"), true)
            + &event(&json!({"choices":[{"delta":{"content":"extra"}}]})),
    ] {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("POST", "/v1/chat/completions")
            .with_status(200)
            .with_body(body)
            .expect(1)
            .create_async()
            .await;
        assert!(real_review("openai", &server.url()).await.is_err());
        mock.assert_async().await;
    }
    // Existing compatible endpoints may finish at EOF without a [DONE] sentinel.
    let mut server = Server::new_async().await;
    let mock = server
        .mock("POST", "/v1/chat/completions")
        .with_status(200)
        .with_body(text_stream(Some("stop"), false))
        .expect(1)
        .create_async()
        .await;
    assert!(real_review("openai", &server.url()).await.is_ok());
    mock.assert_async().await;
}

#[derive(Clone, Default)]
struct Logs(Arc<Mutex<Vec<u8>>>);
impl Write for Logs {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn review_tool_errors_send_one_request_and_never_log_raw_provider_data() {
    for provider in ["openai", "ollama-cloud", "ollama", "openrouter", "gemini"] {
        let mut server = Server::new_async().await;
        let path = if provider == "gemini" {
            "/v1beta/openai/chat/completions"
        } else {
            "/v1/chat/completions"
        };
        let body = json!({"error":{"message":format!("unsupported tool/function: opaque-body-credential {}", server.url())}}).to_string();
        let mock = server
            .mock("POST", path)
            .with_status(400)
            .with_body(body)
            .expect(1)
            .create_async()
            .await;
        let logs = Logs::default();
        let writer = logs.clone();
        let subscriber = tracing_subscriber::fmt()
            .without_time()
            .with_ansi(false)
            .with_max_level(tracing::Level::WARN)
            .with_writer(move || writer.clone())
            .finish();
        let error = real_review(provider, &server.url())
            .with_subscriber(subscriber)
            .await
            .err()
            .unwrap();
        assert_eq!(error.code(), "LLM_INVALID_RESPONSE");
        let logged = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
        for secret in [
            "opaque-body-credential",
            "transport-test-key",
            &server.url(),
        ] {
            assert!(!logged.contains(secret), "provider data leaked in tracing");
            assert!(!error.to_string().contains(secret));
        }
        mock.assert_async().await;
    }
}

#[tokio::test]
async fn both_wire_formats_bound_unterminated_events_and_error_bodies() {
    for provider in ["openai", "anthropic"] {
        for (status, body, code) in [
            (
                200,
                "data: ".to_owned() + &"x".repeat(2 * 1024 * 1024),
                "LLM_INVALID_RESPONSE",
            ),
            (400, "x".repeat(2 * 1024 * 1024), "LLM_INVALID_RESPONSE"),
            (401, "x".repeat(2 * 1024 * 1024), "LLM_AUTH_FAILED"),
            (429, "x".repeat(2 * 1024 * 1024), "LLM_RATE_LIMITED"),
        ] {
            let mut server = Server::new_async().await;
            let path = if provider == "anthropic" {
                "/v1/messages"
            } else {
                "/v1/chat/completions"
            };
            let mock = server
                .mock("POST", path)
                .with_status(status)
                .with_body(body)
                .expect(1)
                .create_async()
                .await;
            let error = real_review(provider, &server.url()).await.err().unwrap();
            assert_eq!(error.code(), code);
            mock.assert_async().await;
        }
    }
}

#[tokio::test]
async fn real_openai_transport_bounds_identity_state_without_collector() {
    for (count, id_len, function) in [
        (3, 65_536, Value::Null),
        (3, 65_536, json!({"name":"test"})),
        (65, 1, json!({"name":"test"})),
        (17, 1024, json!({"name":"test"})),
    ] {
        let mut server = Server::new_async().await;
        let body: String = (0..count)
            .map(|index| {
                event(&json!({"choices":[{"delta":{"tool_calls":[{
                    "index":index,"id":"x".repeat(id_len),"function":function
                }]}}]}))
            })
            .collect();
        let mock = server
            .mock("POST", "/v1/chat/completions")
            .with_status(200)
            .with_body(body)
            .expect(1)
            .create_async()
            .await;
        let provider = factory::build_llm_provider(&ProviderConfig {
            kind: factory::ProviderKind::OpenAi,
            api_key: Some("transport-test-key".into()),
            base_url: Some(server.url()),
        })
        .unwrap();
        let mut stream = provider.stream(GenerateRequest {
            model: "pinned-model".into(),
            messages: vec![Message::user("test")],
            tools: vec![],
            temperature: None,
            max_tokens: None,
            stop_sequences: vec![],
        });
        let mut failure = None;
        while let Some(chunk) = stream.next().await {
            if let Err(error) = chunk {
                failure = Some(error);
                break;
            }
        }
        assert_eq!(failure.unwrap().code(), "LLM_INVALID_RESPONSE");
        mock.assert_async().await;
    }
}

#[tokio::test]
async fn real_anthropic_stream_errors_preserve_sanitized_classification() {
    for (kind, code) in [
        ("overloaded_error", "LLM_PROVIDER_UNAVAILABLE"),
        ("api_error", "LLM_PROVIDER_UNAVAILABLE"),
        ("authentication_error", "LLM_AUTH_FAILED"),
        ("permission_error", "LLM_AUTH_FAILED"),
        ("rate_limit_error", "LLM_RATE_LIMITED"),
        ("invalid_request_error", "LLM_INVALID_RESPONSE"),
    ] {
        let mut server = Server::new_async().await;
        let body = event(&json!({"type":"error","error":{
            "type":kind,"message":format!("opaque-body-credential {}", server.url())
        }}));
        let mock = server
            .mock("POST", "/v1/messages")
            .with_status(200)
            .with_body(body)
            .expect(1)
            .create_async()
            .await;
        let error = real_review("anthropic", &server.url()).await.err().unwrap();
        assert_eq!(error.code(), code, "{kind}");
        assert!(!error.to_string().contains("opaque-body-credential"));
        assert!(!error.to_string().contains(&server.url()));
        if kind == "overloaded_error" {
            assert!(error.recovery_hint().contains("temporary failure"));
        }
        mock.assert_async().await;
    }
}

#[tokio::test]
async fn default_artifact_stream_keeps_tool_fallback_without_logging_error_body() {
    let mut server = Server::new_async().await;
    let failure = server
        .mock("POST", "/v1/chat/completions")
        .match_body(Matcher::PartialJson(
            json!({"tool_choice":{"type":"function"}}),
        ))
        .with_status(400)
        .with_body(format!(
            "unsupported tool: opaque-body-credential {}",
            server.url()
        ))
        .expect(1)
        .create_async()
        .await;
    let success = server
        .mock("POST", "/v1/chat/completions")
        .match_body(Matcher::PartialJson(
            json!({"response_format":{"type":"json_object"}}),
        ))
        .with_status(200)
        .with_body(text_stream(Some("stop"), true))
        .expect(1)
        .create_async()
        .await;
    let provider = factory::build_llm_provider(&ProviderConfig {
        kind: factory::ProviderKind::OpenAi,
        api_key: Some("transport-test-key".into()),
        base_url: Some(server.url()),
    })
    .unwrap();
    let logs = Logs::default();
    let writer = logs.clone();
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_max_level(tracing::Level::WARN)
        .with_writer(move || writer.clone())
        .finish();
    let response = provider
        .generate(GenerateRequest {
            model: "pinned-model".into(),
            messages: vec![Message::user("test")],
            tools: vec![review_v1::tool_schema()],
            temperature: None,
            max_tokens: None,
            stop_sequences: vec![],
        })
        .with_subscriber(subscriber)
        .await
        .unwrap();
    assert_eq!(response.usage.input_tokens, 123);
    assert_eq!(response.usage.output_tokens, 40);
    let logged = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
    assert!(logged.contains("retrying without tool use"));
    for secret in [
        "opaque-body-credential",
        "transport-test-key",
        &server.url(),
    ] {
        assert!(!logged.contains(secret));
    }
    failure.assert_async().await;
    success.assert_async().await;
}
