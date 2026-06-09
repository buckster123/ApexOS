use axum::{
    Json, Router,
    extract::{
        Path, Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use futures_util::{SinkExt, StreamExt};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::{broadcast, Mutex, RwLock};
use apexos_core::{ActionId, BusHandle, Event, Message as CoreMessage, SessionId};
use apexos_plugins::{PolicyEngine, Rule};
use tokio::sync::mpsc;

/// Lightweight record of a council session, served by `GET /api/council[/:id]`.
#[derive(Clone, Serialize, Deserialize)]
pub struct CouncilRecord {
    pub id:        String,
    pub topic:     String,
    pub agents:    Vec<apexos_core::CouncilAgentDef>,
    pub status:    String,   // "running" | "complete"
    pub rounds:    u32,
    pub synthesis: String,
}

/// Map council_id → live butt-in sender. Entry removed when council completes.
pub type CouncilButtInMap  = Arc<Mutex<HashMap<String, mpsc::Sender<String>>>>;
/// Ordered list of all sessions (running + complete) for this daemon run.
pub type CouncilSessionsMap = Arc<Mutex<Vec<CouncilRecord>>>;

#[derive(Clone)]
pub struct GatewayState {
    pub bus:                   BusHandle,
    pub bcast:                 broadcast::Sender<Event>,
    /// Anthropic API key — set via env or browser UI key-entry flow
    pub api_key:               Arc<RwLock<String>>,
    /// OAI-compatible key (OpenRouter / Together / etc.) — separate from Anthropic key
    pub oai_api_key:           Arc<RwLock<String>>,
    pub model:                 Arc<RwLock<String>>,
    /// Active inference backend — live-swappable: "anthropic" | "ollama" | "vllm" | "openrouter"
    pub backend:               Arc<RwLock<String>>,
    /// Base URL for OAI-compatible backends — live-swappable
    pub oai_base_url:          Arc<RwLock<String>>,
    pub policy_mode:           Arc<RwLock<String>>,
    /// Send a mode string ("suggest" | "auto-edit" | "yolo") to live-update the PolicyEngine.
    pub policy_set_tx:         mpsc::Sender<String>,
    pub ui_dir:                PathBuf,
    pub events_dir:            PathBuf,
    pub sessions_dir:          PathBuf,
    pub histories:             Arc<Mutex<HashMap<SessionId, Vec<CoreMessage>>>>,
    pub next_session_id:       Arc<AtomicU64>,
    /// Shared secret for /sensor-bridge WS connections. Empty = no auth required.
    pub sensor_bridge_token:   Arc<String>,
    pub soul_path:             PathBuf,
    pub policy_arc:            Arc<RwLock<PolicyEngine>>,
    /// Council: start a new council session (shared with supervisor for agent-tool calls)
    pub council_start_tx:  mpsc::Sender<(SessionId, ActionId, serde_json::Value)>,
    /// Council: live butt-in senders, keyed by council_id
    pub council_butt_in:   CouncilButtInMap,
    /// Council: session records for listing/detail
    pub council_sessions:  CouncilSessionsMap,
    /// Council: counter for gateway-initiated council IDs (prefix "gw")
    pub council_next_id:   Arc<std::sync::atomic::AtomicU64>,
}

pub fn router(state: GatewayState) -> Router {
    Router::new()
        .route("/ws",              get(ws_handler))
        .route("/sensor-bridge",   get(sensor_bridge_ws_handler))
        .route("/api/status",      get(status_handler))
        .route("/api/key",      post(set_key_handler))
        .route("/api/keys",     get(get_keys_handler).post(set_keys_handler))
        .route("/api/model",    get(get_model_handler).post(set_model_handler))
        .route("/api/models",   get(get_models_handler))
        .route("/api/backend",  get(get_backend_handler).post(set_backend_handler))
        .route("/api/policy",         post(set_policy_handler))
        .route("/api/policy/rules",   get(get_policy_rules_handler))
        .route("/api/soul",     get(get_soul_handler).post(set_soul_handler))
        .route("/api/power",              post(power_handler))
        .route("/api/evolution/history",  get(evolution_history_handler))
        .route("/api/evolution/stats",    get(evolution_stats_handler))
        .route("/api/sessions",           get(sessions_handler))
        .route("/api/sessions/active",    get(active_sessions_handler))
        .route("/api/events/recent",      get(events_recent_handler))
        .route("/api/sessions/{id}/message", post(session_message_handler))
        .route("/api/run",                post(run_command_handler))
        .route("/api/snapshot",           get(snapshot_handler))
        .route("/api/sonus/files",        get(sonus_files_handler))
        .route("/api/sonus/stream",       get(sonus_stream_handler))
        .route("/api/transcribe",         post(transcribe_handler))
        .route("/api/record/start",       post(record_start_handler))
        .route("/api/record/stop",        post(record_stop_handler))
        .route("/api/wake",               post(wake_handler))
        .route("/api/speak",              post(speak_handler))
        .route("/api/council",               get(council_list_handler).post(council_start_handler))
        .route("/api/council/{id}",          get(council_detail_handler))
        .route("/api/council/{id}/butt-in",  post(council_butt_in_handler))
        .route("/terminal-ws",            get(terminal_ws_handler))
        .fallback(static_handler)
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
    let mut rx = state.bcast.subscribe();
    let (mut sink, stream) = socket.split();

    // Priority channel: read task sends session_init frames; write task forwards them
    // before anything from the broadcast. Capacity 8 is enough for the hello + one resume.
    let (prio_tx, mut prio_rx) = tokio::sync::mpsc::channel::<String>(8);

    // Assign a fresh session_id immediately — no blocking on hello.
    let session_id = state.next_session_id.fetch_add(1, Ordering::SeqCst);

    // Send initial session_init (empty history — new session) before write task starts.
    let _ = prio_tx.send(make_session_init(session_id, &[])).await;

    // Write task: drain priority channel first (biased), then relay broadcast events.
    let write = tokio::spawn(async move {
        loop {
            tokio::select! {
                biased;
                Some(msg) = prio_rx.recv() => {
                    if sink.send(Message::Text(msg.into())).await.is_err() { break; }
                }
                result = rx.recv() => match result {
                    Ok(event) => {
                        if let Ok(json) = serde_json::to_string(&event) {
                            if sink.send(Message::Text(json.into())).await.is_err() { break; }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                }
            }
        }
    });

    // Read task: handle hello frames (session resume) and relay everything else as Events.
    let bus      = state.bus.clone();
    let histories = state.histories.clone();
    let read = tokio::spawn(async move {
        let mut stream   = stream;
        let mut session_id = session_id;   // mutable — updated by hello

        while let Some(Ok(msg)) = stream.next().await {
            if let Message::Text(text) = msg {
                let val: serde_json::Value = match serde_json::from_str(&text) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                if val["type"].as_str() == Some("hello") {
                    // Client wants to resume an existing session.
                    let resume = val["resume_session"].as_u64().map(SessionId);
                    let hist = {
                        let lock = histories.lock().await;
                        match resume {
                            Some(s) if lock.contains_key(&s) => {
                                session_id = s.0;
                                lock.get(&s).cloned().unwrap_or_default()
                            }
                            _ => vec![],  // keep current session_id
                        }
                    };
                    let _ = prio_tx.send(make_session_init(session_id, &hist)).await;
                } else {
                    // Regular frame — inject WS-bound session_id and emit as Event.
                    let mut frame = val;
                    frame["session"] = serde_json::json!(session_id);
                    if let Ok(event) = serde_json::from_value::<Event>(frame) {
                        bus.emit(event).await;
                    }
                }
            }
        }
    });

    tokio::select! {
        _ = read  => {}
        _ = write => {}
    }
}

// ── Sensor bridge WS ─────────────────────────────────────────────────────────

async fn sensor_bridge_ws_handler(
    ws:              WebSocketUpgrade,
    Query(params):   Query<HashMap<String, String>>,
    State(state):    State<GatewayState>,
) -> Response {
    let expected = state.sensor_bridge_token.as_str();
    if !expected.is_empty() {
        let provided = params.get("token").map(|s| s.as_str()).unwrap_or("");
        if provided != expected {
            return (StatusCode::UNAUTHORIZED, "invalid sensor bridge token").into_response();
        }
    }
    ws.on_upgrade(move |socket| handle_sensor_bridge(socket, state))
       .into_response()
}

async fn handle_sensor_bridge(socket: WebSocket, state: GatewayState) {
    let (_, mut stream) = socket.split();
    eprintln!("[sensor-bridge] node connected");
    while let Some(Ok(msg)) = stream.next().await {
        if let Message::Text(text) = msg {
            match serde_json::from_str::<Event>(&text) {
                Ok(event) => {
                    if let Event::SensorReading { ref node_id, ref reading, .. } = event {
                        eprintln!("[sensor-bridge] {node_id}: {reading:?}");
                    }
                    state.bus.emit(event).await;
                }
                Err(e) => eprintln!("[sensor-bridge] parse error: {e} — raw: {text}"),
            }
        }
    }
    eprintln!("[sensor-bridge] node disconnected");
}

fn make_session_init(session_id: u64, history: &[CoreMessage]) -> String {
    serde_json::to_string(&serde_json::json!({
        "type":       "session_init",
        "session_id": session_id,
        "history":    history,
    }))
    .unwrap_or_default()
}

// ── Static file handler ───────────────────────────────────────────────────────

async fn static_handler(
    State(state): State<GatewayState>,
    uri: axum::http::Uri,
) -> Response {
    let path = uri.path().trim_start_matches('/');
    let file_name = match path {
        "" => "index.html",
        "mobile" => "mobile.html",
        other => other,
    };

    // Block path traversal
    if file_name.contains("..") {
        return StatusCode::NOT_FOUND.into_response();
    }

    let content_type: &'static str = if file_name.starts_with("lib/") {
        if file_name.ends_with(".js")  { "application/javascript; charset=utf-8" }
        else if file_name.ends_with(".css") { "text/css; charset=utf-8" }
        else { return StatusCode::NOT_FOUND.into_response(); }
    } else {
        match file_name {
            "index.html"        => "text/html; charset=utf-8",
            "desktop.html"      => "text/html; charset=utf-8",
            "mobile.html"       => "text/html; charset=utf-8",
            "style.css"         => "text/css; charset=utf-8",
            "desktop-style.css" => "text/css; charset=utf-8",
            "app.js"            => "application/javascript; charset=utf-8",
            "desktop-app.js"    => "application/javascript; charset=utf-8",
            "manifest.json"     => "application/manifest+json; charset=utf-8",
            _                   => return StatusCode::NOT_FOUND.into_response(),
        }
    };

    let full_path = state.ui_dir.join(file_name);
    match tokio::fs::read(&full_path).await {
        Ok(bytes) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, content_type)],
            bytes,
        ).into_response(),
        Err(e) => {
            eprintln!("[gateway] static {file_name}: {e}");
            StatusCode::NOT_FOUND.into_response()
        }
    }
}

