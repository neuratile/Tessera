//! Provider configuration IPC commands.
//!
//! Per `rules.md` §4.2.1 + §9: manages encrypted API key storage.
//! API keys are encrypted before persistence and never returned in
//! plaintext over IPC.

use serde::Deserialize;
use sqlx::SqlitePool;
use tauri::State;

use crate::services::provider_config_service::{self, ProviderConfigView};
use crate::utils::crypto::CryptoKey;

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
