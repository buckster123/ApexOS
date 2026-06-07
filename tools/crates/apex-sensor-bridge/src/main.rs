//! apex-sensor-bridge — forwards sensor readings to the ApexOS gateway
//!
//! Connects to ws://{SENSOR_BRIDGE_HOST}/sensor-bridge?token={SENSOR_BRIDGE_TOKEN}
//! and pushes SensorReading events on a configurable interval.
//!
//! Env vars:
//!   SENSOR_BRIDGE_HOST   (default: localhost:8787)
//!   SENSOR_BRIDGE_TOKEN  (default: empty = no auth required)
//!   SENSOR_NODE_ID       (default: hostname)
//!   SENSOR_INTERVAL_SECS (default: 30)

use serde_json::json;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tungstenite::{connect, Message};

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

fn hostname() -> String {
    std::fs::read_to_string("/etc/hostname")
        .unwrap_or_else(|_| "unknown".into())
        .trim()
        .to_string()
}

// ── CPU temperature from sysfs ────────────────────────────────────────────────

fn read_cpu_temp() -> Option<f32> {
    // Try Pi-specific thermal zone 0 first, then scan all zones.
    let candidates = [
        "/sys/class/thermal/thermal_zone0/temp".to_string(),
    ];
    for path in &candidates {
        if let Ok(raw) = std::fs::read_to_string(path) {
            if let Ok(millideg) = raw.trim().parse::<i64>() {
                return Some(millideg as f32 / 1000.0);
            }
        }
    }
    // Fallback: scan all zones
    for i in 0..8 {
        let path = format!("/sys/class/thermal/thermal_zone{i}/temp");
        if let Ok(raw) = std::fs::read_to_string(&path) {
            if let Ok(millideg) = raw.trim().parse::<i64>() {
                return Some(millideg as f32 / 1000.0);
            }
        }
    }
    None
}

// ── Build a SensorReading event JSON frame ────────────────────────────────────

fn make_sensor_event(node_id: &str, kind: &str, payload: serde_json::Value) -> String {
    json!({
        "type":      "sensor_reading",
        "node_id":   node_id,
        "reading":   payload,
        "timestamp": now_secs(),
        "kind":      kind,   // top-level kind redundancy for easy log scanning
    })
    .to_string()
}

// ── Main connect + send loop ──────────────────────────────────────────────────

fn run(url: &str, node_id: &str, interval: Duration) {
    loop {
        eprintln!("[apex-sensor-bridge] connecting to {url}");
        match connect(url) {
            Err(e) => {
                eprintln!("[apex-sensor-bridge] connect failed: {e} — retry in 10s");
                std::thread::sleep(Duration::from_secs(10));
                continue;
            }
            Ok((mut ws, _)) => {
                eprintln!("[apex-sensor-bridge] connected");
                loop {
                    // ── CPU temperature ───────────────────────────────────────
                    if let Some(celsius) = read_cpu_temp() {
                        let frame = make_sensor_event(
                            node_id,
                            "temperature",
                            json!({
                                "kind":      "temperature",
                                "celsius":   celsius,
                                "sensor_id": "cpu_thermal"
                            }),
                        );
                        if let Err(e) = ws.send(Message::Text(frame.into())) {
                            eprintln!("[apex-sensor-bridge] send error: {e} — reconnecting");
                            break;
                        }
                    }

                    // ── Ping to keep connection alive ─────────────────────────
                    if let Err(e) = ws.send(Message::Ping(vec![].into())) {
                        eprintln!("[apex-sensor-bridge] ping error: {e} — reconnecting");
                        break;
                    }

                    std::thread::sleep(interval);
                }
                let _ = ws.close(None);
            }
        }
        // Brief pause before reconnect attempt
        std::thread::sleep(Duration::from_secs(5));
    }
}

fn main() {
    let host    = std::env::var("SENSOR_BRIDGE_HOST").unwrap_or_else(|_| "localhost:8787".into());
    let token   = std::env::var("SENSOR_BRIDGE_TOKEN").unwrap_or_default();
    let node_id = std::env::var("SENSOR_NODE_ID").unwrap_or_else(|_| hostname());
    let interval_secs = std::env::var("SENSOR_INTERVAL_SECS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(30);

    let url = if token.is_empty() {
        format!("ws://{host}/sensor-bridge")
    } else {
        format!("ws://{host}/sensor-bridge?token={token}")
    };

    eprintln!("[apex-sensor-bridge] node_id={node_id} interval={interval_secs}s");
    run(&url, &node_id, Duration::from_secs(interval_secs));
}