// ── API routes ────────────────────────────────────────────────────────────────

async fn status_handler(State(state): State<GatewayState>) -> impl IntoResponse {
    let key_set     = !state.api_key.read().await.is_empty();
    let oai_key_set = !state.oai_api_key.read().await.is_empty();
    let model       = state.model.read().await.clone();
    let policy_mode = state.policy_mode.read().await.clone();
    Json(serde_json::json!({
        "api_key_set":     key_set,
        "oai_key_set":     oai_key_set,
        "model":           model,
        "policy_mode":     policy_mode,
    }))
}

async fn set_policy_handler(
    State(state): State<GatewayState>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let mode = body["mode"].as_str().unwrap_or("").trim().to_string();
    if !matches!(mode.as_str(), "suggest" | "auto-edit" | "yolo") {
        return Json(serde_json::json!({ "ok": false, "error": "unknown mode" }));
    }
    *state.policy_mode.write().await = mode.clone();
    let _ = state.policy_set_tx.send(mode).await;
    Json(serde_json::json!({ "ok": true }))
}

async fn get_soul_handler(State(state): State<GatewayState>) -> impl IntoResponse {
    match tokio::fs::read_to_string(&state.soul_path).await {
        Ok(text) => Json(serde_json::json!({ "ok": true, "content": text })),
        Err(e)   => Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
    }
}

