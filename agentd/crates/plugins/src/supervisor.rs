use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::sync::{broadcast, mpsc, oneshot, RwLock};
use tokio::process::Command;
use std::process::Stdio;
use apexos_core::{ActionId, BusHandle, Event, EvolutionId, EvolutionProposal, PluginId, SessionId, ToolCall, ToolOutput};
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

pub enum SupervisorCmd {
    /// Internal: process watcher detected the child exited.
    PluginDied  { id: PluginId },
    /// Start a brand-new plugin (appended to plugins.toml by evolution applier).
    SpawnPlugin { config: PluginConfig },
    /// Kill a plugin and remove it from the registry; does NOT restart.
    KillPlugin  { id: PluginId },
    /// Kill a plugin and restart it (in-place upgrade / config change).
    HotReload   { id: PluginId },
    /// Direct tool call bypassing policy — reply arrives on the oneshot sender.
    DirectCall  { tool: String, args: serde_json::Value, reply: oneshot::Sender<ToolOutput> },
    /// Wire the live soul.md Arc so read_soul_md returns current content.
    SetSoulArc  { arc: Arc<RwLock<String>> },
    /// Wire the scheduler op channel so schedule_* tools route to the scheduler task.
    SetScheduleTx { tx: mpsc::Sender<(SessionId, ActionId, String, serde_json::Value)> },
    /// Wire the council op channel so convene_council routes to the council handler.
    SetCouncilTx  { tx: mpsc::Sender<(SessionId, ActionId, serde_json::Value)> },
}

/// Thin handle for calling plugin tools directly from non-agent code (e.g. the
/// evolution applier calling Cerebro for episode tracking).
#[derive(Clone)]
pub struct ToolProxy {
    tx: mpsc::Sender<SupervisorCmd>,
}

impl ToolProxy {
    pub fn new(tx: mpsc::Sender<SupervisorCmd>) -> Self { Self { tx } }

    pub async fn call(&self, tool: &str, args: serde_json::Value) -> anyhow::Result<ToolOutput> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx.send(SupervisorCmd::DirectCall {
            tool:  tool.to_string(),
            args,
            reply: reply_tx,
        }).await.map_err(|_| anyhow::anyhow!("supervisor channel closed"))?;
        tokio::time::timeout(Duration::from_secs(10), reply_rx).await
            .map_err(|_| anyhow::anyhow!("direct call timed out: {tool}"))?
            .map_err(|_| anyhow::anyhow!("reply dropped"))
    }
}

pub struct Supervisor {
    bus:               BusHandle,
    plugins:           HashMap<PluginId, Plugin>,
    tool_registry:     HashMap<String, PluginId>,
    configs:           HashMap<PluginId, PluginConfig>,
    /// Shared with the evolution applier; writing here updates policy live.
    policy:            Arc<RwLock<PolicyEngine>>,
    pending_approvals: HashMap<ActionId, PendingApproval>,
    sv_tx:             mpsc::Sender<SupervisorCmd>,
    sv_rx:             Option<mpsc::Receiver<SupervisorCmd>>,
    /// Set by main.rs so rollback_evolution can route to the applier task.
    rollback_tx:       Option<mpsc::Sender<(SessionId, ActionId, EvolutionId)>>,
    /// Set by main.rs so schedule_* tools route to the scheduler task.
    schedule_tx:       Option<mpsc::Sender<(SessionId, ActionId, String, serde_json::Value)>>,
    /// Set by main.rs so convene_council routes to the council handler.
    council_tx:        Option<mpsc::Sender<(SessionId, ActionId, serde_json::Value)>>,
    /// Shared with engine so read_soul_md returns the live system prompt.
    soul_arc:          Option<Arc<RwLock<String>>>,
}

