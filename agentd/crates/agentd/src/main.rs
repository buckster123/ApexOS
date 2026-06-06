use apexos_core::{Bus, SystemState};
use apexos_gateway::{serve, GatewayState};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let (bus, handle, bcast) = Bus::new(SystemState::default());
    tokio::spawn(bus.run());

    let state = GatewayState { bus: handle, bcast };
    let addr: std::net::SocketAddr = "0.0.0.0:8787".parse()?;

    eprintln!("agentd listening on {addr}");
    serve(state, addr).await
}