async fn set_soul_handler(
    State(state): State<GatewayState>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let content = match body["content"].as_str() {
        Some(s) => s.to_string(),
        None    => return Json(serde_json::json!({ "ok": false, "error": "missing content" })),
    };
    match tokio::fs::write(&state.soul_path, content).await {
        Ok(_)  => Json(serde_json::json!({ "ok": true })),
        Err(e) => Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
    }
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

    let persist_path = std::env::var("AGENTD_KEY_FILE")
        .unwrap_or_else(|_| "/var/lib/agentd/.api_key".into());
    let _ = tokio::fs::write(&persist_path, &key).await;

    Json(serde_json::json!({ "ok": true }))
}

async fn get_keys_handler(State(state): State<GatewayState>) -> impl IntoResponse {
    Json(serde_json::json!({
        "anthropic_set": !state.api_key.read().await.is_empty(),
        "oai_set":       !state.oai_api_key.read().await.is_empty(),
    }))
}

async fn set_keys_handler(
    State(state): State<GatewayState>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    if let Some(key) = body["anthropic"].as_str() {
        let key = key.trim().to_string();
        if !key.is_empty() {
            *state.api_key.write().await = key.clone();
            let path = std::env::var("AGENTD_KEY_FILE")
                .unwrap_or_else(|_| "/var/lib/agentd/.api_key".into());
            let _ = tokio::fs::write(&path, &key).await;
        }
    }
    if let Some(key) = body["oai"].as_str() {
        let key = key.trim().to_string();
        if !key.is_empty() {
            *state.oai_api_key.write().await = key.clone();
            let path = std::env::var("AGENTD_OAI_KEY_FILE")
                .unwrap_or_else(|_| "/var/lib/agentd/.oai_api_key".into());
            let _ = tokio::fs::write(&path, &key).await;
        }
    }
    Json(serde_json::json!({ "ok": true }))
}

async fn get_model_handler(State(state): State<GatewayState>) -> impl IntoResponse {
    let model = state.model.read().await.clone();
    Json(serde_json::json!({ "model": model }))
}

/// Returns available models for the active backend.
/// For Anthropic: static list. For OAI backends: proxies to {base_url}/models.
async fn get_models_handler(State(state): State<GatewayState>) -> impl IntoResponse {
    let current     = state.model.read().await.clone();
    let backend     = state.backend.read().await.clone();
    let oai_base    = state.oai_base_url.read().await.clone();

    if backend == "anthropic" {
        return Json(serde_json::json!({
            "backend": backend,
            "current": current,
            "models": [
                { "id": "claude-sonnet-4-6", "name": "Sonnet 4.6" },
                { "id": "claude-opus-4-8",   "name": "Opus 4.8"   },
                { "id": "claude-opus-4-7",   "name": "Opus 4.7"   },
                { "id": "claude-haiku-4-5",  "name": "Haiku 4.5"  },
            ]
        }));
    }

    // OAI-compatible backend: query {base_url}/models for live model list
    let models_url = format!("{}/models", oai_base.trim_end_matches('/'));
    let api_key = state.oai_api_key.read().await.clone();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .unwrap_or_default();

    let mut req = client.get(&models_url);
    if !api_key.is_empty() {
        req = req.header("authorization", format!("Bearer {api_key}"));
    }

    match req.send().await {
        Ok(resp) if resp.status().is_success() => {
            if let Ok(body) = resp.json::<serde_json::Value>().await {
                let models: Vec<serde_json::Value> = body["data"]
                    .as_array()
                    .unwrap_or(&vec![])
                    .iter()
                    .filter_map(|m| m["id"].as_str())
                    .map(|id| serde_json::json!({ "id": id, "name": id }))
                    .collect();
                return Json(serde_json::json!({
                    "backend": backend,
                    "oai_base_url": oai_base,
                    "current": current,
                    "models":  models,
                }));
            }
        }
        _ => {}
    }

    // Fallback: return just the current model
    Json(serde_json::json!({
        "backend": backend,
        "oai_base_url": oai_base,
        "current": current,
        "models": [{ "id": current, "name": current }],
    }))
}

async fn get_backend_handler(State(state): State<GatewayState>) -> impl IntoResponse {
    Json(serde_json::json!({
        "backend":     state.backend.read().await.clone(),
        "oai_base_url": state.oai_base_url.read().await.clone(),
    }))
}

async fn set_backend_handler(
    State(state): State<GatewayState>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let backend = body["backend"].as_str().unwrap_or("").trim().to_lowercase();
    if backend.is_empty() {
        return Json(serde_json::json!({ "ok": false, "error": "missing backend" }));
    }
    *state.backend.write().await = backend;

    if let Some(url) = body["oai_base_url"].as_str() {
        let url = url.trim().to_string();
        if !url.is_empty() {
            *state.oai_base_url.write().await = url;
        }
    }

    // Optionally update the model when switching backends
    if let Some(model) = body["model"].as_str() {
        let model = model.trim().to_string();
        if !model.is_empty() {
            *state.model.write().await = model;
        }
    }

    Json(serde_json::json!({ "ok": true }))
}

async fn set_model_handler(
    State(state): State<GatewayState>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let model = body["model"].as_str().unwrap_or("").trim().to_string();
    if model.is_empty() {
        return Json(serde_json::json!({ "ok": false, "error": "empty model" }));
    }
    *state.model.write().await = model;
    Json(serde_json::json!({ "ok": true }))
}

async fn power_handler(
    State(_): State<GatewayState>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let action = body["action"].as_str().unwrap_or("");
    let cmd = match action {
        "reboot"   => "reboot",
        "shutdown" => "poweroff",
        _ => return Json(serde_json::json!({ "ok": false, "error": "unknown action" })),
    };
    match tokio::process::Command::new("sudo")
        .args(["systemctl", cmd])
        .output()
        .await
    {
        Ok(o) if o.status.success() => Json(serde_json::json!({ "ok": true })),
        Ok(o) => {
            let err = String::from_utf8_lossy(&o.stderr).to_string();
            eprintln!("[gateway] power/{cmd}: {err}");
            Json(serde_json::json!({ "ok": false, "error": err }))
        }
        Err(e) => Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
    }
}

