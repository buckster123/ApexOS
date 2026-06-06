use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::sync::{broadcast, mpsc};
use tokio::process::Command;
use std::process::Stdio;
use apexos_core::{BusHandle, Event, PluginId, ToolOutput};
use crate::config::{PluginConfig, RestartPolicy};
use crate::mcp::McpClient;

struct Plugin {
    client: Arc<McpClient>,
}

enum SupervisorCmd {
    PluginDied { id: PluginId },
}

pub struct Supervisor {
    bus:           BusHandle,
    plugins:       HashMap<PluginId, Plugin>,
    tool_registry: HashMap<String, PluginId>,
    configs:       HashMap<PluginId, PluginConfig>,
}

impl Supervisor {
    pub fn new(bus: BusHandle) -> Self {
        Self {
            bus,
            plugins:       HashMap::new(),
            tool_registry: HashMap::new(),
            configs:       HashMap::new(),
        }
    }

    /// Boot all plugins from config then run the dispatch/supervision loop.
    pub async fn run(
        mut self,
        plugin_configs: Vec<PluginConfig>,
        mut bus_rx: broadcast::Receiver<Event>,
    ) {
        let (sv_tx, mut sv_rx) = mpsc::channel::<SupervisorCmd>(64);

        for cfg in plugin_configs {
            if let Err(e) = self.spawn_plugin(&cfg, sv_tx.clone()).await {
                eprintln!("[supervisor] failed to start plugin '{}': {e}", cfg.id);
            }
        }

        loop {
            tokio::select! {
                result = bus_rx.recv() => match result {
                    Ok(Event::ToolRequested { session, call }) => {
                        if let Some(pid) = self.tool_registry.get(&call.tool).cloned() {
                            if let Some(plugin) = self.plugins.get(&pid) {
                                let client = plugin.client.clone();
                                let bus    = self.bus.clone();
                                tokio::spawn(async move {
                                    let output = match client.call_tool(&call.tool, &call.args).await {
                                        Ok(o)  => o,
                                        Err(e) => ToolOutput {
                                            ok:      false,
                                            content: serde_json::json!(e.to_string()),
                                        },
                                    };
                                    bus.emit(Event::ToolResult { session, call: call.id, output }).await;
                                });
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                },

                Some(cmd) = sv_rx.recv() => match cmd {
                    SupervisorCmd::PluginDied { id } => {
                        self.handle_died(id, sv_tx.clone()).await;
                    }
                },
            }
        }
    }

    async fn spawn_plugin(
        &mut self,
        cfg: &PluginConfig,
        sv_tx: mpsc::Sender<SupervisorCmd>,
    ) -> anyhow::Result<()> {
        let plugin_id = PluginId(cfg.id.clone());

        let mut cmd = Command::new(&cfg.cmd);
        cmd.args(&cfg.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        if let Some(cwd) = &cfg.cwd {
            cmd.current_dir(cwd);
        }
        if let Some(env) = &cfg.env {
            for (k, v) in env {
                cmd.env(k, v);
            }
        }

        let mut child = cmd.spawn()?;

        // Drain stderr to prevent the child's buffer from blocking.
        if let Some(stderr) = child.stderr.take() {
            let id = plugin_id.clone();
            tokio::spawn(async move {
                use tokio::io::AsyncBufReadExt;
                let mut lines = tokio::io::BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    eprintln!("[plugin:{id}] {line}");
                }
            });
        }

        let client = Arc::new(McpClient::attach(&mut child).await?);
        client.initialize().await?;
        let tools = client.list_tools().await?;

        for spec in &tools {
            self.tool_registry.insert(spec.name.clone(), plugin_id.clone());
        }

        eprintln!("[supervisor] plugin '{}' up — {} tools", plugin_id.0, tools.len());
        self.bus.emit(Event::PluginUp { plugin: plugin_id.clone(), tools }).await;

        // Watch child for exit; send SupervisorCmd when it dies.
        let id_w = plugin_id.clone();
        tokio::spawn(async move {
            let _ = child.wait().await;
            let _ = sv_tx.send(SupervisorCmd::PluginDied { id: id_w }).await;
        });

        self.configs.insert(plugin_id.clone(), cfg.clone());
        self.plugins.insert(plugin_id, Plugin { client });
        Ok(())
    }

    async fn handle_died(&mut self, id: PluginId, sv_tx: mpsc::Sender<SupervisorCmd>) {
        self.tool_registry.retain(|_, owner| owner != &id);
        self.plugins.remove(&id);

        eprintln!("[supervisor] plugin '{}' died", id.0);
        self.bus.emit(Event::PluginDown {
            plugin: id.clone(),
            reason: "process exited".into(),
        }).await;

        let cfg = match self.configs.get(&id) {
            Some(c) => c.clone(),
            None    => return,
        };

        if cfg.restart == RestartPolicy::Always {
            eprintln!("[supervisor] restarting '{}' in 1s…", id.0);
            tokio::time::sleep(Duration::from_secs(1)).await;
            if let Err(e) = self.spawn_plugin(&cfg, sv_tx).await {
                eprintln!("[supervisor] restart of '{}' failed: {e}", id.0);
            }
        }
    }
}
