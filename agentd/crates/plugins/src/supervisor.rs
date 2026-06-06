use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::sync::{broadcast, mpsc};
use tokio::process::Command;
use std::process::Stdio;
use apexos_core::{ActionId, BusHandle, Event, PluginId, SessionId, ToolCall, ToolOutput};
use crate::config::{PluginConfig, RestartPolicy};
use crate::mcp::McpClient;
use crate::policy::{Decision, PolicyEngine};

struct Plugin {
    client: Arc<McpClient>,
}

struct PendingApproval {
    session: SessionId,
    call:    ToolCall,
}

enum SupervisorCmd {
    PluginDied { id: PluginId },
}

pub struct Supervisor {
    bus:               BusHandle,
    plugins:           HashMap<PluginId, Plugin>,
    tool_registry:     HashMap<String, PluginId>,
    configs:           HashMap<PluginId, PluginConfig>,
    policy:            PolicyEngine,
    pending_approvals: HashMap<ActionId, PendingApproval>,
}

impl Supervisor {
    pub fn new(bus: BusHandle, policy: PolicyEngine) -> Self {
        Self {
            bus,
            plugins:           HashMap::new(),
            tool_registry:     HashMap::new(),
            configs:           HashMap::new(),
            policy,
            pending_approvals: HashMap::new(),
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
                        match self.policy.check(&call.tool) {
                            Decision::Allow => {
                                self.dispatch_tool(session, call);
                            }
                            Decision::Ask => {
                                eprintln!("[policy] approval required: '{}' (session {:?})",
                                    call.tool, session);
                                self.pending_approvals.insert(
                                    call.id,
                                    PendingApproval { session, call: call.clone() },
                                );
                                self.bus.emit(Event::ApprovalPending { session, call }).await;
                            }
                        }
                    }

                    Ok(Event::UserApproval { session, action, granted }) => {
                        if let Some(pending) = self.pending_approvals.remove(&action) {
                            if granted {
                                eprintln!("[policy] approved: '{}' (session {:?})",
                                    pending.call.tool, pending.session);
                                self.dispatch_tool(pending.session, pending.call);
                            } else {
                                eprintln!("[policy] denied: '{}' (session {:?})",
                                    pending.call.tool, pending.session);
                                let bus = self.bus.clone();
                                tokio::spawn(async move {
                                    bus.emit(Event::ToolResult {
                                        session,
                                        call: action,
                                        output: ToolOutput {
                                            ok:      false,
                                            content: serde_json::json!("denied by user"),
                                        },
                                    }).await;
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

    /// Dispatch a tool call immediately (policy already checked).
    fn dispatch_tool(&self, session: SessionId, call: ToolCall) {
        // Virtual tool: agent_spawn is handled by the async router, not an MCP plugin.
        if call.tool == "agent_spawn" {
            let prompt  = call.args["prompt"].as_str().unwrap_or("").to_owned();
            let system  = call.args["system"].as_str().map(str::to_owned);
            let bus     = self.bus.clone();
            let call_id = call.id;
            tokio::spawn(async move {
                bus.emit(Event::SpawnAgent { parent: session, call_id, prompt, system }).await;
            });
            return;
        }

        let tool_name = call.tool.clone();
        if let Some(pid) = self.tool_registry.get(&tool_name).cloned() {
            if let Some(plugin) = self.plugins.get(&pid) {
                let client   = plugin.client.clone();
                let bus      = self.bus.clone();
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
                return;
            }
        }
        // Unknown tool or plugin not live → return error so the turn loop unblocks.
        let bus     = self.bus.clone();
        let call_id = call.id;
        tokio::spawn(async move {
            bus.emit(Event::ToolResult {
                session,
                call: call_id,
                output: ToolOutput {
                    ok:      false,
                    content: serde_json::json!(format!("unknown tool: {tool_name}")),
                },
            }).await;
        });
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
