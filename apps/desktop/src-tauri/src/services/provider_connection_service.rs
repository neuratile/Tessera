//! Provider connection testing service.
//!
//! Per `rules.md` sections 4.2 and 5.2: this service owns the business logic
//! for validating provider credentials and enumerating accessible models. The
//! Tauri command layer passes IPC arguments in, and this service turns them
//! into outbound HTTP probes against the selected provider.

use std::time::{Duration, Instant};

use reqwest::header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION};
use reqwest::Client;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::config::{AppConfig, DEFAULT_OLLAMA_CLOUD_BASE_URL};
use crate::error::AppResult;
use crate::providers::factory::ProviderKind;
use crate::providers::llm::{anthropic, gemini, openai, openrouter};
use crate::repositories::provider_config_repo;
use crate::services::provider_config_service;
use crate::utils::crypto::CryptoKey;

const DEFAULT_USER_ID: &str = "00000000-0000-4000-8000-000000000001";
const CONNECTION_TEST_TIMEOUT_SECS: u64 = 15;

/// User-supplied input for a provider connection test.
#[derive(Debug, Clone)]
pub struct ProviderConnectionTestArgs {
    pub provider: String,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub default_model: Option<String>,
}

/// Frontend-facing result for `test_provider_connection`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConnectionTestResult {
    pub ok: bool,
    pub message: String,
    pub latency_ms: u64,
    pub models: Vec<String>,
}