async fn evolution_history_handler(State(state): State<GatewayState>) -> impl IntoResponse {
    let mut entries: Vec<serde_json::Value> = Vec::new();

    let Ok(mut dir) = tokio::fs::read_dir(&state.events_dir).await else {
        return Json(serde_json::json!([]));
    };

    // Collect matching filenames first so we can sort them.
    let mut files: Vec<String> = Vec::new();
    while let Ok(Some(entry)) = dir.next_entry().await {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with("events-") && name.ends_with(".jsonl") {
            files.push(entry.path().to_string_lossy().to_string());
        }
    }
    files.sort();

    for path in files {
        let Ok(text) = tokio::fs::read_to_string(&path).await else { continue };
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() { continue }
            let Ok(val) = serde_json::from_str::<serde_json::Value>(line) else { continue };
            if val.get("type").and_then(|t| t.as_str()) == Some("evolution_applied") {
                entries.push(val);
            }
        }
    }

    Json(serde_json::json!(entries))
}

async fn evolution_stats_handler(State(state): State<GatewayState>) -> impl IntoResponse {
    let mut applied_total:  u64 = 0;
    let mut rolledback_total: u64 = 0;
    let mut by_kind: std::collections::HashMap<String, u64> = std::collections::HashMap::new();

    let Ok(mut dir) = tokio::fs::read_dir(&state.events_dir).await else {
        return Json(serde_json::json!({
            "applied_total": 0, "rolledback_total": 0,
            "rollback_rate": 0.0, "by_kind": {}
        }));
    };

    let mut files: Vec<String> = Vec::new();
    while let Ok(Some(entry)) = dir.next_entry().await {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with("events-") && name.ends_with(".jsonl") {
            files.push(entry.path().to_string_lossy().to_string());
        }
    }
    files.sort();

    for path in files {
        let Ok(text) = tokio::fs::read_to_string(&path).await else { continue };
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() { continue }
            let Ok(val) = serde_json::from_str::<serde_json::Value>(line) else { continue };
            match val.get("type").and_then(|t| t.as_str()) {
                Some("evolution_applied") => {
                    applied_total += 1;
                    let kind = val.get("proposal")
                        .and_then(|p| p.get("kind"))
                        .and_then(|k| k.as_str())
                        .unwrap_or("unknown")
                        .to_string();
                    *by_kind.entry(kind).or_insert(0) += 1;
                }
                Some("evolution_rolled_back") => {
                    rolledback_total += 1;
                }
                _ => {}
            }
        }
    }

    let rollback_rate = if applied_total > 0 {
        (rolledback_total as f64 / applied_total as f64 * 100.0 * 10.0).round() / 10.0
    } else {
        0.0
    };

    Json(serde_json::json!({
        "applied_total":    applied_total,
        "rolledback_total": rolledback_total,
        "rollback_rate":    rollback_rate,
        "by_kind":          by_kind,
    }))
}

// ── sessions ──────────────────────────────────────────────────────────────────

/// GET /api/sessions/active — sessions currently loaded in memory (this daemon run).
/// Returns session_id + message_count so agents can choose a target for send_to_agent.
async fn active_sessions_handler(State(state): State<GatewayState>) -> impl IntoResponse {
    let histories = state.histories.lock().await;
    let mut sessions: Vec<serde_json::Value> = histories.iter()
        .map(|(sid, hist)| serde_json::json!({
            "session_id":    sid.0,
            "message_count": hist.len(),
        }))
        .collect();
    drop(histories);
    sessions.sort_by(|a, b| {
        b["session_id"].as_u64().unwrap_or(0)
            .cmp(&a["session_id"].as_u64().unwrap_or(0))
    });
    Json(serde_json::json!(sessions))
}

/// POST /api/sessions/:id/message — inject a message into an agent session from
/// external code (scripts, other services, the desktop UI). Same path as A2A:
/// emits UserPrompt on the bus so the target session starts a new turn.
async fn session_message_handler(
    State(state): State<GatewayState>,
    Path(id):     Path<u64>,
    Json(body):   Json<serde_json::Value>,
) -> impl IntoResponse {
    let message = match body["message"].as_str() {
        Some(s) if !s.trim().is_empty() => s.to_string(),
        _ => return Json(serde_json::json!({ "ok": false, "error": "missing message" })),
    };
    state.bus.emit(Event::UserPrompt { session: SessionId(id), text: message }).await;
    Json(serde_json::json!({ "ok": true, "session_id": id }))
}

async fn sessions_handler(State(state): State<GatewayState>) -> impl IntoResponse {
    use apexos_core::{ContentBlock, Message};
    use tokio::fs;

    let mut sessions = Vec::new();
    let mut rd = match fs::read_dir(&state.sessions_dir).await {
        Ok(r) => r,
        Err(_) => return Json(serde_json::json!([])),
    };

    while let Ok(Some(entry)) = rd.next_entry().await {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") { continue; }
        let id: u64 = match path.file_stem().and_then(|s| s.to_str())
            .and_then(|s| s.parse().ok()) { Some(n) => n, None => continue };

        let last_active = entry.metadata().await.ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let text = match fs::read_to_string(&path).await { Ok(t) => t, Err(_) => continue };
        let message_count = text.lines().filter(|l| !l.trim().is_empty()).count();
        if message_count == 0 { continue; }

        let preview: String = text.lines()
            .filter_map(|line| serde_json::from_str::<Message>(line).ok())
            .find_map(|msg| {
                if let Message::User { content } = msg {
                    content.into_iter().find_map(|b| {
                        if let ContentBlock::Text { text } = b { Some(text) } else { None }
                    })
                } else {
                    None
                }
            })
            .unwrap_or_default();
        let preview: String = preview.chars().take(80).collect();

        sessions.push(serde_json::json!({
            "session_id":    id,
            "last_active":   last_active,
            "message_count": message_count,
            "preview":       preview,
        }));
    }

    sessions.sort_by(|a, b| {
        let ta = a["last_active"].as_u64().unwrap_or(0);
        let tb = b["last_active"].as_u64().unwrap_or(0);
        tb.cmp(&ta)
    });

    Json(serde_json::json!(sessions))
}

