//! Provider configuration IPC commands.
//!
//! Per `rules.md` §4.2.1 + §9: manages encrypted API key storage.
//! API keys are encrypted before persistence and never returned in
//! plaintext over IPC.

use serde::Deserialize;
use sqlx::SqlitePool;
use tauri::State;

use crate::config::AppConfig;
use crate::services::ollama_health_service::{self, OllamaModelInfo};
use crate::services::provider_config_service::{self, ProviderConfigView};
use crate::services::provider_connection_service::{self, ProviderConnectionTestResult};
use crate::utils::crypto::CryptoKey;

/// IPC payload for `save_provider_config`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveProviderArgs {
    pub provider: String,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub default_model: Option<String>,
    #[serde(default = "default_true")]
    pub is_active: bool,
}

/// IPC payload for `test_provider_connection`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestProviderConnectionArgs {
    pub provider: String,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub default_model: Option<String>,
}

fn default_true() -> bool {
    true
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub async fn save_provider_config(
    pool: State<'_, SqlitePool>,
    crypto: State<'_, CryptoKey>,
    args: SaveProviderArgs,
) -> Result<String, String> {
    provider_config_service::save_config(
        &pool,
        &crypto,
        args.provider,
        args.api_key,
        args.base_url,
        args.default_model,
        args.is_active,
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_provider_configs(
    pool: State<'_, SqlitePool>,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<Vec<ProviderConfigView>, String> {
    provider_config_service::list_configs(&pool, limit, offset)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub async fn delete_provider_config(pool: State<'_, SqlitePool>, id: String) -> Result<(), String> {
    provider_config_service::delete_config(&pool, &id)
        .await
        .map_err(|e| e.to_string())
}

/// Probe a provider endpoint and return latency plus any accessible models.
///
/// Delegates to `provider_connection_service` so the actual probe logic
/// (Ollama daemon hit, cloud-provider construction-only check) lives
/// alongside the rest of the provider business logic.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub async fn test_provider_connection(
    pool: State<'_, SqlitePool>,
    crypto: State<'_, CryptoKey>,
    cfg: State<'_, AppConfig>,
    args: TestProviderConnectionArgs,
) -> Result<ProviderConnectionTestResult, String> {
    provider_connection_service::test_connection(
        &pool,
        &crypto,
        &cfg,
        provider_connection_service::ProviderConnectionTestArgs {
            provider: args.provider,
            api_key: args.api_key,
            base_url: args.base_url,
            default_model: args.default_model,
        },
    )
    .await
    .map_err(|error| error.to_string())
}

/// List the locally pulled Ollama models. Returns an error when the
/// daemon is unreachable; the UI uses presence/absence of the chosen
/// model to decide whether to surface an `ollama pull <model>` hint.
///
/// `base_url` falls back to the configured `AppConfig::ollama_base_url`
/// when omitted. This avoids the previous behavior of silently routing
/// to `http://localhost:11434` even when the user had configured a
/// remote daemon. The HTTP probe itself lives in `ollama_health_service`.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub async fn list_ollama_models(
    cfg: State<'_, AppConfig>,
    base_url: Option<String>,
) -> Result<Vec<OllamaModelInfo>, String> {
    let base = base_url
        .as_deref()
        .unwrap_or(cfg.ollama_base_url.as_str());
    ollama_health_service::list_models(base)
        .await
        .map_err(|e| e.to_string())
}
