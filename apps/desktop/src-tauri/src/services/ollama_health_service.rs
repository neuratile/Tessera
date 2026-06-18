//! Ollama health-check service (Phase 7).
//!
//! Per `rules.md` section 4.2: this service contains the business logic for
//! detecting Ollama's installed/running state and the list of available
//! local models. It has no Tauri/IPC awareness; the `commands::ollama`
//! layer is responsible for the IPC boundary.
//!
//! Detection flow:
//! 1. Spawn `ollama --version` to determine whether the CLI exists in `PATH`.
//! 2. Probe `GET {base_url}/api/version` to determine whether the daemon is
//!    accepting HTTP requests.
//! 3. If the daemon is running, call `GET {base_url}/api/tags` to list pulled
//!    models.
//!
//! A 3-second timeout is used because the user is waiting on the first-run
//! wizard screen; a slow response is effectively "not running" from the UI's
//! perspective.

use std::io::ErrorKind;
use std::process::Command;
use std::time::Duration;

use anyhow::anyhow;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::instrument;

use crate::error::AppResult;
use crate::utils::provider_base_url::normalize_ollama_base_url;

/// Timeout used for all Ollama health-check requests.
///
/// Short by design: from the wizard's perspective a daemon that takes more
/// than 3 seconds to respond to `/api/version` is effectively not running.
const HEALTH_CHECK_TIMEOUT_SECS: u64 = 3;

/// Status of the local Ollama installation.
///
/// Mirrors the `OllamaStatusSchema` Zod schema in
/// `packages/shared/src/schemas/ollama-status.schema.ts` - field names and
/// optionality must stay in sync (rules.md section 12.3.1).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OllamaStatus {
    /// `true` when the `ollama` CLI is available in `PATH`.
    pub installed: bool,
    /// `true` when `GET /api/version` returned HTTP 200.
    pub running: bool,
    /// Names of locally available models (for example `"qwen2.5-coder:7b"`).
    pub models: Vec<String>,
}

/// Wire type for `GET /api/version`.
#[derive(Debug, Deserialize)]
struct OllamaVersionResponse {
    version: String,
}

/// Wire type for one model entry in `GET /api/tags`.
#[derive(Debug, Deserialize)]
struct OllamaModelEntry {
    name: String,
    /// On-disk size in bytes. Absent in older daemon responses, hence the
    /// default — [`check_status`] ignores it; [`list_models`] surfaces it.
    #[serde(default)]
    size: u64,
}

/// One locally-pulled model with its on-disk size, from `GET /api/tags`.
///
/// Mirrors the subset the provider wizard surfaces (name + size); the
/// digest/sha fields the daemon returns are dropped. Serializes to the
/// same shape the `list_ollama_models` IPC command returns
/// (`{ name, sizeBytes }`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OllamaModelInfo {
    pub name: String,
    pub size_bytes: u64,
}

/// Wire type for `GET /api/tags`.
#[derive(Debug, Deserialize)]
struct OllamaTagsResponse {
    models: Vec<OllamaModelEntry>,
}

/// Check the status of the Ollama daemon at `base_url`.
///
/// The function is intentionally infallible at the service level for ordinary
/// "not installed" and "not running" states: those are valid states, not
/// application errors. Only genuine transport or parse failures are returned
/// as `Err`.
///
/// # Errors
///
/// Returns an application error if the HTTP client cannot be constructed
/// (rare - platform TLS rejection) or if Ollama returns a response body that
/// cannot be parsed as the expected JSON shape.
#[instrument(skip_all, fields(base_url = %base_url))]
pub async fn check_status(base_url: &str) -> AppResult<OllamaStatus> {
    if !is_ollama_installed()? {
        return Ok(OllamaStatus {
            installed: false,
            running: false,
            models: Vec::new(),
        });
    }

    let client = build_client()?;
    let base = normalize_base_url(base_url);
    let version_url = format!("{base}/api/version");
    let version_response = client.get(&version_url).send().await;

    match version_response {
        Err(error) if is_connection_error(&error) => {
            tracing::debug!("ollama not reachable: {error}");
            Ok(OllamaStatus {
                installed: true,
                running: false,
                models: Vec::new(),
            })
        }
        Err(error) => Err(anyhow!("ollama /api/version request failed: {error}").into()),
        Ok(response) if !response.status().is_success() => {
            tracing::warn!(
                status = %response.status(),
                "ollama /api/version returned non-2xx"
            );
            Ok(OllamaStatus {
                installed: true,
                running: false,
                models: Vec::new(),
            })
        }
        Ok(response) => {
            let version_payload: OllamaVersionResponse = response
                .json()
                .await
                .map_err(|error| anyhow!("ollama /api/version response parse failed: {error}"))?;

            tracing::debug!(version = %version_payload.version, "ollama daemon responded");

            let models = fetch_model_list(&client, &base).await?;

            Ok(OllamaStatus {
                installed: true,
                running: true,
                models,
            })
        }
    }
}

