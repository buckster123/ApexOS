use axum::{
    Json, Router,
    extract::{
        Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use futures_util::{SinkExt, StreamExt};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::{broadcast, Mutex, RwLock};
use apexos_core::{BusHandle, Event, Message as CoreMessage, SessionId};
use apexos_plugins::{PolicyEngine, Rule};
use tokio::sync::mpsc;

#[derive(Clone)]
pub struct GatewayState {
    pub bus:                   BusHandle,
    pub bcast:                 broadcast::Sender<Event>,
    pub api_key:               Arc<RwLock<String>>,
    pub model:                 Arc<RwLock<String>>,
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
}

pub fn router(state: GatewayState) -> Router {
    Router::new()
        .route("/ws",              get(ws_handler))
        .route("/sensor-bridge",   get(sensor_bridge_ws_handler))
        .route("/api/status",      get(status_handler))
        .route("/api/key",      post(set_key_handler))
        .route("/api/model",    get(get_model_handler).post(set_model_handler))
        .route("/api/policy",         post(set_policy_handler))
        .route("/api/policy/rules",   get(get_policy_rules_handler))
        .route("/api/soul",     get(get_soul_handler).post(set_soul_handler))
        .route("/api/power",              post(power_handler))
        .route("/api/evolution/history",  get(evolution_history_handler))
        .route("/api/evolution/stats",    get(evolution_stats_handler))
        .route("/api/sessions",           get(sessions_handler))
        .route("/api/run",                post(run_command_handler))
        .route("/api/snapshot",           get(snapshot_handler))
        .route("/api/sonus/files",        get(sonus_files_handler))
        .route("/api/sonus/stream",       get(sonus_stream_handler))
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
    let file_name = if path.is_empty() { "index.html" } else { path };

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
            "style.css"         => "text/css; charset=utf-8",
            "desktop-style.css" => "text/css; charset=utf-8",
            "app.js"            => "application/javascript; charset=utf-8",
            "desktop-app.js"    => "application/javascript; charset=utf-8",
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
    let model       = state.model.read().await.clone();
    let policy_mode = state.policy_mode.read().await.clone();
    Json(serde_json::json!({
        "api_key_set":  key_set,
        "model":        model,
        "policy_mode":  policy_mode,
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

async fn get_model_handler(State(state): State<GatewayState>) -> impl IntoResponse {
    let model = state.model.read().await.clone();
    Json(serde_json::json!({ "model": model }))
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

// ── serve ─────────────────────────────────────────────────────────────────────

pub async fn serve(state: GatewayState, addr: SocketAddr) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router(state)).await?;
    Ok(())
}