// ── event log ─────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct EventsQuery {
    hours: Option<u64>,
    types: Option<String>,
    max:   Option<usize>,
}

/// GET /api/events/recent — filtered view of the JSONL event log.
/// Returns a JSON array of raw event objects. Noisy streaming events
/// (agent_text, tool_result, turn_complete) are excluded by default.
async fn events_recent_handler(
    State(state):  State<GatewayState>,
    Query(params): Query<EventsQuery>,
) -> impl IntoResponse {
    const NOISE: &[&str] = &["agent_text", "agent_thinking", "tool_result", "turn_complete"];

    let hours      = params.hours.unwrap_or(24).min(168);
    let max_events = params.max.unwrap_or(500).min(2000);
    let type_filter: Option<std::collections::HashSet<String>> =
        params.types.as_deref().map(|s| s.split(',').map(|t| t.trim().to_owned()).collect());

    let days_back = ((hours as f64) / 24.0).ceil() as i64 + 1;
    let today = chrono::Local::now().date_naive();
    let mut date_files: Vec<std::path::PathBuf> = Vec::new();
    for d in 0..days_back {
        let date = today - chrono::Duration::days(d);
        let path = state.events_dir.join(format!("events-{}.jsonl", date.format("%Y-%m-%d")));
        if tokio::fs::metadata(&path).await.is_ok() {
            date_files.push(path);
        }
    }
    date_files.reverse();

    let mut events: Vec<serde_json::Value> = Vec::new();
    for path in &date_files {
        let Ok(text) = tokio::fs::read_to_string(path).await else { continue };
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() { continue }
            let Ok(val) = serde_json::from_str::<serde_json::Value>(line) else { continue };
            let ev_type = val["type"].as_str().unwrap_or("");
            if NOISE.contains(&ev_type) { continue }
            if let Some(ref filter) = type_filter {
                if !filter.contains(ev_type) { continue }
            }
            events.push(val);
        }
    }

    if events.len() > max_events {
        let skip = events.len() - max_events;
        events.drain(0..skip);
    }

    Json(serde_json::json!(events))
}

// ── shell passthrough ─────────────────────────────────────────────────────────

async fn run_command_handler(
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let command = match body["command"].as_str() {
        Some(s) if !s.trim().is_empty() => s.trim().to_string(),
        _ => return Json(serde_json::json!({ "ok": false, "error": "missing command" })),
    };

    // Block obviously destructive patterns
    const DENY: &[&str] = &["rm -rf /", "mkfs", "dd if=/dev/zero", ":(){ :|:& };:"];
    for pat in DENY {
        if command.contains(pat) {
            return Json(serde_json::json!({ "ok": false, "error": "command denied" }));
        }
    }

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        tokio::process::Command::new("sh").arg("-c").arg(&command).output(),
    ).await;

    match result {
        Ok(Ok(o)) => Json(serde_json::json!({
            "ok":        true,
            "stdout":    String::from_utf8_lossy(&o.stdout).to_string(),
            "stderr":    String::from_utf8_lossy(&o.stderr).to_string(),
            "exit_code": o.status.code().unwrap_or(-1),
        })),
        Ok(Err(e)) => Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
        Err(_)     => Json(serde_json::json!({ "ok": false, "error": "timed out (30s)" })),
    }
}

// ── camera snapshot ───────────────────────────────────────────────────────────

async fn snapshot_handler(
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let night = params.get("night").map(|v| v == "true" || v == "1").unwrap_or(false);
    let out = "/tmp/apex_snapshot.jpg";

    let mut cmd = tokio::process::Command::new("rpicam-jpeg");
    cmd.args(["--output", out, "--timeout", "3000",
              "--width",  "1280", "--height", "720",
              "--nopreview", "--camera", "0", "-q", "85"]);
    if night {
        cmd.args(["--ev", "2", "--awb", "fluorescent", "--shutter", "100000"]);
    }

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        cmd.output(),
    ).await;

    match result {
        Ok(Ok(o)) if o.status.success() => {
            match tokio::fs::read(out).await {
                Ok(bytes) => (
                    StatusCode::OK,
                    [(header::CONTENT_TYPE, "image/jpeg")],
                    bytes,
                ).into_response(),
                Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
            }
        }
        Ok(Ok(o)) => {
            let err = String::from_utf8_lossy(&o.stderr).to_string();
            eprintln!("[snapshot] rpicam-jpeg failed: {err}");
            (StatusCode::INTERNAL_SERVER_ERROR, err).into_response()
        }
        Ok(Err(e)) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
        Err(_)     => (StatusCode::GATEWAY_TIMEOUT, "camera timeout (10s)").into_response(),
    }
}

// ── Sonus / media ────────────────────────────────────────────────────────────

fn sonus_dir() -> std::path::PathBuf {
    std::env::var("SUNO_DOWNLOAD_DIR")
        .unwrap_or_else(|_| "/var/lib/agentd/workspace/sonus".into())
        .into()
}

async fn sonus_files_handler() -> impl IntoResponse {
    const AUDIO_EXTS: &[&str] = &["mp3", "wav", "ogg", "webm", "flac", "aac", "m4a", "opus"];
    let dir = sonus_dir();
    let mut entries: Vec<serde_json::Value> = Vec::new();

    if let Ok(mut rd) = tokio::fs::read_dir(&dir).await {
        while let Ok(Some(entry)) = rd.next_entry().await {
            let name = entry.file_name().to_string_lossy().to_string();
            let ext  = name.rsplit('.').next().unwrap_or("").to_lowercase();
            if !AUDIO_EXTS.contains(&ext.as_str()) { continue; }
            let size = entry.metadata().await.map(|m| m.len()).unwrap_or(0);
            let url  = format!("/api/sonus/stream?name={}", urlencoding_simple(&name));
            entries.push(serde_json::json!({ "name": name, "size": size, "url": url }));
        }
    }

    entries.sort_by(|a, b| {
        a["name"].as_str().unwrap_or("").cmp(b["name"].as_str().unwrap_or(""))
    });

    Json(serde_json::json!(entries))
}