/// Fetch + deserialize `GET {base}/api/tags`. Surfaces transport failures,
/// non-2xx responses, and parse failures as errors; the two callers decide
/// whether to soften them. `base` must already be normalized.
async fn fetch_tags_response(client: &Client, base: &str) -> AppResult<OllamaTagsResponse> {
    let tags_url = format!("{base}/api/tags");
    let response = client
        .get(&tags_url)
        .send()
        .await
        .map_err(|error| anyhow!("ollama /api/tags request failed: {error}"))?;

    if !response.status().is_success() {
        return Err(anyhow!("ollama responded with HTTP {}", response.status().as_u16()).into());
    }

    response
        .json::<OllamaTagsResponse>()
        .await
        .map_err(|error| anyhow!("ollama /api/tags response parse failed: {error}").into())
}

/// Fetch the list of installed model names from `GET /api/tags`.
///
/// Returns an empty vec if the request fails (transport, non-2xx, or parse)
/// so that a partial failure in model listing does not mask the fact that
/// the daemon is running — matching this function's documented contract.
async fn fetch_model_list(client: &Client, base: &str) -> AppResult<Vec<String>> {
    match fetch_tags_response(client, base).await {
        Ok(tags) => Ok(tags.models.into_iter().map(|model| model.name).collect()),
        Err(error) => {
            tracing::warn!(%error, "ollama /api/tags failed; returning empty model list");
            Ok(Vec::new())
        }
    }
}

/// List locally-pulled Ollama models (name + size) from `GET /api/tags`.
///
/// Unlike [`fetch_model_list`], a transport/non-2xx/parse failure here is
/// surfaced as an error rather than an empty list: the provider wizard uses
/// the presence of a specific model to decide whether to show an
/// `ollama pull <model>` hint, so a silently-empty list would mislead.
///
/// The base URL is normalized with [`normalize_base_url`] — the same
/// host:port canonicalization [`check_status`] already applies — so a custom
/// base URL carrying a path (e.g. `.../v1`) reaches the correct
/// `{host}/api/tags` endpoint rather than `{host}/v1/api/tags`. (The old
/// inline command only trimmed a trailing slash; this aligns the two
/// `/api/tags` callers on one canonicalization.)
///
/// # Errors
///
/// Returns an application error when the HTTP client cannot be built, the
/// daemon is unreachable or returns a non-2xx status, or the tags payload
/// cannot be parsed.
pub async fn list_models(base_url: &str) -> AppResult<Vec<OllamaModelInfo>> {
    let client = build_client()?;
    let base = normalize_base_url(base_url);
    let tags = fetch_tags_response(&client, &base).await?;

    Ok(tags
        .models
        .into_iter()
        .map(|model| OllamaModelInfo {
            name: model.name,
            size_bytes: model.size,
        })
        .collect())
}

/// Build a short-timeout HTTP client for health-check calls.
fn build_client() -> AppResult<Client> {
    Client::builder()
        .timeout(Duration::from_secs(HEALTH_CHECK_TIMEOUT_SECS))
        .build()
        .map_err(|error| anyhow!("failed to build HTTP client: {error}").into())
}

/// Strip trailing `/` so endpoint URLs never have double slashes.
fn normalize_base_url(raw: &str) -> String {
    normalize_ollama_base_url(raw)
}

