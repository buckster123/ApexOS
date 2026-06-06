use apexos_core::{Bus, SystemState};
use apexos_gateway::{serve, GatewayState};
use apexos_plugins::{load as load_plugins, Supervisor};
use std::path::PathBuf;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let (bus, handle, bcast) = Bus::new(SystemState::default());
    tokio::spawn(bus.run());

    // Gateway
    let gw_state = GatewayState { bus: handle.clone(), bcast: bcast.clone() };
    let gw_addr: std::net::SocketAddr = "0.0.0.0:8787".parse()?;
    tokio::spawn(async move {
        if let Err(e) = serve(gw_state, gw_addr).await {
            eprintln!("[gateway] error: {e}");
        }
    });

    // Plugin supervisor
    let config_path = PathBuf::from(
        std::env::var("AGENTD_PLUGINS_TOML")
            .unwrap_or_else(|_| "config/plugins.toml".into())
    );

    let plugin_configs = match load_plugins(&config_path) {
        Ok(c)  => { eprintln!("[agentd] loaded {} plugin(s)", c.len()); c }
        Err(e) => { eprintln!("[agentd] plugins config: {e}"); vec![] }
    };

    let supervisor = Supervisor::new(handle.clone());
    tokio::spawn(supervisor.run(plugin_configs, bcast.subscribe()));

    eprintln!("[agentd] ready — gateway ws://0.0.0.0:8787/ws");
    tokio::signal::ctrl_c().await?;
    eprintln!("[agentd] shutting down");
    Ok(())
}