fn urlencoding_simple(s: &str) -> String {
    s.chars().map(|c| match c {
        'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
        ' ' => "+".to_string(),
        _ => format!("%{:02X}", c as u32),
    }).collect()
}

async fn sonus_stream_handler(
    Query(params):   Query<HashMap<String, String>>,
    req_headers:     axum::http::HeaderMap,
) -> Response {
    let name = match params.get("name").map(|s| s.trim().to_string()) {
        Some(n) if !n.is_empty() => n,
        _ => return (StatusCode::BAD_REQUEST, "missing name").into_response(),
    };
    if name.contains('/') || name.contains("..") || name.contains('\\') {
        return (StatusCode::BAD_REQUEST, "invalid name").into_response();
    }

    let ct = match name.rsplit('.').next().unwrap_or("").to_lowercase().as_str() {
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" | "opus" => "audio/ogg",
        "webm" => "audio/webm",
        "flac" => "audio/flac",
        "aac" | "m4a" => "audio/mp4",
        _ => "application/octet-stream",
    };

    let path = sonus_dir().join(&name);
    let bytes = match tokio::fs::read(&path).await {
        Ok(b) => b,
        Err(_) => return StatusCode::NOT_FOUND.into_response(),
    };
    let total = bytes.len();

    if let Some(range_hdr) = req_headers.get(header::RANGE) {
        if let Ok(range_str) = range_hdr.to_str() {
            if let Some(rest) = range_str.strip_prefix("bytes=") {
                let mut parts = rest.splitn(2, '-');
                let start = parts.next().and_then(|s| s.parse::<usize>().ok()).unwrap_or(0);
                let end   = parts.next()
                    .and_then(|s| if s.is_empty() { None } else { s.parse::<usize>().ok() })
                    .unwrap_or(total.saturating_sub(1))
                    .min(total.saturating_sub(1));
                if start < total && start <= end {
                    let body  = bytes[start..=end].to_vec();
                    let len   = body.len();
                    return axum::http::Response::builder()
                        .status(StatusCode::PARTIAL_CONTENT)
                        .header(header::CONTENT_TYPE, ct)
                        .header(header::ACCEPT_RANGES, "bytes")
                        .header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{total}"))
                        .header(header::CONTENT_LENGTH, len)
                        .body(axum::body::Body::from(body))
                        .unwrap();
                }
            }
        }
    }

    axum::http::Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, ct)
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CONTENT_LENGTH, total)
        .body(axum::body::Body::from(bytes))
        .unwrap()
}

// ── policy rules ─────────────────────────────────────────────────────────────

async fn get_policy_rules_handler(State(state): State<GatewayState>) -> impl IntoResponse {
    let engine = state.policy_arc.read().await;
    let rules: HashMap<String, &'static str> = engine.config.rules.iter()
        .map(|(k, v)| (k.clone(), match v {
            Rule::Allow     => "allow",
            Rule::Ask       => "ask",
            Rule::Workspace => "workspace",
        }))
        .collect();
    Json(serde_json::json!({ "rules": rules }))
}

// ── Wake word trigger ─────────────────────────────────────────────────────────

static WAKE_ACTIVE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

async fn wake_handler(State(state): State<GatewayState>) -> impl IntoResponse {
    // One wake sequence at a time
    if WAKE_ACTIVE.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
        return StatusCode::CONFLICT.into_response();
    }

    tokio::spawn(async move {
        // 1. Piper "yes?" — wait for it to finish so mic captures after the ding
        let model = std::env::var("PIPER_MODEL").unwrap_or_default();
        if !model.is_empty() {
            let wav = "/tmp/apex_wake_ding.wav";
            if let Ok(mut child) = tokio::process::Command::new("piper")
                .args(["--model", &model, "--output_file", wav])
                .stdin(std::process::Stdio::piped())
                .spawn()
            {
                if let Some(mut stdin) = child.stdin.take() {
                    use tokio::io::AsyncWriteExt;
                    let _ = stdin.write_all(b"yes?").await;
                }
                let _ = child.wait().await;
                let _ = tokio::process::Command::new("aplay")
                    .args(["-q", wav])
                    .output().await;
                let _ = tokio::fs::remove_file(wav).await;
            }
        }

        // 2. Signal the frontend to start recording
        let _ = state.bcast.send(apexos_core::Event::WakeTriggered);

        WAKE_ACTIVE.store(false, Ordering::SeqCst);
    });

    StatusCode::OK.into_response()
}

// ── Server-side mic recording (ALSA → whisper, no browser getUserMedia needed) ─

const SERVER_WAV: &str = "/tmp/apex_stt_server.wav";

static SERVER_RECORDER: OnceLock<tokio::sync::Mutex<Option<tokio::process::Child>>> = OnceLock::new();

fn recorder_lock() -> &'static tokio::sync::Mutex<Option<tokio::process::Child>> {
    SERVER_RECORDER.get_or_init(|| tokio::sync::Mutex::new(None))
}

async fn record_start_handler() -> impl IntoResponse {
    let device = std::env::var("ALSA_CAPTURE_DEVICE")
        .unwrap_or_else(|_| "plughw:2,0".into());

    // Kill any in-flight recording
    {
        let mut guard = recorder_lock().lock().await;
        if let Some(mut c) = guard.take() { let _ = c.kill().await; }
    }
    let _ = tokio::fs::remove_file(SERVER_WAV).await;

    match tokio::process::Command::new("arecord")
        .args(["-D", &device, "-f", "S16_LE", "-r", "16000", "-c", "1", "-d", "30", SERVER_WAV])
        .spawn()
    {
        Ok(child) => {
            *recorder_lock().lock().await = Some(child);
            StatusCode::OK.into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("arecord: {e}")).into_response(),
    }
}