impl Supervisor {
    pub fn new(bus: BusHandle, policy: Arc<RwLock<PolicyEngine>>) -> Self {
        let (sv_tx, sv_rx) = mpsc::channel::<SupervisorCmd>(64);
        Self {
            bus,
            plugins:           HashMap::new(),
            tool_registry:     HashMap::new(),
            configs:           HashMap::new(),
            policy,
            pending_approvals: HashMap::new(),
            sv_tx,
            sv_rx: Some(sv_rx),
            rollback_tx:       None,
            soul_arc:          None,
            schedule_tx:       None,
            council_tx:        None,
        }
    }

    /// Returns a sender that main.rs can use to send hot-reload commands.
    pub fn cmd_tx(&self) -> mpsc::Sender<SupervisorCmd> {
        self.sv_tx.clone()
    }

    /// Wires the rollback channel so `rollback_evolution` can reach the applier.
    pub fn set_rollback_tx(&mut self, tx: mpsc::Sender<(SessionId, ActionId, EvolutionId)>) {
        self.rollback_tx = Some(tx);
    }

    /// Wires the scheduler channel so schedule_* tools route to the scheduler task.
    pub fn set_schedule_tx(&mut self, tx: mpsc::Sender<(SessionId, ActionId, String, serde_json::Value)>) {
        self.schedule_tx = Some(tx);
    }

    /// Wires the council channel so convene_council routes to the council handler.
    pub fn set_council_tx(&mut self, tx: mpsc::Sender<(SessionId, ActionId, serde_json::Value)>) {
        self.council_tx = Some(tx);
    }

    /// Shares the live soul.md Arc so `read_soul_md` returns current content.
    pub fn set_soul_arc(&mut self, arc: Arc<RwLock<String>>) {
        self.soul_arc = Some(arc);
    }

