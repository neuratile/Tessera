use super::*;
use serde_json::json;
mod input_tests;
mod stream_tests;
use crate::{
    providers::llm::{Chunk, ChunkStream, FinishReason, ProviderCapabilities, Usage},
    repositories::provider_config_repo::ProviderConfigUpsert,
    repositories::review_repo::{
        GitObjectFormat, ReviewChange, ReviewChangeKind, ReviewSource, SourceSide,
    },
    services::git_change_service::{CapturedBlob, ChangeKind, ChangedFile, DiffHunk},
};
use async_trait::async_trait;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Mock {
    name: &'static str,
    tools: bool,
    chunks: Vec<Chunk>,
    calls: Arc<AtomicUsize>,
    pending: bool,
}
#[async_trait]
impl LlmProvider for Mock {
    fn name(&self) -> &'static str {
        self.name
    }
    fn capabilities(&self) -> &ProviderCapabilities {
        static TOOLS: ProviderCapabilities = ProviderCapabilities {
            supports_tools: true,
            supports_streaming: true,
            max_context_tokens: 32_000,
            max_output_tokens: 8_000,
        };
        static TEXT: ProviderCapabilities = ProviderCapabilities {
            supports_tools: false,
            supports_streaming: true,
            max_context_tokens: 32_000,
            max_output_tokens: 8_000,
        };
        if self.tools {
            &TOOLS
        } else {
            &TEXT
        }
    }
    fn count_tokens(&self, text: &str) -> usize {
        text.len().div_ceil(4)
    }
    fn stream(&self, request: GenerateRequest) -> ChunkStream {
        self.calls.fetch_add(1, Ordering::SeqCst);
        assert_eq!(request.model, "pinned-model");
        assert_eq!(request.max_tokens, Some(6_000));
        assert_eq!(!request.tools.is_empty(), self.tools);
        let encoded = serde_json::to_string(&request).unwrap();
        assert!(!encoded.contains("synthetic-secret-value"));
        if self.pending {
            Box::pin(futures::stream::pending())
        } else {
            Box::pin(futures::stream::iter(
                self.chunks.clone().into_iter().map(Ok),
            ))
        }
    }
}
fn fixture() -> (StagedChangeSet, SupportingCapture, ReviewSnapshot) {
    let id = Uuid::new_v4();
    let blob = CapturedBlob {
        path: "src/a.ts".into(),
        blob_id: "a".repeat(40),
        mode: "100644".into(),
        content_hash: {
            use sha2::{Digest, Sha256};
            format!(
                "{:x}",
                Sha256::digest(b"const token = 'synthetic-secret-value';\nconst a = 1;\n")
            )
        },
        content: b"const token = 'synthetic-secret-value';\nconst a = 1;\n".to_vec(),
    };
    let manifest = ReviewSnapshot {
        snapshot_id: id,
        git_object_format: GitObjectFormat::Sha1,
        base_commit: None,
        index_fingerprint: "c".repeat(64),
        sources: vec![ReviewSource {
            path: blob.path.clone(),
            side: SourceSide::New,
            blob_id: blob.blob_id.clone(),
            mode: blob.mode.clone(),
            content_hash: blob.content_hash.clone(),
            byte_length: u32::try_from(blob.content.len()).unwrap(),
            line_count: 2,
        }],
        changes: vec![ReviewChange {
            change_kind: ReviewChangeKind::Added,
            old_path: None,
            new_path: Some(blob.path.clone()),
        }],
        context_paths: vec![],
        exclusions: vec![],
    };
    let capture = StagedChangeSet {
        snapshot_id: id.to_string(),
        git_object_format: "sha1".into(),
        base_commit: None,
        index_fingerprint: "c".repeat(64),
        files: vec![ChangedFile {
            change_kind: ChangeKind::Added,
            old: None,
            new: Some(blob),
            hunks: vec![DiffHunk {
                old_start: 0,
                old_lines: 0,
                new_start: 1,
                new_lines: 2,
                patch: String::new(),
            }],
        }],
        excluded_files: vec![],
    };
    let support = SupportingCapture {
        snapshot_id: id.to_string(),
        index_fingerprint: "c".repeat(64),
        files: vec![],
        omitted: vec![],
    };
    (capture, support, manifest)
}
fn done() -> Chunk {
    Chunk::Done {
        usage: Usage {
            input_tokens: 123,
            output_tokens: 40,
        },
        finish_reason: FinishReason::Stop,
    }
}
fn tool(payload: &str) -> Vec<Chunk> {
    vec![
        Chunk::ToolCallStart {
            id: "call".into(),
            name: review_v1::TOOL_NAME.into(),
        },
        Chunk::ToolCallArgsDelta {
            id: "call".into(),
            json_fragment: payload.into(),
        },
        done(),
    ]
}
fn finding(manifest: &ReviewSnapshot) -> Value {
    json!({"findings":[{"summary":"A defect","explanation":"Unexpected outcome","expectedBehavior":"Expected outcome","suggestedTest":"Assert outcome","severity":"major","sources":[{"snapshotId":manifest.snapshot_id,"path":"src/a.ts","side":"new","blobId":"a".repeat(40),"contentHash":manifest.sources[0].content_hash,"lineStart":2,"lineEnd":2}]}]})
}
async fn invoke(
    (chunks, tools, pending): (Vec<Chunk>, bool, bool),
    active: bool,
    selected: bool,
    name: &'static str,
    timeout: Duration,
    cancel: bool,
    payload_mutation: Option<(&str, Value)>,
) -> (AppResult<ReviewGenerationOutcome>, usize) {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    crate::db::run_migrations(&pool).await.unwrap();
    let id = provider_config_repo::upsert(
        &pool,
        ProviderConfigUpsert {
            provider: name.into(),
            api_key_encrypted: None,
            api_key_nonce: None,
            base_url: None,
            default_model: Some("other-default".into()),
            is_active: active,
        },
    )
    .await
    .unwrap();
    let (capture, support, manifest) = fixture();
    let chunks = if let Some((field, value)) = payload_mutation {
        let mut payload = finding(&manifest);
        payload["findings"][0]["sources"][0][field] = value;
        tool(&payload.to_string())
    } else {
        chunks
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let mock = Mock {
        name: if name == "ollama-cloud" {
            "ollama"
        } else {
            name
        },
        tools,
        chunks,
        calls: calls.clone(),
        pending,
    };
    let cancelled = async move {
        if !cancel {
            futures::future::pending::<()>().await;
        }
    };
    let result = run(
        &pool,
        &CryptoKey::from_bytes([9; 32]),
        ReviewRequest {
            provider_config_id: selected.then(|| Uuid::parse_str(&id).unwrap()),
            model: "pinned-model",
            capture: &capture,
            support: &support,
            manifest: &manifest,
        },
        cancelled,
        timeout,
        |_| Ok(Arc::new(mock)),
    )
    .await;
    pool.close().await;
    (result, calls.load(Ordering::SeqCst))
}
async fn check(chunks: Vec<Chunk>) -> AppResult<ReviewGenerationOutcome> {
    let (result, calls) = invoke(
        (chunks, true, false),
        true,
        true,
        "ollama",
        Duration::from_secs(2),
        false,
        None,
    )
    .await;
    assert_eq!(calls, 1);
    result
}
#[tokio::test]
async fn valid_empty_tool_and_text_provider_shapes() {
    for (name, tools) in [
        ("openai", true),
        ("anthropic", true),
        ("gemini", true),
        ("ollama", false),
        ("ollama-cloud", false),
        ("openrouter", true),
    ] {
        let chunks = if tools {
            tool("{\"findings\":[]}")
        } else {
            vec![Chunk::TextDelta("{\"findings\":[]}".into()), done()]
        };
        let (result, calls) = invoke(
            (chunks, tools, false),
            true,
            true,
            name,
            Duration::from_secs(2),
            false,
            None,
        )
        .await;
        let outcome = result.unwrap();
        assert_eq!(outcome.findings, []);
        assert_eq!(calls, 1);
        assert_eq!(outcome.provenance.model, "pinned-model");
        assert_eq!(outcome.provenance.prompt_version, "review_v1");
        assert_eq!(outcome.provenance.input_tokens, Some(123));
    }
}
#[tokio::test]
async fn rejects_malformed_missing_empty_and_duplicate_json_without_repair() {
    for payload in [
        "",
        "{",
        "{}",
        "{\"findings\":[],\"extra\":true}",
        "{\"findings\":[],\"findings\":[]}",
        "```json\n{\"findings\":[]}\n```",
        "{\"findings\":[],\"evidenceState\":\"reproduced\"}",
    ] {
        assert!(check(tool(payload)).await.is_err());
    }
}
#[tokio::test]
async fn rejects_invented_citation_identity_and_ranges() {
    for (field, value) in [
        ("path", json!("src/invented.ts")),
        ("side", json!("old")),
        ("snapshotId", json!(Uuid::new_v4())),
        ("blobId", json!("f".repeat(40))),
        ("contentHash", json!("f".repeat(64))),
        ("lineEnd", json!(3)),
        ("lineStart", json!(0)),
    ] {
        let (result, calls) = invoke(
            (vec![], true, false),
            true,
            true,
            "ollama",
            Duration::from_secs(2),
            false,
            Some((field, value)),
        )
        .await;
        assert!(result.is_err(), "{field}");
        assert_eq!(calls, 1);
    }
}
#[tokio::test]
async fn rejects_incomplete_and_bad_terminal_streams() {
    for chunks in [
        vec![Chunk::TextDelta("{\"findings\":[]}".into())],
        vec![done(), done()],
        vec![
            Chunk::ToolCallArgsDelta {
                id: "unknown".into(),
                json_fragment: "{}".into(),
            },
            done(),
        ],
        vec![
            Chunk::TextDelta("{\"findings\":[]}".into()),
            Chunk::Done {
                usage: Usage::default(),
                finish_reason: FinishReason::MaxTokens,
            },
        ],
        vec![
            Chunk::TextDelta("{\"findings\":[]}".into()),
            Chunk::Done {
                usage: Usage {
                    input_tokens: 1,
                    output_tokens: 6001,
                },
                finish_reason: FinishReason::Stop,
            },
        ],
    ] {
        assert!(check(chunks).await.is_err());
    }
}
#[tokio::test]
async fn output_bytes_timeout_and_cancellation_are_bounded() {
    assert!(
        check(vec![Chunk::TextDelta("x".repeat(128 * 1024 + 1)), done()])
            .await
            .is_err()
    );
    for cancel in [false, true] {
        let (result, calls) = invoke(
            (vec![], true, true),
            true,
            true,
            "ollama",
            Duration::from_millis(10),
            cancel,
            None,
        )
        .await;
        assert!(result.is_err());
        assert_eq!(calls, usize::from(!cancel));
    }
}
#[tokio::test]
async fn no_active_connection_never_calls_provider() {
    for (active, selected) in [(false, true), (true, false)] {
        let (result, calls) = invoke(
            (tool("{\"findings\":[]}"), true, false),
            active,
            selected,
            "ollama",
            Duration::from_secs(2),
            false,
            None,
        )
        .await;
        assert!(result.is_err());
        assert_eq!(calls, 0);
    }
}
#[test]
fn validates_valid_findings_as_suspected_and_rejects_unsupplied_ranges() {
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
    let mut context = prepare(&request, &mock).unwrap();
    fit(&request, &mut context, &mock, &review_v1::tool_schema()).unwrap();
    let payload = finding(&manifest).to_string();
    let findings = output::validate(
        &payload,
        &review_v1::tool_schema(),
        &manifest,
        &context,
        &[],
        "ollama",
    )
    .unwrap();
    assert_eq!(
        findings[0].evidence_state,
        crate::repositories::review_repo::EvidenceState::Suspected
    );
    assert_eq!(
        findings[0].suggested_test.as_deref(),
        Some("Assert outcome")
    );
    assert_ne!(findings[0].finding_id, Uuid::nil());
    context.excerpts.clear();
    assert!(output::validate(
        &payload,
        &review_v1::tool_schema(),
        &manifest,
        &context,
        &[],
        "ollama"
    )
    .is_err());
}
