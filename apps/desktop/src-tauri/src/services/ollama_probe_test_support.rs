use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use serde_json::Value as JsonValue;

use crate::config::{AppConfig, DEFAULT_OLLAMA_BASE_URL};
use crate::db::init_pool_at;
use crate::prompts::test_cases_v2;
use crate::prompts::test_plan_v2;
use crate::prompts::PromptContext;
use crate::providers::factory::{self, ProviderConfig, ProviderKind};
use crate::providers::llm::{
    approximate_token_count, Content, GenerateRequest, Message, ToolSchema,
};
use crate::repositories::artifact_repo::ArtifactType;
use crate::services::ast_service;
use crate::services::chunking_service::{self, Chunk, ChunkKind};
use crate::services::file_discovery_service::{self, FileType};
use crate::services::provider_connection_service::{self, ProviderConnectionTestArgs};
use crate::services::generation_service::normalize_missing_arrays;
use crate::utils::crypto::CryptoKey;
use uuid::Uuid;

const DEFAULT_PROJECT_NAME: &str = "express-api-fixture";
const DEFAULT_SCOPE_HINT: &str = "auth module";
const JSON_RESULT_PREFIX: &str = "JSON_RESULT:";
const MAX_FIXTURE_CHUNKS: usize = 18;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GoldenProbeOutput {
    artifact_type: String,
    prompt_version: String,
    model: String,
    scope_hint: String,
    chunk_count: usize,
    usage_input_tokens: u32,
    usage_output_tokens: u32,
    structured_data: JsonValue,
}

#[derive(Debug)]
struct FixtureContext {
    project_summary: String,
    chunks: Vec<Chunk>,
}

fn test_config() -> AppConfig {
    AppConfig {
        ollama_base_url: DEFAULT_OLLAMA_BASE_URL.to_string(),
        db_path: None,
        log_level: "info".to_string(),
        jwt_secret: "0123456789abcdef0123456789abcdef".to_string(),
        jwt_access_ttl_secs: 900,
        jwt_refresh_ttl_secs: 60 * 60 * 24 * 7,
        sentry_dsn: None,
    }
}

fn required_env(key: &str) -> Result<String> {
    env::var(key).with_context(|| format!("missing required env var {key}"))
}

