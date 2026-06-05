//! WebSocket connection and upgrade route.

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use futures::{SinkExt, StreamExt};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use uuid::Uuid;
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};

use crate::middleware::auth::Claims;
use crate::AppState;

/// How long a freshly upgraded socket has to present its auth message.
const AUTH_TIMEOUT: Duration = Duration::from_secs(10);

/// First message the client must send after the upgrade.
/// The token is never placed in the URL, so it cannot leak via access logs,
/// browser history, or Referer headers.
#[derive(serde::Deserialize)]
struct AuthMessage {
    r#type: String,
    token: String,
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    Path(board_id): Path<Uuid>,
    State(state): State<Arc<AppState>>,
) -> Response {
    // Authentication happens over the socket itself (first-message auth);
    // browsers cannot set an Authorization header on WebSocket upgrades.
    ws.on_upgrade(move |socket| handle_socket(socket, board_id, state))
}

/// Waits for the first-message auth frame, validates the JWT, and verifies the
/// user belongs to the board's team. Returns `None` on any failure.
async fn authenticate(
    socket: &mut WebSocket,
    board_id: Uuid,
    state: &Arc<AppState>,
) -> Option<Uuid> {
    let msg = tokio::time::timeout(AUTH_TIMEOUT, socket.recv())
        .await
        .ok()?? // outer: timeout elapsed; inner: socket closed
        .ok()?;

    let Message::Text(text) = msg else {
        return None;
    };

    let auth: AuthMessage = serde_json::from_str(&text).ok()?;
    if auth.r#type != "auth" {
        return None;
    }

    let mut validation = Validation::new(Algorithm::HS256);
    validation.leeway = 30;

    let claims = decode::<Claims>(
        &auth.token,
        &DecodingKey::from_secret(state.config.jwt_secret.as_bytes()),
        &validation,
    )
    .ok()?;

    // Refresh tokens must not grant realtime access — mirror the HTTP
    // AuthUser extractor's token-kind check.
    if claims.claims.kind.as_deref() == Some("refresh") {
        return None;
    }

    let user_id = Uuid::parse_str(&claims.claims.sub).ok()?;

    // Verify membership in the board's team.
    let board_row = sqlx::query("SELECT team_id FROM boards WHERE id = $1")
        .bind(board_id)
        .fetch_optional(&state.db)
        .await
        .ok()??;

    use sqlx::Row;
    let board_team_id: Uuid = board_row.get("team_id");

    crate::services::team_service::check_membership(&state.db, user_id, board_team_id)
        .await
        .ok()?;

    Some(user_id)
}

async fn handle_socket(mut socket: WebSocket, board_id: Uuid, state: Arc<AppState>) {
    let Some(user_id) = authenticate(&mut socket, board_id, &state).await else {
        tracing::debug!("websocket auth failed for board {}", board_id);
        let _ = socket.send(Message::Close(None)).await;
        return;
    };

    // Ack so the client knows it can start consuming events.
    if socket
        .send(Message::Text(r#"{"type":"auth_ok"}"#.into()))
        .await
        .is_err()
    {
        return;
    }

    let (mut sender, mut receiver) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<Message>();

    // Register our channel with the ws_hub room
    state.ws_hub.join(board_id, tx.clone());

    // Task to forward messages from the channel to the WebSocket client
    let mut send_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if sender.send(msg).await.is_err() {
                break;
            }
        }
    });

    // Task to receive messages from the WebSocket client
    let ws_hub_clone = state.ws_hub.clone();
    let tx_clone = tx.clone();
    let mut recv_task = tokio::spawn(async move {
        while let Some(msg_res) = receiver.next().await {
            match msg_res {
                Ok(msg) => {
                    if let Message::Close(_) = msg {
                        break;
                    }
                    // We discard other inbound client messages (they use REST API for operations)
                }
                Err(_) => break,
            }
        }
    });

    // Wait until either task terminates (e.g. client disconnects or network drops)
    tokio::select! {
        _ = &mut send_task => {
            tracing::debug!("sender task closed for user {} on board {}", user_id, board_id);
        }
        _ = &mut recv_task => {
            tracing::debug!("receiver task closed for user {} on board {}", user_id, board_id);
        }
    }

    // Clean up connection from the hub
    ws_hub_clone.leave(board_id, &tx_clone);
}

/// Mount WebSocket route.
pub fn router() -> Router<Arc<AppState>> {
    Router::new().route("/boards/{board_id}", get(ws_handler))
}
