use apexos_core::{Bus, ContentBlock, Event, Message, PluginId, SessionId, SystemState, ToolSpec};
use apexos_gateway::{serve, GatewayState};
use apexos_plugins::{load as load_plugins, PolicyConfig, PolicyEngine, Supervisor};
use apexos_agent::{AnthropicProvider, TurnEngine, run_turn};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex, RwLock};

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

    // Policy engine
    let policy_path = PathBuf::from(
        std::env::var("AGENTD_POLICY_TOML")
            .unwrap_or_else(|_| "config/policy.toml".into())
    );
    let policy_config = match PolicyConfig::load(&policy_path) {
        Ok(c)  => { eprintln!("[agentd] policy mode: {:?}", c.mode); c }
        Err(e) => { eprintln!("[agentd] policy config: {e} — using defaults"); PolicyConfig::default() }
    };
    let policy = PolicyEngine::new(policy_config);

    let supervisor = Supervisor::new(handle.clone(), policy);
    tokio::spawn(supervisor.run(plugin_configs, bcast.subscribe()));

    // Agent turn engine
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .unwrap_or_else(|_| {
            eprintln!("[agentd] ANTHROPIC_API_KEY not set — agent turns will fail");
            String::new()
        });

    let engine: Arc<TurnEngine> = Arc::new(TurnEngine::new(
        AnthropicProvider::new(api_key, "claude-opus-4-8"),
        16,
        None,
    ));

    // Shared tool registry: plugin id → tools it advertises.
    let tool_reg: Arc<RwLock<HashMap<PluginId, Vec<ToolSpec>>>> =
        Arc::new(RwLock::new(HashMap::new()));

    // Per-session conversation history (the agent's working memory).
    let histories: Arc<Mutex<HashMap<SessionId, Vec<Message>>>> =
        Arc::new(Mutex::new(HashMap::new()));

    // Subscribe before spawning so we don't miss early PluginUp events.
    let agent_rx = bcast.subscribe();

    spawn_agent_listener(agent_rx, bcast.clone(), handle.clone(), tool_reg, histories, engine);

    eprintln!("[agentd] ready — gateway ws://0.0.0.0:8787/ws");
    tokio::signal::ctrl_c().await?;
    eprintln!("[agentd] shutting down");
    Ok(())
}

fn spawn_agent_listener(
    mut rx:       broadcast::Receiver<Event>,
    bcast:        broadcast::Sender<Event>,
    bus:          apexos_core::BusHandle,
    tool_reg:     Arc<RwLock<HashMap<PluginId, Vec<ToolSpec>>>>,
    histories:    Arc<Mutex<HashMap<SessionId, Vec<Message>>>>,
    engine:       Arc<TurnEngine>,
) {
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(Event::UserPrompt { session, text }) => {
                    // Append user message to local history.
                    let mut hist = histories.lock().await;
                    let history = hist.entry(session).or_default();
                    history.push(Message::User {
                        content: vec![ContentBlock::Text { text }],
                    });
                    let snapshot = history.clone();
                    drop(hist);

                    let tools: Vec<ToolSpec> = tool_reg.read().await
                        .values()
                        .flatten()
                        .cloned()
                        .collect();

                    let bus     = bus.clone();
                    let bcast   = bcast.clone();
                    let engine  = engine.clone();
                    let hist_ref = histories.clone();

                    tokio::spawn(async move {
                        match run_turn(session, snapshot, bus, bcast, tools, engine).await {
                            Ok(updated) => {
                                hist_ref.lock().await.insert(session, updated);
                            }
                            Err(e) => {
                                eprintln!("[agent:{:?}] turn error: {e}", session);
                            }
                        }
                    });
                }

                Ok(Event::PluginUp { plugin, tools }) => {
                    tool_reg.write().await.insert(plugin, tools);
                }
                Ok(Event::PluginDown { plugin, .. }) => {
                    tool_reg.write().await.remove(&plugin);
                }

                Ok(_) => {}
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        }
    });
}