fn optional_env(key: &str) -> Option<String> {
    env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn parse_artifact_type(value: &str) -> Result<ArtifactType> {
    match value.trim() {
        "test-plan" | "test_plan" => Ok(ArtifactType::TestPlan),
        "test-cases" | "test_cases" => Ok(ArtifactType::TestCases),
        other => Err(anyhow!("unsupported artifact type `{other}`")),
    }
}

fn select_prompt(
    artifact_type: ArtifactType,
    context: &PromptContext<'_>,
) -> (Vec<Message>, ToolSchema, &'static str) {
    // Mirrors `generation_service::build_prompt` v2 routing so the golden
    // suite exercises the same prompts the desktop app ships.
    match artifact_type {
        ArtifactType::TestPlan => (
            test_plan_v2::build_messages(context),
            test_plan_v2::tool(),
            test_plan_v2::VERSION,
        ),
        ArtifactType::TestCases => (
            test_cases_v2::build_messages(context),
            test_cases_v2::tool(),
            test_cases_v2::VERSION,
        ),
        _ => unreachable!("probe only accepts test-plan and test-cases"),
    }
}

fn annotate_chunk(relative_path: &str, mut chunk: Chunk) -> Chunk {
    chunk.content = format!("File: {relative_path}\n{}", chunk.content);
    chunk
}

fn module_chunk(relative_path: &str, content: &str) -> Chunk {
    let annotated = format!("File: {relative_path}\n{content}");
    Chunk {
        kind: ChunkKind::Module,
        name: relative_path.to_string(),
        start_line: 1,
        end_line: u32::try_from(content.lines().count().max(1)).unwrap_or(u32::MAX),
        token_count: approximate_token_count(&annotated),
        oversize: false,
        content: annotated,
    }
}

fn build_project_summary(
    report: &file_discovery_service::DiscoveryReport,
    readme_summary: Option<&str>,
) -> String {
    let source_paths: Vec<&str> = report
        .files
        .iter()
        .filter(|file| file.file_type == FileType::Source)
        .map(|file| file.relative_path.as_str())
        .collect();
    let config_paths: Vec<&str> = report
        .files
        .iter()
        .filter(|file| file.file_type == FileType::Config)
        .map(|file| file.relative_path.as_str())
        .collect();

    let mut parts = Vec::new();
    if let Some(readme) = readme_summary {
        let trimmed = readme.trim();
        if !trimmed.is_empty() {
            parts.push(trimmed.to_string());
        }
    }

    parts.push(format!(
        "The fixture contains {} source files and {} config files. Key source files: {}.",
        source_paths.len(),
        config_paths.len(),
        source_paths
            .iter()
            .take(6)
            .copied()
            .collect::<Vec<_>>()
            .join(", ")
    ));

    if !config_paths.is_empty() {
        parts.push(format!(
            "Relevant config files: {}.",
            config_paths
                .iter()
                .take(4)
                .copied()
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    parts.join("\n\n")
}

fn load_fixture_context(root: &Path) -> Result<FixtureContext> {
    let report = file_discovery_service::discover(root).with_context(|| {
        format!(
            "failed to discover golden fixture files under {}",
            root.display()
        )
    })?;
    let readme_path = root.join("README.md");
    let readme_summary = if readme_path.exists() {
        Some(
            fs::read_to_string(&readme_path)
                .with_context(|| format!("failed to read {}", readme_path.display()))?,
        )
    } else {
        None
    };

    let mut chunks = Vec::new();
    for file in &report.files {
        let full_path = root.join(&file.relative_path);
        match file.file_type {
            FileType::Source => {
                let source = fs::read_to_string(&full_path)
                    .with_context(|| format!("failed to read {}", full_path.display()))?;
                let parsed = ast_service::parse(&source, file.language).with_context(|| {
                    format!("failed to parse source fixture file {}", file.relative_path)
                })?;
                let file_chunks = chunking_service::chunk_source(&source, &parsed);
                if file_chunks.is_empty() {
                    chunks.push(module_chunk(&file.relative_path, &source));
                } else {
                    chunks.extend(
                        file_chunks
                            .into_iter()
                            .map(|chunk| annotate_chunk(&file.relative_path, chunk)),
                    );
                }
            }
            FileType::Config if file.relative_path == "package.json" => {
                let package_json = fs::read_to_string(&full_path)
                    .with_context(|| format!("failed to read {}", full_path.display()))?;
                chunks.push(module_chunk(&file.relative_path, &package_json));
            }
            _ => {}
        }
    }

    if chunks.is_empty() {
        return Err(anyhow!(
            "fixture at {} produced zero prompt chunks",
            root.display()
        ));
    }

    let project_summary = build_project_summary(&report, readme_summary.as_deref());
    chunks.truncate(MAX_FIXTURE_CHUNKS);

    Ok(FixtureContext {
        project_summary,
        chunks,
    })
}

fn extract_tool_arguments(response_content: &[Content], tool_name: &str) -> Result<String> {
    let mut free_text = String::new();
    for content in response_content {
        match content {
            Content::ToolUse { name, args, .. } if name == tool_name => return Ok(args.clone()),
            Content::Text { text } => free_text.push_str(text),
            _ => {}
        }
    }

    // Small / non-tool-trained models often ignore the `tools`
    // parameter and emit the JSON payload as plain text. The probe
    // mirrors the production salvage path in
    // `generation_service::salvage_tool_args` which handles three
    // shapes: bare payload, name+arguments tool-call wrapper, and
    // per-item wrappers (rejected). Keeps the golden suite green on
    // the same model the desktop ships with (`qwen2.5-coder:1.5b`
    // in CI, `qwen2.5-coder:7b` locally).
    if let Some(salvaged) =
        crate::services::generation_service::salvage_tool_args(&free_text, tool_name)
    {
        return Ok(salvaged);
    }

    let preview: String = free_text.chars().take(240).collect();
    let tail: String = {
        let chars: Vec<char> = free_text.chars().collect();
        chars[chars.len().saturating_sub(120)..].iter().collect()
    };
    // Distinguish the three salvage-failure shapes so CI logs point at
    // the real culprit:
    //  - balanced JSON present but wrapper shape rejected (per-item
    //    wrapper) → model quality issue, swap model;
    //  - a `{` with no balanced close → output truncated by max_tokens;
    //  - no `{` at all → model emitted prose only.
    if crate::services::generation_service::salvage_json_from_text(&free_text).is_some() {
        return Err(anyhow!(
            "model did not invoke `{tool_name}`; free text contained a JSON \
             object but its wrapper shape is unsalvageable (per-item tool-call \
             wrapper) — swap to a tool-trained model; head: {preview}"
        ));
    }
    if free_text.contains('{') {
        return Err(anyhow!(
            "model did not invoke `{tool_name}` and its free-text JSON is \
             unbalanced — output likely truncated by max_tokens; \
             head: {preview}; tail: {tail}"
        ));
    }
    Err(anyhow!(
        "model did not invoke `{tool_name}` and free text contained no JSON object; \
         preview: {preview}"
    ))
}

fn validate_tool_output(tool: &ToolSchema, data: &JsonValue) -> Result<()> {
    let tool_name = tool.name.clone();
    let validator = jsonschema::JSONSchema::compile(&tool.parameters_schema).map_err(|error| {
        anyhow!("tool schema for `{tool_name}` failed to compile as JSON Schema: {error}")
    })?;
    let errors: Vec<String> = validator
        .validate(data)
        .err()
        .map(|errs| errs.map(|err| err.to_string()).collect())
        .unwrap_or_default();
    if errors.is_empty() {
        return Ok(());
    }

    let preview = errors.into_iter().take(3).collect::<Vec<_>>().join("; ");
    Err(anyhow!(
        "model output for `{}` failed JSON-Schema validation: {preview}",
        tool.name
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "integration probe for Vitest Ollama connection coverage"]
    async fn provider_connection_probe_emits_json() {
        let base_url = required_env("OLLAMA_PROBE_BASE_URL").expect("probe base url");
        let default_model = required_env("OLLAMA_PROBE_MODEL").expect("probe model");
        let provider =
            optional_env("OLLAMA_PROBE_PROVIDER").unwrap_or_else(|| "ollama".to_string());

        let db_path =
            env::temp_dir().join(format!("testing-ide-provider-probe-{}.db", Uuid::new_v4()));
        let pool = init_pool_at(&db_path).await.expect("probe pool");
        let config = test_config();
        let crypto = CryptoKey::derive_from_secret(&config.jwt_secret);

        let result = provider_connection_service::test_connection(
            &pool,
            &crypto,
            &config,
            ProviderConnectionTestArgs {
                provider,
                api_key: optional_env("OLLAMA_PROBE_API_KEY"),
                base_url: Some(base_url),
                default_model: Some(default_model),
            },
        )
        .await
        .expect("provider probe");

        println!(
            "{JSON_RESULT_PREFIX}{}",
            serde_json::to_string(&result).expect("serialize provider probe")
        );

        pool.close().await;
        let _ignored = std::fs::remove_file(db_path);
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "integration probe for Vitest golden Ollama coverage"]
    async fn golden_generation_probe_emits_json() {
        let fixture_root =
            PathBuf::from(required_env("OLLAMA_GOLDEN_FIXTURE_ROOT").expect("fixture root"));
        let base_url = required_env("OLLAMA_GOLDEN_BASE_URL").expect("golden base url");
        let model = required_env("OLLAMA_GOLDEN_MODEL").expect("golden model");
        let artifact_type = parse_artifact_type(
            &required_env("OLLAMA_GOLDEN_ARTIFACT_TYPE").expect("golden artifact type"),
        )
        .expect("artifact type");
        let project_name = optional_env("OLLAMA_GOLDEN_PROJECT_NAME")
            .unwrap_or_else(|| DEFAULT_PROJECT_NAME.to_string());
        let scope_hint = optional_env("OLLAMA_GOLDEN_SCOPE_HINT")
            .unwrap_or_else(|| DEFAULT_SCOPE_HINT.to_string());

        let fixture = load_fixture_context(&fixture_root).expect("fixture context");
        let provider = factory::build_llm_provider(&ProviderConfig {
            kind: ProviderKind::Ollama,
            base_url: Some(base_url),
            api_key: None,
        })
        .expect("ollama provider");

        let prompt_context = PromptContext {
            project_name: &project_name,
            project_summary: &fixture.project_summary,
            chunks: &fixture.chunks,
            scope_hint: &scope_hint,
            reviewer_feedback: "",
        };
        let (messages, tool_schema, prompt_version) = select_prompt(artifact_type, &prompt_context);

        let response = provider
            .generate(GenerateRequest {
                model: model.clone(),
                messages,
                tools: vec![tool_schema.clone()],
                temperature: Some(0.1),
                // Mirror the production generation budget
                // (`RESPONSE_RESERVE_TOKENS`). The test-cases payload now
                // carries a runnable `files[]` array on top of the cases,
                // which overruns a 4k cap on the 3B model CI ships —
                // truncating the free-text JSON the model emits mid-array
                // so the salvage path cannot balance the object and the
                // probe fails. Reading the const (now 6k) keeps probe +
                // prod in lockstep so this stays fixed in one place.
                max_tokens: Some(crate::services::generation_service::RESPONSE_RESERVE_TOKENS),
                stop_sequences: Vec::new(),
            })
            .await
            .expect("golden generation");

        let tool_args =
            extract_tool_arguments(&response.content, &tool_schema.name).expect("tool args");
        let mut structured_data: JsonValue = serde_json::from_str(&tool_args).expect("structured JSON");
        normalize_missing_arrays(&mut structured_data, &tool_schema);
        validate_tool_output(&tool_schema, &structured_data).expect("tool schema validation");

        let output = GoldenProbeOutput {
            artifact_type: match artifact_type {
                ArtifactType::TestPlan => "test-plan".to_string(),
                ArtifactType::TestCases => "test-cases".to_string(),
                _ => unreachable!("probe only accepts test-plan and test-cases"),
            },
            prompt_version: prompt_version.to_string(),
            model,
            scope_hint,
            chunk_count: fixture.chunks.len(),
            usage_input_tokens: response.usage.input_tokens,
            usage_output_tokens: response.usage.output_tokens,
            structured_data,
        };

        println!(
            "{JSON_RESULT_PREFIX}{}",
            serde_json::to_string(&output).expect("serialize golden probe")
        );
    }
}