    /// Boot all plugins from config then run the dispatch/supervision loop.
    pub async fn run(
        mut self,
        plugin_configs: Vec<PluginConfig>,
        mut bus_rx: broadcast::Receiver<Event>,
    ) {
        let mut sv_rx = self.sv_rx.take().expect("run() called twice");

        for cfg in plugin_configs {
            let tx = self.sv_tx.clone();
            if let Err(e) = self.spawn_plugin(&cfg, tx).await {
                eprintln!("[supervisor] failed to start plugin '{}': {e}", cfg.id);
            }
        }

        loop {
            tokio::select! {
                result = bus_rx.recv() => match result {
                    Ok(Event::ToolRequested { session, call }) => {
                        let decision = self.policy.read().await.check(&call.tool);
                        match decision {
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

                Some(cmd) = sv_rx.recv() => {
                    let tx = self.sv_tx.clone();
                    match cmd {
                        SupervisorCmd::PluginDied { id } => {
                            self.handle_died(id, tx).await;
                        }
                        SupervisorCmd::SpawnPlugin { config } => {
                            if let Err(e) = self.spawn_plugin(&config, tx).await {
                                eprintln!("[supervisor] spawn failed for '{}': {e}", config.id);
                            }
                        }
                        SupervisorCmd::KillPlugin { id } => {
                            // Remove config first so handle_died (from dying process) won't restart.
                            self.configs.remove(&id);
                            self.tool_registry.retain(|_, owner| owner != &id);
                            let had_plugin = self.plugins.remove(&id).is_some();
                            if had_plugin {
                                self.bus.emit(Event::PluginDown {
                                    plugin: id,
                                    reason: "killed by evolution".into(),
                                }).await;
                            }
                        }
                        SupervisorCmd::HotReload { id } => {
                            // Kill the live instance (keep config so handle_died restarts it).
                            self.tool_registry.retain(|_, owner| owner != &id);
                            let had_plugin = self.plugins.remove(&id).is_some();
                            if had_plugin {
                                self.bus.emit(Event::PluginDown {
                                    plugin: id.clone(),
                                    reason: "hot-reload".into(),
                                }).await;
                            }
                            // For non-Always policies: the process won't self-restart, so force it.
                            if let Some(cfg) = self.configs.get(&id).cloned() {
                                if cfg.restart != RestartPolicy::Always {
                                    tokio::time::sleep(Duration::from_millis(300)).await;
                                    if let Err(e) = self.spawn_plugin(&cfg, tx).await {
                                        eprintln!("[supervisor] hot-reload '{}' failed: {e}", id.0);
                                    }
                                }
                                // else: child exits → PluginDied fires → handle_died restarts
                            }
                        }
                        SupervisorCmd::SetSoulArc { arc } => {
                            self.soul_arc = Some(arc);
                        }
                        SupervisorCmd::SetScheduleTx { tx } => {
                            self.schedule_tx = Some(tx);
                        }
                        SupervisorCmd::SetCouncilTx { tx } => {
                            self.council_tx = Some(tx);
                        }
                        SupervisorCmd::DirectCall { tool, args, reply } => {
                            if let Some(pid) = self.tool_registry.get(&tool).cloned() {
                                if let Some(plugin) = self.plugins.get(&pid) {
                                    let client = plugin.client.clone();
                                    tokio::spawn(async move {
                                        let out = match client.call_tool(&tool, &args).await {
                                            Ok(o)  => o,
                                            Err(e) => ToolOutput {
                                                ok:      false,
                                                content: serde_json::json!(e.to_string()),
                                            },
                                        };
                                        let _ = reply.send(out);
                                    });
                                } else {
                                    let _ = reply.send(ToolOutput {
                                        ok: false,
                                        content: serde_json::json!(format!("plugin for '{tool}' not live")),
                                    });
                                }
                            } else {
                                let _ = reply.send(ToolOutput {
                                    ok: false,
                                    content: serde_json::json!(format!("unknown tool: {tool}")),
                                });
                            }
                        }
                    }
                },
            }
        }
    }

    /// Dispatch a tool call immediately (policy already checked).
    fn dispatch_tool(&self, session: SessionId, call: ToolCall) {
        // Virtual tool: propose_evolution (Phase 0 stub — emits EvolutionProposed
        // and acks immediately; apply logic handled by evolution applier in main.rs).
        if call.tool == "propose_evolution" {
            let evolution_id = EvolutionId(call.id.0);
            let call_id      = call.id;
            let bus          = self.bus.clone();
            match serde_json::from_value::<EvolutionProposal>(call.args.clone()) {
                Ok(proposal) => {
                    tokio::spawn(async move {
                        bus.emit(Event::EvolutionProposed {
                            id: evolution_id,
                            proposal,
                            proposed_by: session,
                        }).await;
                        bus.emit(Event::ToolResult {
                            session,
                            call: call_id,
                            output: ToolOutput {
                                ok:      true,
                                content: serde_json::json!({
                                    "status":       "proposed",
                                    "evolution_id": evolution_id.0,
                                    "note":         "proposal queued for apply",
                                }),
                            },
                        }).await;
                    });
                }
                Err(e) => {
                    let err = e.to_string();
                    let bus = self.bus.clone();
                    tokio::spawn(async move {
                        bus.emit(Event::ToolResult {
                            session,
                            call: call_id,
                            output: ToolOutput {
                                ok:      false,
                                content: serde_json::json!(
                                    format!("invalid evolution proposal: {err}")
                                ),
                            },
                        }).await;
                    });
                }
            }
            return;
        }

        // Virtual tool: rollback_evolution — routes to the applier via rollback channel.
        if call.tool == "rollback_evolution" {
            let evolution_id = call.args["evolution_id"].as_u64().map(EvolutionId);
            let call_id      = call.id;
            let bus          = self.bus.clone();
            match (evolution_id, self.rollback_tx.as_ref()) {
                (Some(eid), Some(tx)) => {
                    let tx = tx.clone();
                    tokio::spawn(async move {
                        if tx.send((session, call_id, eid)).await.is_err() {
                            bus.emit(Event::ToolResult {
                                session,
                                call: call_id,
                                output: ToolOutput {
                                    ok:      false,
                                    content: serde_json::json!("rollback channel closed"),
                                },
                            }).await;
                        }
                    });
                }
                (None, _) => {
                    tokio::spawn(async move {
                        bus.emit(Event::ToolResult {
                            session,
                            call: call_id,
                            output: ToolOutput {
                                ok:      false,
                                content: serde_json::json!("missing evolution_id"),
                            },
                        }).await;
                    });
                }
                (_, None) => {
                    tokio::spawn(async move {
                        bus.emit(Event::ToolResult {
                            session,
                            call: call_id,
                            output: ToolOutput {
                                ok:      false,
                                content: serde_json::json!("rollback not available"),
                            },
                        }).await;
                    });
                }
            }
            return;
        }

        // Virtual tool: read_soul_md — returns live soul.md content so the agent can
        // read the current system prompt before proposing update_system_prompt.
        if call.tool == "read_soul_md" {
            let call_id = call.id;
            let bus     = self.bus.clone();
            let soul    = self.soul_arc.clone();
            tokio::spawn(async move {
                let content = match soul {
                    Some(arc) => arc.read().await.clone(),
                    None      => String::from("soul.md not yet initialized"),
                };
                bus.emit(Event::ToolResult {
                    session,
                    call: call_id,
                    output: ToolOutput {
                        ok:      true,
                        content: serde_json::json!(content),
                    },
                }).await;
            });
            return;
        }

        // Virtual tools: schedule_task / list_schedules / cancel_schedule — forwarded to scheduler task.
        if matches!(call.tool.as_str(), "schedule_task" | "list_schedules" | "cancel_schedule") {
            let call_id  = call.id;
            let tool     = call.tool.clone();
            let args     = call.args.clone();
            let bus      = self.bus.clone();
            match &self.schedule_tx {
                Some(tx) => {
                    let tx = tx.clone();
                    tokio::spawn(async move {
                        if tx.send((session, call_id, tool, args)).await.is_err() {
                            bus.emit(Event::ToolResult {
                                session,
                                call: call_id,
                                output: ToolOutput { ok: false, content: serde_json::json!("scheduler not available") },
                            }).await;
                        }
                    });
                }
                None => {
                    tokio::spawn(async move {
                        bus.emit(Event::ToolResult {
                            session,
                            call: call_id,
                            output: ToolOutput { ok: false, content: serde_json::json!("scheduler not initialized") },
                        }).await;
                    });
                }
            }
            return;
        }

        // Virtual tool: convene_council — routes to the council handler task.
        if call.tool == "convene_council" {
            let call_id = call.id;
            let args    = call.args.clone();
            let bus     = self.bus.clone();
            match &self.council_tx {
                Some(tx) => {
                    let tx = tx.clone();
                    tokio::spawn(async move {
                        if tx.send((session, call_id, args)).await.is_err() {
                            bus.emit(Event::ToolResult {
                                session,
                                call: call_id,
                                output: ToolOutput { ok: false, content: serde_json::json!("council handler not available") },
                            }).await;
                        }
                    });
                }
                None => {
                    tokio::spawn(async move {
                        bus.emit(Event::ToolResult {
                            session,
                            call: call_id,
                            output: ToolOutput { ok: false, content: serde_json::json!("council not initialized") },
                        }).await;
                    });
                }
            }
            return;
        }

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
        // Only emit PluginDown if the plugin was still in our live set.
        // HotReload removes it first, so this avoids a duplicate PluginDown event.
        let was_live = self.plugins.remove(&id).is_some();

        if was_live {
            eprintln!("[supervisor] plugin '{}' died", id.0);
            self.bus.emit(Event::PluginDown {
                plugin: id.clone(),
                reason: "process exited".into(),
            }).await;
        }

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
