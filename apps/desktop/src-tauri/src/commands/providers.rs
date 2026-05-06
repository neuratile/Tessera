//! Provider configuration IPC commands.
//!
//! Per `rules.md` §4.2.1 + §9: manages encrypted API key storage.
//! API keys are encrypted before persistence and never returned in
//! plaintext over IPC.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tauri::State;

use crate::config::AppConfig;
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
) -> Result<Vec<ProviderConfigView>, String> {
    provider_config_service::list_configs(&pool)
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

/// Listed model from the Ollama daemon. Mirrors the subset of
/// `/api/tags` we surface to the UI (we drop the digest / sha256
/// fields the daemon returns — UI only needs name + size for the
/// wizard's "do I have this model pulled?" check).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OllamaModel {
    pub name: String,
    pub size_bytes: u64,
}

#[derive(Debug, Deserialize)]
struct OllamaTagsResponse {
    models: Vec<OllamaTagsEntry>,
}

#[derive(Debug, Deserialize)]
struct OllamaTagsEntry {
    name: String,
    #[serde(default)]
    size: u64,
}

/// List the locally pulled Ollama models. Returns an error when the
/// daemon is unreachable; the UI uses presence/absence of the chosen
/// model to decide whether to surface an `ollama pull <model>` hint.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub async fn list_ollama_models(base_url: Option<String>) -> Result<Vec<OllamaModel>, String> {
    let base = base_url.as_deref().unwrap_or("http://localhost:11434");
    let url = format!("{}/api/tags", base.trim_end_matches('/'));

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .map_err(|_| "could not build HTTP client".to_string())?;

    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|_| "Ollama unreachable".to_string())?;

    if !resp.status().is_success() {
        return Err(format!(
            "Ollama responded with HTTP {}",
            resp.status().as_u16()
        ));
    }

    let body: OllamaTagsResponse = resp
        .json()
        .await
        .map_err(|_| "Ollama returned an unparseable tags payload".to_string())?;

    Ok(body
        .models
        .into_iter()
        .map(|e| OllamaModel {
            name: e.name,
            size_bytes: e.size,
        })
        .collect())
}