async fn record_stop_handler() -> impl IntoResponse {
    // Stop the recorder
    {
        let mut guard = recorder_lock().lock().await;
        if let Some(mut c) = guard.take() { let _ = c.kill().await; }
    }
    // Small yield so arecord flushes its WAV header
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    let model = std::env::var("WHISPER_MODEL")
        .unwrap_or_else(|_| "/var/lib/agentd/whisper/ggml-tiny.en.bin".into());
    let bin = std::env::var("WHISPER_BIN")
        .unwrap_or_else(|_| "/usr/local/bin/whisper-cpp".into());

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        tokio::process::Command::new(&bin)
            .args(["-m", &model, "-f", SERVER_WAV, "-nt", "-l", "en", "--no-prints"])
            .output(),
    ).await;
    let _ = tokio::fs::remove_file(SERVER_WAV).await;

    match result {
        Ok(Ok(out)) => {
            let raw = String::from_utf8_lossy(&out.stdout);
            let text = raw.lines()
                .map(|l| l.trim())
                .filter(|l| !l.is_empty() && *l != "[BLANK_AUDIO]")
                .collect::<Vec<_>>()
                .join(" ");
            Json(serde_json::json!({ "text": text })).into_response()
        }
        Ok(Err(e)) => (StatusCode::INTERNAL_SERVER_ERROR, format!("whisper: {e}")).into_response(),
        Err(_)     => (StatusCode::GATEWAY_TIMEOUT, "whisper timed out").into_response(),
    }
}

// ── Voice: STT + TTS ─────────────────────────────────────────────────────────

async fn transcribe_handler(body: axum::body::Bytes) -> impl IntoResponse {
    if body.is_empty() {
        return (StatusCode::BAD_REQUEST, "empty audio").into_response();
    }

    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros();
    let tmp_in  = format!("/tmp/apex_stt_{stamp}.webm");
    let tmp_wav = format!("/tmp/apex_stt_{stamp}.wav");

    if let Err(e) = tokio::fs::write(&tmp_in, &body).await {
        return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
    }

    // Convert to 16kHz mono WAV
    let ff = tokio::process::Command::new("ffmpeg")
        .args(["-y", "-i", &tmp_in, "-ar", "16000", "-ac", "1", &tmp_wav])
        .output().await;
    let _ = tokio::fs::remove_file(&tmp_in).await;
    if let Err(e) = ff {
        return (StatusCode::INTERNAL_SERVER_ERROR, format!("ffmpeg: {e}")).into_response();
    }
    let ff_out = ff.unwrap();
    if !ff_out.status.success() {
        let _ = tokio::fs::remove_file(&tmp_wav).await;
        let stderr = String::from_utf8_lossy(&ff_out.stderr).to_string();
        return (StatusCode::INTERNAL_SERVER_ERROR, format!("ffmpeg failed: {stderr}")).into_response();
    }

    let model = std::env::var("WHISPER_MODEL")
        .unwrap_or_else(|_| "/var/lib/agentd/whisper/ggml-tiny.en.bin".into());
    let bin = std::env::var("WHISPER_BIN")
        .unwrap_or_else(|_| "/usr/local/bin/whisper-cpp".into());

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        tokio::process::Command::new(&bin)
            .args(["-m", &model, "-f", &tmp_wav, "-nt", "-l", "en", "--no-prints"])
            .output(),
    ).await;
    let _ = tokio::fs::remove_file(&tmp_wav).await;

    match result {
        Ok(Ok(out)) => {
            let raw = String::from_utf8_lossy(&out.stdout);
            let text = raw.lines()
                .map(|l| l.trim())
                .filter(|l| !l.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            Json(serde_json::json!({ "text": text })).into_response()
        }
        Ok(Err(e)) => (StatusCode::INTERNAL_SERVER_ERROR, format!("whisper: {e}")).into_response(),
        Err(_)     => (StatusCode::GATEWAY_TIMEOUT, "whisper timed out (30s)").into_response(),
    }
}

async fn speak_handler(Json(body): Json<serde_json::Value>) -> impl IntoResponse {
    let text = match body["text"].as_str() {
        Some(t) if !t.trim().is_empty() => t.to_string(),
        _ => return StatusCode::BAD_REQUEST.into_response(),
    };

    tokio::spawn(async move {
        if let Ok(model) = std::env::var("PIPER_MODEL") {
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_micros();
            let wav = format!("/tmp/apex_speak_{stamp}.wav");
            if let Ok(mut child) = tokio::process::Command::new("piper")
                .args(["--model", &model, "--output_file", &wav])
                .stdin(std::process::Stdio::piped())
                .spawn()
            {
                if let Some(mut stdin) = child.stdin.take() {
                    use tokio::io::AsyncWriteExt;
                    let _ = stdin.write_all(text.as_bytes()).await;
                }
                let _ = child.wait().await;
                let _ = tokio::process::Command::new("aplay")
                    .args(["-q", &wav])
                    .output().await;
                let _ = tokio::fs::remove_file(&wav).await;
            }
        } else {
            let _ = tokio::process::Command::new("espeak-ng")
                .args(["-a", "100", "-s", "150", &text])
                .output().await;
        }
    });

    StatusCode::OK.into_response()
}

// ── PTY terminal ─────────────────────────────────────────────────────────────

async fn terminal_ws_handler(ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(handle_terminal_ws)
}

