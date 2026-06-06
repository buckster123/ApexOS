use axum::{
    Router,
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
    routing::get,
};
use futures_util::{SinkExt, StreamExt};
use std::net::SocketAddr;
use tokio::sync::broadcast;
use apexos_core::{BusHandle, Event};

#[derive(Clone)]
pub struct GatewayState {
    pub bus:   BusHandle,
    pub bcast: broadcast::Sender<Event>,
}

pub fn router(state: GatewayState) -> Router {
    Router::new()
        .route("/ws", get(ws_handler))
        .with_state(state)
}

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

pub async fn serve(state: GatewayState, addr: SocketAddr) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router(state)).await?;
    Ok(())
}