/// Returns `true` when the `ollama` CLI can be spawned from `PATH`.
///
/// Any successfully spawned process counts as "installed", even if the CLI
/// exits non-zero, because the phase goal is binary detection rather than
/// validating a specific version-string format.
fn is_ollama_installed() -> AppResult<bool> {
    is_command_available("ollama")
}

/// Returns `true` when `command_name --version` can be spawned from `PATH`.
fn is_command_available(command_name: &str) -> AppResult<bool> {
    match Command::new(command_name).arg("--version").output() {
        Ok(_output) => Ok(true),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

/// Classify a `reqwest::Error` as a connection-level failure (refused,
/// DNS failure, timeout) vs. a higher-level protocol error.
fn is_connection_error(error: &reqwest::Error) -> bool {
    error.is_connect() || error.is_timeout()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Server;

    #[test]
    fn normalize_base_url_strips_trailing_slash() {
        assert_eq!(
            normalize_base_url("http://localhost:11434/"),
            "http://localhost:11434"
        );
        assert_eq!(
            normalize_base_url("http://localhost:11434"),
            "http://localhost:11434"
        );
        assert_eq!(
            normalize_base_url("http://localhost:11434/api/"),
            "http://localhost:11434"
        );
        assert_eq!(
            normalize_base_url("http://localhost:11434/v1/"),
            "http://localhost:11434"
        );
    }

    #[test]
    fn missing_command_is_not_installed() {
        let is_installed = is_command_available("testing-ide-command-that-does-not-exist")
            .expect("installation probe should not fail in tests");

        assert!(!is_installed);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn fetch_model_list_returns_models_on_success() {
        let mut server = Server::new_async().await;

        let _tags_mock = server
            .mock("GET", "/api/tags")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"models":[{"name":"qwen2.5-coder:7b"},{"name":"nomic-embed-text"}]}"#)
            .create_async()
            .await;

        let client = build_client().expect("client");
        let models = fetch_model_list(&client, &server.url())
            .await
            .expect("should succeed");

        assert!(models.contains(&"qwen2.5-coder:7b".to_string()));
        assert!(models.contains(&"nomic-embed-text".to_string()));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn fetch_model_list_returns_empty_when_tags_fails() {
        let mut server = Server::new_async().await;

        let _tags_mock = server
            .mock("GET", "/api/tags")
            .with_status(500)
            .with_body("internal error")
            .create_async()
            .await;

        let client = build_client().expect("client");
        let models = fetch_model_list(&client, &server.url())
            .await
            .expect("should succeed");

        assert!(
            models.is_empty(),
            "failed /api/tags should yield empty model list"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn list_models_returns_models_with_sizes_on_success() {
        let mut server = Server::new_async().await;

        let _tags_mock = server
            .mock("GET", "/api/tags")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"models":[{"name":"qwen2.5-coder:7b","size":4096},{"name":"nomic-embed-text"}]}"#,
            )
            .create_async()
            .await;

        let models = list_models(&server.url()).await.expect("should succeed");

        assert_eq!(models.len(), 2);
        assert_eq!(models[0].name, "qwen2.5-coder:7b");
        assert_eq!(models[0].size_bytes, 4096);
        // `size` is optional in the daemon payload and defaults to 0.
        assert_eq!(models[1].name, "nomic-embed-text");
        assert_eq!(models[1].size_bytes, 0);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn list_models_surfaces_non_2xx_as_error() {
        let mut server = Server::new_async().await;

        let _tags_mock = server
            .mock("GET", "/api/tags")
            .with_status(500)
            .with_body("internal error")
            .create_async()
            .await;

        // Unlike `fetch_model_list`, `list_models` must NOT swallow the
        // failure: the pull-hint logic depends on the error.
        let result = list_models(&server.url()).await;
        assert!(result.is_err(), "non-2xx must surface as Err");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn list_models_surfaces_unparseable_body_as_error() {
        let mut server = Server::new_async().await;

        let _tags_mock = server
            .mock("GET", "/api/tags")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body("not json")
            .create_async()
            .await;

        let result = list_models(&server.url()).await;
        assert!(result.is_err(), "unparseable tags body must surface as Err");
    }
}