#[derive(Debug, Clone)]
struct ResolvedConnectionTestArgs {
    kind: ProviderKind,
    api_key: Option<String>,
    base_url: String,
    default_model: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAiModelListResponse {
    data: Vec<OpenAiModelListEntry>,
}

#[derive(Debug, Deserialize)]
struct OpenAiModelListEntry {
    id: String,
}

#[derive(Debug, Deserialize)]
struct AnthropicModelListResponse {
    data: Vec<AnthropicModelListEntry>,
}

#[derive(Debug, Deserialize)]
struct AnthropicModelListEntry {
    id: String,
}

#[derive(Debug, Deserialize)]
struct OllamaTagsResponse {
    models: Vec<OllamaTagEntry>,
}

#[derive(Debug, Deserialize)]
struct OllamaTagEntry {
    name: String,
}

/// Test a provider connection using either the explicitly supplied values or
/// any previously saved config as fallback.
///
/// Missing API keys are treated as a handled failure (`ok = false`) rather
/// than a thrown error because the caller is explicitly asking for a diagnostic
/// payload.
///
/// # Errors
///
/// Returns an application error when:
/// - the provider id is invalid
/// - a stored config row cannot be decrypted
/// - the HTTP client cannot be constructed
pub async fn test_connection(
    pool: &SqlitePool,
    crypto: &CryptoKey,
    cfg: &AppConfig,
    args: ProviderConnectionTestArgs,
) -> AppResult<ProviderConnectionTestResult> {
    let kind = ProviderKind::from_str_value(&args.provider)?;
    let stored_row =
        provider_config_repo::fetch_for_user_provider(pool, DEFAULT_USER_ID, kind.as_str()).await?;
    let stored_config = match stored_row.as_ref() {
        Some(row) => Some(provider_config_service::build_provider_config(crypto, row)?),
        None => None,
    };
    let resolved = resolve_args(kind, cfg, stored_row.as_ref(), stored_config.as_ref(), args);
    let client = build_client()?;

    if resolved.kind.requires_api_key() && resolved.api_key.is_none() {
        return Ok(ProviderConnectionTestResult {
            ok: false,
            message: "API key not configured for this provider.".into(),
            latency_ms: 0,
            models: Vec::new(),
        });
    }

    let started_at = Instant::now();
    let models_result = fetch_models(&client, &resolved).await;
    let latency_ms = elapsed_ms(started_at);

    Ok(match models_result {
        Ok(models) => finalize_success(&resolved, latency_ms, models),
        Err(message) => ProviderConnectionTestResult {
            ok: false,
            message,
            latency_ms,
            models: Vec::new(),
        },
    })
}

fn resolve_args(
    kind: ProviderKind,
    cfg: &AppConfig,
    stored_row: Option<&provider_config_repo::ProviderConfigRow>,
    stored_config: Option<&crate::providers::factory::ProviderConfig>,
    args: ProviderConnectionTestArgs,
) -> ResolvedConnectionTestArgs {
    let api_key = match args.api_key {
        Some(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        None => stored_config.and_then(|config| config.api_key.clone()),
    };

    let base_url = match args.base_url {
        Some(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                default_base_url_for(kind, cfg)
            } else {
                provider_config_service::normalize_base_url(kind, trimmed)
            }
        }
        None => stored_config
            .and_then(|config| config.base_url.clone())
            .map_or_else(
                || default_base_url_for(kind, cfg),
                |value| provider_config_service::normalize_base_url(kind, &value),
            ),
    };

    let default_model = match args.default_model {
        Some(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        None => stored_row.and_then(|row| row.default_model.clone()),
    };

    ResolvedConnectionTestArgs {
        kind,
        api_key,
        base_url,
        default_model,
    }
}

fn default_base_url_for(kind: ProviderKind, cfg: &AppConfig) -> String {
    let default_url = match kind {
        ProviderKind::Ollama => cfg.ollama_base_url.clone(),
        ProviderKind::OllamaCloud => DEFAULT_OLLAMA_CLOUD_BASE_URL.to_string(),
        ProviderKind::OpenAi => openai::DEFAULT_BASE_URL.to_string(),
        ProviderKind::OpenRouter => openrouter::DEFAULT_BASE_URL.to_string(),
        ProviderKind::Anthropic => anthropic::DEFAULT_BASE_URL.to_string(),
        ProviderKind::Gemini => gemini::DEFAULT_BASE_URL.to_string(),
    };

    provider_config_service::normalize_base_url(kind, &default_url)
}

fn build_client() -> AppResult<Client> {
    Client::builder()
        .timeout(Duration::from_secs(CONNECTION_TEST_TIMEOUT_SECS))
        .build()
        .map_err(Into::into)
}

async fn fetch_models(
    client: &Client,
    args: &ResolvedConnectionTestArgs,
) -> Result<Vec<String>, String> {
    match args.kind {
        ProviderKind::Ollama => {
            let response: OllamaTagsResponse = fetch_json(
                client,
                &build_url(&args.base_url, "/api/tags"),
                HeaderMap::new(),
            )
            .await?;
            Ok(response
                .models
                .into_iter()
                .map(|model| model.name)
                .collect())
        }
        ProviderKind::OllamaCloud => {
            let mut headers = HeaderMap::new();
            insert_bearer_auth(
                &mut headers,
                args.api_key
                    .as_deref()
                    .ok_or_else(|| "API key not configured for this provider.".to_string())?,
            )?;
            let response: OllamaTagsResponse =
                fetch_json(client, &build_url(&args.base_url, "/api/tags"), headers).await?;
            Ok(response
                .models
                .into_iter()
                .map(|model| model.name)
                .collect())
        }
        ProviderKind::OpenAi => {
            let mut headers = HeaderMap::new();
            insert_bearer_auth(
                &mut headers,
                args.api_key
                    .as_deref()
                    .ok_or_else(|| "API key not configured for this provider.".to_string())?,
            )?;
            let response: OpenAiModelListResponse =
                fetch_json(client, &build_url(&args.base_url, "/v1/models"), headers).await?;
            Ok(response.data.into_iter().map(|model| model.id).collect())
        }
        ProviderKind::OpenRouter => {
            let mut headers = HeaderMap::new();
            insert_bearer_auth(
                &mut headers,
                args.api_key
                    .as_deref()
                    .ok_or_else(|| "API key not configured for this provider.".to_string())?,
            )?;
            let response: OpenAiModelListResponse =
                fetch_json(client, &build_url(&args.base_url, "/v1/models"), headers).await?;
            Ok(response.data.into_iter().map(|model| model.id).collect())
        }
        ProviderKind::Anthropic => {
            let mut headers = HeaderMap::new();
            insert_plain_header(
                &mut headers,
                HeaderName::from_static("x-api-key"),
                args.api_key
                    .as_deref()
                    .ok_or_else(|| "API key not configured for this provider.".to_string())?,
            )?;
            insert_plain_header(
                &mut headers,
                HeaderName::from_static("anthropic-version"),
                anthropic::ANTHROPIC_VERSION,
            )?;
            let response: AnthropicModelListResponse =
                fetch_json(client, &build_url(&args.base_url, "/v1/models"), headers).await?;
            Ok(response.data.into_iter().map(|model| model.id).collect())
        }
        ProviderKind::Gemini => {
            let mut headers = HeaderMap::new();
            insert_bearer_auth(
                &mut headers,
                args.api_key
                    .as_deref()
                    .ok_or_else(|| "API key not configured for this provider.".to_string())?,
            )?;
            let url = build_url(
                &args.base_url,
                &format!("{}/models", gemini::OPENAI_COMPAT_PATH),
            );
            let response: OpenAiModelListResponse = fetch_json(client, &url, headers).await?;
            // The compatibility endpoint returns fully-qualified ids
            // (`models/gemini-2.5-flash`); strip the prefix so the list
            // matches the bare model names used on chat requests.
            Ok(response
                .data
                .into_iter()
                .map(|model| match model.id.strip_prefix("models/") {
                    Some(bare) => bare.to_string(),
                    None => model.id,
                })
                .collect())
        }
    }
}

async fn fetch_json<T: DeserializeOwned>(
    client: &Client,
    url: &str,
    headers: HeaderMap,
) -> Result<T, String> {
    let response = client
        .get(url)
        .headers(headers)
        .send()
        .await
        .map_err(|error| error.to_string())?;

    let status = response.status();
    let body = response.text().await.map_err(|error| error.to_string())?;

    if !status.is_success() {
        return Err(format!(
            "request failed with status {}: {}",
            status.as_u16(),
            truncate_for_message(&body)
        ));
    }

    serde_json::from_str(&body).map_err(|error| format!("response parse failed: {error}"))
}

fn insert_bearer_auth(headers: &mut HeaderMap, api_key: &str) -> Result<(), String> {
    let mut value = HeaderValue::from_str(&format!("Bearer {api_key}"))
        .map_err(|_| "API key contains invalid characters for an HTTP header.".to_string())?;
    value.set_sensitive(true);
    headers.insert(AUTHORIZATION, value);
    Ok(())
}

fn insert_plain_header(
    headers: &mut HeaderMap,
    name: HeaderName,
    value: &str,
) -> Result<(), String> {
    let mut header_value = HeaderValue::from_str(value)
        .map_err(|_| format!("invalid header value for {}", name.as_str()))?;
    header_value.set_sensitive(name == HeaderName::from_static("x-api-key"));
    headers.insert(name, header_value);
    Ok(())
}

fn build_url(base_url: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base_url.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

fn truncate_for_message(message: &str) -> String {
    const MAX_MESSAGE_CHARS: usize = 160;
    let trimmed = message.trim();
    if trimmed.is_empty() {
        return "empty response body".into();
    }
    let shortened: String = trimmed.chars().take(MAX_MESSAGE_CHARS).collect();
    if trimmed.chars().count() <= MAX_MESSAGE_CHARS {
        shortened
    } else {
        format!("{shortened}...")
    }
}

fn finalize_success(
    args: &ResolvedConnectionTestArgs,
    latency_ms: u64,
    models: Vec<String>,
) -> ProviderConnectionTestResult {
    let model_is_available = match args.default_model.as_deref() {
        Some(selected) => models
            .iter()
            .any(|model| is_model_match(args.kind, selected, model)),
        None => true,
    };

    let message = match args.default_model.as_deref() {
        Some(selected_model) if !model_is_available => {
            format!("Connected, but model `{selected_model}` was not returned by the provider.")
        }
        _ => "Connection successful.".into(),
    };

    ProviderConnectionTestResult {
        ok: model_is_available,
        message,
        latency_ms,
        models,
    }
}

fn is_model_match(kind: ProviderKind, selected_model: &str, candidate_model: &str) -> bool {
    if kind == ProviderKind::Ollama || kind == ProviderKind::OllamaCloud {
        return candidate_model == selected_model
            || candidate_model
                .strip_prefix(selected_model)
                .is_some_and(|suffix| suffix.starts_with(':') || suffix.starts_with('-'));
    }

    candidate_model == selected_model
}

fn elapsed_ms(started_at: Instant) -> u64 {
    let millis = started_at.elapsed().as_millis();
    u64::try_from(millis).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_pool_at;
    use crate::services::provider_config_service::save_config;
    use std::path::PathBuf;
    use uuid::Uuid;

    fn tmp_db() -> PathBuf {
        std::env::temp_dir().join(format!("testing-ide-provider-test-{}.db", Uuid::new_v4()))
    }

    fn test_key() -> CryptoKey {
        CryptoKey::derive_from_secret("phase-9-provider-test-secret")
    }

    fn test_config() -> AppConfig {
        AppConfig {
            ollama_base_url: "http://localhost:11434".into(),
            db_path: None,
            log_level: "info".into(),
            jwt_secret: "0123456789abcdef0123456789abcdef".into(),
            jwt_access_ttl_secs: 900,
            jwt_refresh_ttl_secs: 60 * 60 * 24 * 7,
            sentry_dsn: None,
        }
    }

    #[tokio::test]
    async fn test_connection_reports_missing_api_key_as_handled_failure() {
        let path = tmp_db();
        let pool = init_pool_at(&path).await.expect("pool");
        let result = test_connection(
            &pool,
            &test_key(),
            &test_config(),
            ProviderConnectionTestArgs {
                provider: "openai".into(),
                api_key: None,
                base_url: Some("https://api.openai.com".into()),
                default_model: None,
            },
        )
        .await
        .expect("result");

        assert!(!result.ok);
        assert!(result.message.contains("API key"));
        assert_eq!(result.models.len(), 0);

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_connection_uses_saved_key_when_args_omit_it() {
        let path = tmp_db();
        let pool = init_pool_at(&path).await.expect("pool");
        let crypto = test_key();
        let mut server = mockito::Server::new_async().await;

        save_config(
            &pool,
            &crypto,
            "openai".into(),
            Some("sk-saved-123".into()),
            Some(server.url()),
            Some("gpt-4o-mini".into()),
            true,
        )
        .await
        .expect("save");

        let mock = server
            .mock("GET", "/v1/models")
            .match_header("authorization", "Bearer sk-saved-123")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"data":[{"id":"gpt-4o-mini"},{"id":"gpt-4o"}]}"#)
            .create_async()
            .await;

        let result = test_connection(
            &pool,
            &crypto,
            &test_config(),
            ProviderConnectionTestArgs {
                provider: "openai".into(),
                api_key: None,
                base_url: Some(server.url()),
                default_model: Some("gpt-4o-mini".into()),
            },
        )
        .await
        .expect("result");

        assert!(result.ok);
        assert_eq!(
            result.models,
            vec!["gpt-4o-mini".to_string(), "gpt-4o".to_string()]
        );
        mock.assert_async().await;

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_connection_parses_anthropic_model_list() {
        let path = tmp_db();
        let pool = init_pool_at(&path).await.expect("pool");
        let mut server = mockito::Server::new_async().await;

        let mock = server
            .mock("GET", "/v1/models")
            .match_header("x-api-key", "sk-ant-123")
            .match_header("anthropic-version", anthropic::ANTHROPIC_VERSION)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"data":[{"id":"claude-sonnet-4-20250514","display_name":"Claude Sonnet 4","type":"model","created_at":"2025-02-19T00:00:00Z"}]}"#,
            )
            .create_async()
            .await;

        let result = test_connection(
            &pool,
            &test_key(),
            &test_config(),
            ProviderConnectionTestArgs {
                provider: "anthropic".into(),
                api_key: Some("sk-ant-123".into()),
                base_url: Some(server.url()),
                default_model: Some("claude-sonnet-4-20250514".into()),
            },
        )
        .await
        .expect("result");

        assert!(result.ok);
        assert_eq!(result.models, vec!["claude-sonnet-4-20250514".to_string()]);
        mock.assert_async().await;

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_connection_parses_gemini_model_list_and_strips_prefix() {
        let path = tmp_db();
        let pool = init_pool_at(&path).await.expect("pool");
        let mut server = mockito::Server::new_async().await;

        let mock = server
            .mock("GET", "/v1beta/openai/models")
            .match_header("authorization", "Bearer AIza-123")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"data":[{"id":"models/gemini-2.5-flash"},{"id":"models/gemini-2.5-pro"}]}"#,
            )
            .create_async()
            .await;

        let result = test_connection(
            &pool,
            &test_key(),
            &test_config(),
            ProviderConnectionTestArgs {
                provider: "gemini".into(),
                api_key: Some("AIza-123".into()),
                base_url: Some(server.url()),
                default_model: Some("gemini-2.5-flash".into()),
            },
        )
        .await
        .expect("result");

        assert!(result.ok);
        assert_eq!(
            result.models,
            vec![
                "gemini-2.5-flash".to_string(),
                "gemini-2.5-pro".to_string()
            ]
        );
        mock.assert_async().await;

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_connection_accepts_ollama_tagged_model_variants() {
        let path = tmp_db();
        let pool = init_pool_at(&path).await.expect("pool");
        let mut server = mockito::Server::new_async().await;

        let mock = server
            .mock("GET", "/api/tags")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"models":[{"name":"qwen2.5-coder:7b-instruct-q4_K_M"},{"name":"nomic-embed-text:latest"}]}"#,
            )
            .create_async()
            .await;

        let result = test_connection(
            &pool,
            &test_key(),
            &test_config(),
            ProviderConnectionTestArgs {
                provider: "ollama".into(),
                api_key: None,
                base_url: Some(server.url()),
                default_model: Some("qwen2.5-coder:7b".into()),
            },
        )
        .await
        .expect("result");

        assert!(result.ok);
        assert_eq!(result.models.len(), 2);
        mock.assert_async().await;

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_connection_reports_missing_selected_model() {
        let path = tmp_db();
        let pool = init_pool_at(&path).await.expect("pool");
        let mut server = mockito::Server::new_async().await;

        let mock = server
            .mock("GET", "/v1/models")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"data":[{"id":"gpt-4o"}]}"#)
            .create_async()
            .await;

        let result = test_connection(
            &pool,
            &test_key(),
            &test_config(),
            ProviderConnectionTestArgs {
                provider: "openai".into(),
                api_key: Some("sk-openai".into()),
                base_url: Some(server.url()),
                default_model: Some("gpt-4o-mini".into()),
            },
        )
        .await
        .expect("result");

        assert!(!result.ok);
        assert!(result.message.contains("gpt-4o-mini"));
        mock.assert_async().await;

        pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn normalize_base_url_strips_api_or_version_suffix() {
        assert_eq!(
            provider_config_service::normalize_base_url(ProviderKind::OllamaCloud, "https://ollama.com/api/"),
            "https://ollama.com"
        );
        assert_eq!(
            provider_config_service::normalize_base_url(ProviderKind::OllamaCloud, "https://ollama.com/v1/"),
            "https://ollama.com"
        );
        assert_eq!(
            provider_config_service::normalize_base_url(ProviderKind::Anthropic, "https://api.anthropic.com/v1/"),
            "https://api.anthropic.com"
        );
    }
}
