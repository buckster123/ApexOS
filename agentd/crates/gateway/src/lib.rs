use axum::{
    Json, Router,
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
    routing::{get, post},
};
use futures_util::{SinkExt, StreamExt};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tower_http::services::ServeDir;
use apexos_core::{BusHandle, Event};

#[derive(Clone)]
pub struct GatewayState {
    pub bus:     BusHandle,
    pub bcast:   broadcast::Sender<Event>,
    pub api_key: Arc<RwLock<String>>,
}

pub fn router(state: GatewayState, ui_dir: PathBuf) -> Router {
    Router::new()
        .route("/ws",       get(ws_handler))
        .route("/api/status", get(status_handler))
        .route("/api/key",    post(set_key_handler))
        .fallback_service(ServeDir::new(ui_dir))
        .with_state(state)
}

// ── WebSocket ─────────────────────────────────────────────────────────────────

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<GatewayState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: GatewayState) {
    // Subscribe BEFORE spawning tasks so no events are missed.
    let mut rx = state.bcast.subscribe();
    let (mut sink, mut stream) = socket.split();

    let bus = state.bus.clone();
    let read = tokio::spawn(async move {
        while let Some(Ok(msg)) = stream.next().await {
            if let Message::Text(text) = msg {
                if let Ok(event) = serde_json::from_str::<Event>(&text) {
                    bus.emit(event).await;
                }
            }
        }
    });

    let write = tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    if let Ok(json) = serde_json::to_string(&event) {
                        if sink.send(Message::Text(json.into())).await.is_err() {
                            break;
                        }
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        }
    });

    tokio::select! {
        _ = read  => {}
        _ = write => {}
    }
}

// ── API routes ────────────────────────────────────────────────────────────────

async fn status_handler(State(state): State<GatewayState>) -> impl IntoResponse {
    let set = !state.api_key.read().await.is_empty();
    Json(serde_json::json!({ "api_key_set": set }))
}

async fn set_key_handler(
    State(state): State<GatewayState>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let key = body["key"].as_str().unwrap_or("").trim().to_string();
    if key.is_empty() {
        return Json(serde_json::json!({ "ok": false, "error": "empty key" }));
    }
    *state.api_key.write().await = key.clone();

    // Best-effort persist to a file the agentd user can write.
    let persist_path = std::env::var("AGENTD_KEY_FILE")
        .unwrap_or_else(|_| "/var/lib/agentd/.api_key".into());
    let _ = tokio::fs::write(&persist_path, &key).await;

    Json(serde_json::json!({ "ok": true }))
}

// ── serve ─────────────────────────────────────────────────────────────────────

pub async fn serve(state: GatewayState, addr: SocketAddr, ui_dir: PathBuf) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router(state, ui_dir)).await?;
    Ok(())
}
