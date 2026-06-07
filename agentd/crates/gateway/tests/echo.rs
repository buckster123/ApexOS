use apexos_core::{Bus, Event, SessionId, SystemState};
use apexos_gateway::{router, GatewayState};
use futures_util::{SinkExt, StreamExt};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio_tungstenite::connect_async;
use tungstenite::Message;

#[tokio::test]
async fn user_prompt_echoes_back() {
    let (bus_actor, handle, bcast) = Bus::new(SystemState::default());
    tokio::spawn(bus_actor.run());

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let state = GatewayState { bus: handle, bcast, api_key: Arc::new(tokio::sync::RwLock::new(String::new())), model: Arc::new(tokio::sync::RwLock::new("claude-opus-4-8".into())), policy_mode: "SUGGEST".into(), ui_dir: PathBuf::from("."), events_dir: PathBuf::from(".") };
    tokio::spawn(async move { axum::serve(listener, router(state)).await.unwrap() });

    let (mut ws, _) = connect_async(format!("ws://{}/ws", addr)).await.unwrap();

    // Yield so handle_socket can subscribe to broadcast before we fire events.
    tokio::time::sleep(Duration::from_millis(20)).await;

    ws.send(Message::Text(
        r#"{"type":"user_prompt","session":1,"text":"hello"}"#.into(),
    ))
    .await
    .unwrap();

    let response = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match ws.next().await.unwrap().unwrap() {
                Message::Text(json) => break json,
                _ => continue,
            }
        }
    })
    .await
    .expect("timed out waiting for echo");

    let event: Event = serde_json::from_str(&response).unwrap();
    assert!(
        matches!(event, Event::UserPrompt { session: SessionId(1), ref text } if text == "hello"),
        "unexpected event: {response}"
    );
}

#[tokio::test]
async fn multiple_clients_both_receive_broadcast() {
    let (bus_actor, handle, bcast) = Bus::new(SystemState::default());
    tokio::spawn(bus_actor.run());

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let state = GatewayState { bus: handle, bcast, api_key: Arc::new(tokio::sync::RwLock::new(String::new())), model: Arc::new(tokio::sync::RwLock::new("claude-opus-4-8".into())), policy_mode: "SUGGEST".into(), ui_dir: PathBuf::from("."), events_dir: PathBuf::from(".") };
    tokio::spawn(async move { axum::serve(listener, router(state)).await.unwrap() });

    let (mut ws1, _) = connect_async(format!("ws://{}/ws", addr)).await.unwrap();
    let (ws2, _) = connect_async(format!("ws://{}/ws", addr)).await.unwrap();

    tokio::time::sleep(Duration::from_millis(20)).await;

    // ws1 sends; both ws1 AND ws2 should receive via broadcast.
    ws1.send(Message::Text(
        r#"{"type":"user_prompt","session":2,"text":"broadcast"}"#.into(),
    ))
    .await
    .unwrap();

    let recv = |mut ws: tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>| async move {
        tokio::time::timeout(Duration::from_secs(2), async move {
            loop {
                match ws.next().await.unwrap().unwrap() {
                    Message::Text(json) => break json,
                    _ => continue,
                }
            }
        })
        .await
        .expect("timed out")
    };

    let (r1, r2) = tokio::join!(recv(ws1), recv(ws2));
    for r in [r1, r2] {
        let event: Event = serde_json::from_str(&r).unwrap();
        assert!(matches!(event, Event::UserPrompt { session: SessionId(2), .. }));
    }
}