unsafe fn open_pty_session() -> Option<(i32, i32, std::process::Child)> {
    use std::os::unix::io::FromRawFd;
    use std::os::unix::process::CommandExt;

    let mut master_fd: libc::c_int = -1;
    let mut slave_fd:  libc::c_int = -1;
    let ws = libc::winsize { ws_row: 24, ws_col: 80, ws_xpixel: 0, ws_ypixel: 0 };
    if libc::openpty(&mut master_fd, &mut slave_fd,
                     std::ptr::null_mut(), std::ptr::null(), &ws) != 0 {
        eprintln!("[terminal] openpty: {}", std::io::Error::last_os_error());
        return None;
    }

    let slave_out = libc::dup(slave_fd);
    let slave_err = libc::dup(slave_fd);
    if slave_out < 0 || slave_err < 0 {
        libc::close(master_fd); libc::close(slave_fd);
        if slave_out >= 0 { libc::close(slave_out); }
        return None;
    }

    let mut cmd = std::process::Command::new("/bin/bash");
    cmd.env("TERM", "xterm-256color")
       .env("HOME", std::env::var("HOME").unwrap_or_else(|_| "/root".into()))
       .stdin(std::process::Stdio::from_raw_fd(slave_fd))
       .stdout(std::process::Stdio::from_raw_fd(slave_out))
       .stderr(std::process::Stdio::from_raw_fd(slave_err));

    // post-fork pre-exec: new session + controlling terminal via fd 0 (stdin = slave)
    cmd.pre_exec(|| unsafe {
        libc::setsid();
        libc::ioctl(0, libc::TIOCSCTTY as _, 0i32);
        Ok(())
    });

    let child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => { eprintln!("[terminal] spawn: {e}"); libc::close(master_fd); return None; }
    };

    let mr = libc::dup(master_fd);
    let mw = libc::dup(master_fd);
    libc::close(master_fd);
    if mr < 0 || mw < 0 { return None; }

    Some((mr, mw, child))
}

async fn handle_terminal_ws(socket: WebSocket) {
    let (mr, mw, mut child) = match unsafe { open_pty_session() } {
        Some(t) => t,
        None    => return,
    };

    // Separate fd for resize ioctls so mw can be moved into the writer thread
    let mw_resize = unsafe { libc::dup(mw) };

    let (from_pty_tx, mut from_pty_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(64);
    let (to_pty_tx,   to_pty_rx)       = std::sync::mpsc::channel::<Vec<u8>>();

    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            let n = unsafe { libc::read(mr, buf.as_mut_ptr() as _, buf.len()) };
            if n <= 0 { break; }
            if from_pty_tx.blocking_send(buf[..n as usize].to_vec()).is_err() { break; }
        }
        unsafe { libc::close(mr); }
    });

    std::thread::spawn(move || {
        for data in to_pty_rx {
            unsafe { libc::write(mw, data.as_ptr() as _, data.len()); }
        }
        unsafe { libc::close(mw); }
    });

    let (mut sink, mut stream) = socket.split();

    let ws_write = tokio::spawn(async move {
        while let Some(data) = from_pty_rx.recv().await {
            if sink.send(Message::Binary(data.into())).await.is_err() { break; }
        }
    });

    let ws_read = tokio::spawn(async move {
        while let Some(Ok(msg)) = stream.next().await {
            match msg {
                Message::Text(text) => {
                    if text.starts_with('{') {
                        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&text) {
                            if val["type"].as_str() == Some("resize") {
                                let cols = val["cols"].as_u64().unwrap_or(80) as libc::c_ushort;
                                let rows = val["rows"].as_u64().unwrap_or(24) as libc::c_ushort;
                                unsafe {
                                    let ws = libc::winsize {
                                        ws_col: cols, ws_row: rows,
                                        ws_xpixel: 0, ws_ypixel: 0,
                                    };
                                    libc::ioctl(mw_resize, libc::TIOCSWINSZ as _, &ws);
                                }
                                continue;
                            }
                        }
                    }
                    let _ = to_pty_tx.send(text.as_bytes().to_vec());
                }
                Message::Binary(data) => { let _ = to_pty_tx.send(data.to_vec()); }
                Message::Close(_) => break,
                _ => {}
            }
        }
        unsafe { libc::close(mw_resize); }
        drop(to_pty_tx);
    });

    tokio::select! { _ = ws_write => {} _ = ws_read => {} }
    let _ = child.kill();
    eprintln!("[terminal] session closed");
}

// ── Council ───────────────────────────────────────────────────────────────────

/// POST /api/council — start a new council session from the UI.
/// Body: { topic, agents, max_rounds?, consensus_threshold? }
async fn council_start_handler(
    State(state): State<GatewayState>,
    Json(mut body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let id = format!("gw{}", state.council_next_id.fetch_add(1, std::sync::atomic::Ordering::SeqCst));
    body["council_id"] = serde_json::json!(id);
    // Use sentinel session/call so no spurious ToolResult lands on an agent turn
    let session = apexos_core::SessionId(u64::MAX);
    let call_id = apexos_core::ActionId(u64::MAX);
    if state.council_start_tx.send((session, call_id, body)).await.is_err() {
        return (StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "council handler unavailable"}))).into_response();
    }
    Json(serde_json::json!({"council_id": id})).into_response()
}

/// GET /api/council — list all council sessions (running + complete).
async fn council_list_handler(
    State(state): State<GatewayState>,
) -> impl IntoResponse {
    let sessions = state.council_sessions.lock().await;
    Json(sessions.clone()).into_response()
}

/// GET /api/council/:id — detail for a single council session.
async fn council_detail_handler(
    State(state): State<GatewayState>,
    Path(id):     Path<String>,
) -> impl IntoResponse {
    let sessions = state.council_sessions.lock().await;
    match sessions.iter().find(|r| r.id == id) {
        Some(r) => Json(r.clone()).into_response(),
        None    => (StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "council not found"}))).into_response(),
    }
}

/// POST /api/council/:id/butt-in — inject a human message into a running council.
/// Body: { message: "..." }
async fn council_butt_in_handler(
    State(state): State<GatewayState>,
    Path(id):     Path<String>,
    Json(body):   Json<serde_json::Value>,
) -> impl IntoResponse {
    let msg = body["message"].as_str().unwrap_or("").to_owned();
    if msg.is_empty() {
        return (StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "message required"}))).into_response();
    }
    let map = state.council_butt_in.lock().await;
    match map.get(&id) {
        Some(tx) => {
            if tx.send(msg).await.is_ok() {
                Json(serde_json::json!({"ok": true})).into_response()
            } else {
                (StatusCode::GONE,
                    Json(serde_json::json!({"error": "council channel closed"}))).into_response()
            }
        }
        None => (StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "council not active or not found"}))).into_response(),
    }
}

// ── serve ─────────────────────────────────────────────────────────────────────

pub async fn serve(state: GatewayState, addr: SocketAddr) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router(state)).await?;
    Ok(())
}
