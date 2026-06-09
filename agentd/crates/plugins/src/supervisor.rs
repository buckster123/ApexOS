use std::{collections::HashMap, sync::Arc, time::Duration};
use std::path::PathBuf;
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
    /// Path to the events log directory so query_event_log can read JSONL files.
    events_dir:        Option<PathBuf>,
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
            events_dir:        None,
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

    pub fn set_events_dir(&mut self, dir: PathBuf) {
        self.events_dir = Some(dir);
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

        // Virtual tool: query_event_log — reads recent JSONL events and returns
        // human-readable summaries for agent analysis / Cerebro ingestion.
        if call.tool == "query_event_log" {
            let hours      = call.args["hours"].as_u64().unwrap_or(24).min(168);
            let types_arg  = call.args["types"].as_str().map(str::to_owned);
            let max_events = call.args["max"].as_u64().unwrap_or(500).min(2000) as usize;
            let events_dir = self.events_dir.clone();
            let bus        = self.bus.clone();
            let call_id    = call.id;
            tokio::spawn(async move {
                let Some(dir) = events_dir else {
                    bus.emit(Event::ToolResult {
                        session, call: call_id,
                        output: ToolOutput { ok: false, content: serde_json::json!("events_dir not configured") },
                    }).await;
                    return;
                };

                let type_filter: Option<std::collections::HashSet<String>> =
                    types_arg.as_deref().map(|s| s.split(',').map(|t| t.trim().to_owned()).collect());

                // Determine which date files to read based on hours window.
                let days_back = ((hours as f64) / 24.0).ceil() as u64 + 1;
                let today = chrono::Local::now().date_naive();
                let mut date_files: Vec<std::path::PathBuf> = Vec::new();
                for d in 0..days_back {
                    let date = today - chrono::Duration::days(d as i64);
                    let path = dir.join(format!("events-{}.jsonl", date.format("%Y-%m-%d")));
                    if tokio::fs::metadata(&path).await.is_ok() {
                        date_files.push(path);
                    }
                }
                date_files.reverse(); // oldest first

                let mut lines: Vec<String> = Vec::new();
                for path in &date_files {
                    let Ok(text) = tokio::fs::read_to_string(path).await else { continue };
                    for line in text.lines() {
                        let line = line.trim();
                        if line.is_empty() { continue }
                        let Ok(val) = serde_json::from_str::<serde_json::Value>(line) else { continue };
                        let ev_type = val["type"].as_str().unwrap_or("").to_owned();
                        if let Some(ref filter) = type_filter {
                            if !filter.contains(&ev_type) { continue }
                        }
                        if let Some(summary) = format_event_line(&val) {
                            lines.push(summary);
                        }
                    }
                }

                // Take the last max_events lines (most recent).
                let total = lines.len();
                if lines.len() > max_events {
                    lines = lines.split_off(lines.len() - max_events);
                }

                let text = if lines.is_empty() {
                    format!("No matching events found in the last {hours}h.")
                } else {
                    format!("Last {hours}h event log ({} events, showing {}):\n\n{}",
                        total, lines.len(), lines.join("\n"))
                };

                bus.emit(Event::ToolResult {
                    session, call: call_id,
                    output: ToolOutput { ok: true, content: serde_json::json!(text) },
                }).await;
            });
            return;
        }

        // Virtual tool: send_to_agent — fire-and-forget async peer-to-peer message.
        if call.tool == "send_to_agent" {
            let to_id   = call.args["session_id"].as_u64().map(SessionId);
            let body    = call.args["message"].as_str().unwrap_or("").to_owned();
            let call_id = call.id;
            let msg_id  = call.id.0;
            let bus     = self.bus.clone();
            match to_id {
                Some(to) => {
                    tokio::spawn(async move {
                        bus.emit(Event::AgentMessage { from: session, to, body, msg_id }).await;
                        bus.emit(Event::ToolResult {
                            session,
                            call: call_id,
                            output: ToolOutput {
                                ok:      true,
                                content: serde_json::json!({ "status": "sent", "msg_id": msg_id }),
                            },
                        }).await;
                    });
                }
                None => {
                    tokio::spawn(async move {
                        bus.emit(Event::ToolResult {
                            session,
                            call: call_id,
                            output: ToolOutput {
                                ok:      false,
                                content: serde_json::json!("send_to_agent: missing or invalid session_id"),
                            },
                        }).await;
                    });
                }
            }
            return;
        }

        // Virtual tool: list_mesh_peers — returns current peers.toml as JSON.
        if call.tool == "list_mesh_peers" {
            let call_id = call.id;
            let bus     = self.bus.clone();
            tokio::spawn(async move {
                let path = std::env::var("PEERS_TOML")
                    .unwrap_or_else(|_| "/etc/agentd/peers.toml".into());
                let content = tokio::fs::read_to_string(&path).await
                    .unwrap_or_else(|_| "# no peers registered\n".into());
                bus.emit(Event::ToolResult {
                    session, call: call_id,
                    output: ToolOutput { ok: true, content: serde_json::json!(content) },
                }).await;
            });
            return;
        }

        // Virtual tool: bootstrap_node — SSH to target, clone ApexOS repo, background install.sh.
        // Returns quickly; install takes 15-20 min and node appears in mesh via mDNS.
        if call.tool == "bootstrap_node" {
            let target_ip   = call.args["target_ip"].as_str().unwrap_or("").to_owned();
            let ssh_password = call.args["ssh_password"].as_str().unwrap_or("").to_owned();
            let ssh_user    = call.args["ssh_user"].as_str().unwrap_or("apexos").to_owned();
            let api_key     = call.args["api_key"].as_str().unwrap_or("").to_owned();
            let repo_url    = call.args["repo_url"].as_str()
                .unwrap_or("https://github.com/buckster123/ApexOS.git").to_owned();
            let call_id     = call.id;
            let bus         = self.bus.clone();

            if target_ip.is_empty() || ssh_password.is_empty() {
                tokio::spawn(async move {
                    bus.emit(Event::ToolResult {
                        session, call: call_id,
                        output: ToolOutput {
                            ok:      false,
                            content: serde_json::json!("bootstrap_node: target_ip and ssh_password are required"),
                        },
                    }).await;
                });
                return;
            }

            tokio::spawn(async move {
                let ssh_base = vec![
                    "sshpass".to_string(),
                    format!("-p{ssh_password}"),
                    "ssh".into(),
                    "-o".into(), "StrictHostKeyChecking=no".into(),
                    "-o".into(), "ConnectTimeout=5".into(),
                    format!("{ssh_user}@{target_ip}"),
                ];

                // Step 1: connectivity check
                let ok = tokio::process::Command::new(&ssh_base[0])
                    .args(&ssh_base[1..])
                    .arg("echo OK")
                    .output().await;
                match ok {
                    Err(e) => {
                        bus.emit(Event::ToolResult {
                            session, call: call_id,
                            output: ToolOutput {
                                ok:      false,
                                content: serde_json::json!(format!("SSH to {target_ip} failed: {e}")),
                            },
                        }).await;
                        return;
                    }
                    Ok(o) if !o.status.success() => {
                        let stderr = String::from_utf8_lossy(&o.stderr);
                        bus.emit(Event::ToolResult {
                            session, call: call_id,
                            output: ToolOutput {
                                ok:      false,
                                content: serde_json::json!(format!("SSH auth failed for {ssh_user}@{target_ip}: {stderr}")),
                            },
                        }).await;
                        return;
                    }
                    _ => {}
                }

                // Step 2: check if already an ApexOS node
                let check = tokio::process::Command::new(&ssh_base[0])
                    .args(&ssh_base[1..])
                    .arg("systemctl is-active agentd 2>/dev/null || echo inactive")
                    .output().await;
                if let Ok(o) = check {
                    let out = String::from_utf8_lossy(&o.stdout).trim().to_string();
                    if out == "active" {
                        bus.emit(Event::ToolResult {
                            session, call: call_id,
                            output: ToolOutput {
                                ok:      true,
                                content: serde_json::json!(format!(
                                    "{target_ip} is already running agentd. Register it manually with POST /api/mesh/peers."
                                )),
                            },
                        }).await;
                        return;
                    }
                }

                // Step 3: install git if needed, clone repo
                let prep_cmd = format!(
                    "apt-get install -y -q git 2>/dev/null; \
                     git clone {repo_url} /home/{ssh_user}/ApexOS 2>/dev/null || \
                     git -C /home/{ssh_user}/ApexOS pull"
                );
                let prep = tokio::time::timeout(
                    tokio::time::Duration::from_secs(60),
                    tokio::process::Command::new(&ssh_base[0])
                        .args(&ssh_base[1..])
                        .arg(format!("echo '{ssh_password}' | sudo -S bash -c {prep_cmd:?}"))
                        .output(),
                ).await;
                if prep.is_err() {
                    bus.emit(Event::ToolResult {
                        session, call: call_id,
                        output: ToolOutput {
                            ok:      false,
                            content: serde_json::json!("bootstrap_node: git clone timed out"),
                        },
                    }).await;
                    return;
                }

                // Step 4: inject API key (if provided) and background install.sh
                let api_key_export = if api_key.is_empty() {
                    String::new()
                } else {
                    format!("export ANTHROPIC_API_KEY={api_key:?}; ")
                };
                let install_cmd = format!(
                    "cd /home/{ssh_user}/ApexOS && \
                     {api_key_export}\
                     nohup bash install.sh > /tmp/apex-install.log 2>&1 &\
                     echo $!"
                );
                let launch = tokio::process::Command::new(&ssh_base[0])
                    .args(&ssh_base[1..])
                    .arg(format!("echo '{ssh_password}' | sudo -S bash -c {install_cmd:?}"))
                    .output().await;

                let pid_line = launch.as_ref().ok()
                    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                    .unwrap_or_default();

                let msg = format!(
                    "Bootstrap of {ssh_user}@{target_ip} started (PID {pid_line}). \
                     install.sh is running in background — takes 15-20 min. \
                     Monitor: ssh {ssh_user}@{target_ip} tail -f /tmp/apex-install.log. \
                     The node will appear in the mesh automatically once Avahi starts."
                );
                bus.emit(Event::ToolResult {
                    session, call: call_id,
                    output: ToolOutput { ok: true, content: serde_json::json!(msg) },
                }).await;
            });
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

/// Convert a raw event JSON object into a concise human-readable sentence.
/// Returns None for high-frequency noise events (agent_text, tool_result, etc.)
fn format_event_line(v: &serde_json::Value) -> Option<String> {
    let t = v["type"].as_str()?;
    let line = match t {
        // Skip streaming noise
        "agent_text" | "agent_thinking" | "tool_result" | "turn_complete" => return None,

        "user_prompt" => {
            let session = v["session"].as_u64().unwrap_or(0);
            let text    = v["text"].as_str().unwrap_or("").chars().take(120).collect::<String>();
            format!("Session {session}: user said '{text}'")
        }
        "tool_requested" => {
            let session = v["session"].as_u64().unwrap_or(0);
            let tool    = v["call"]["tool"].as_str().unwrap_or("?");
            format!("Session {session}: tool '{tool}' called")
        }
        "approval_pending" => {
            let session = v["session"].as_u64().unwrap_or(0);
            let tool    = v["call"]["tool"].as_str().unwrap_or("?");
            format!("Session {session}: tool '{tool}' awaiting approval")
        }
        "user_approval" => {
            let session = v["session"].as_u64().unwrap_or(0);
            let granted = v["granted"].as_bool().unwrap_or(false);
            format!("Session {session}: approval {}", if granted { "granted" } else { "denied" })
        }
        "evolution_proposed" => {
            let kind   = v["proposal"]["kind"].as_str().unwrap_or("?");
            let reason = v["proposal"]["reason"].as_str().unwrap_or("");
            format!("Evolution proposed: {kind} — '{reason}'")
        }
        "evolution_applied" => {
            let kind   = v["proposal"]["kind"].as_str().unwrap_or("?");
            let reason = v["proposal"]["reason"].as_str().unwrap_or("");
            format!("Evolution applied: {kind} — '{reason}'")
        }
        "evolution_rolled_back" => {
            format!("Evolution rolled back (id={})", v["id"].as_u64().unwrap_or(0))
        }
        "plugin_up" => {
            let plugin = v["plugin"].as_str().unwrap_or("?");
            let n      = v["tools"].as_array().map(|a| a.len()).unwrap_or(0);
            format!("Plugin '{plugin}' started ({n} tools)")
        }
        "plugin_down" => {
            let plugin = v["plugin"].as_str().unwrap_or("?");
            format!("Plugin '{plugin}' stopped")
        }
        "wake_triggered"   => "Wake word triggered".into(),
        "spawn_agent"      => {
            let parent = v["parent"].as_u64().unwrap_or(0);
            let prompt = v["prompt"].as_str().unwrap_or("").chars().take(80).collect::<String>();
            format!("Session {parent}: spawned sub-agent — '{prompt}'")
        }
        "sub_agent_started" => {
            let child  = v["child"].as_u64().unwrap_or(0);
            let parent = v["parent"].as_u64().unwrap_or(0);
            format!("Sub-agent {child} started (parent: {parent})")
        }
        "sensor_reading" => {
            let node    = v["node_id"].as_str().unwrap_or("?");
            let reading = &v["reading"];
            if let Some(iaq) = reading["iaq"].as_f64() {
                let temp = reading["temperature"].as_f64().unwrap_or(0.0);
                let rh   = reading["humidity"].as_f64().unwrap_or(0.0);
                format!("Sensor {node}: IAQ={iaq:.0} Temp={temp:.1}°C RH={rh:.0}%")
            } else {
                format!("Sensor {node}: {reading}")
            }
        }
        "council_started" => {
            let id    = v["id"].as_str().unwrap_or("?");
            let topic = v["topic"].as_str().unwrap_or("");
            format!("Council '{id}' started: topic='{topic}'")
        }
        "council_complete" => {
            let id    = v["id"].as_str().unwrap_or("?");
            let synth = v["synthesis"].as_str().unwrap_or("").chars().take(100).collect::<String>();
            format!("Council '{id}' complete: '{synth}'")
        }
        "agent_message" => {
            let from = v["from"].as_u64().unwrap_or(0);
            let to   = v["to"].as_u64().unwrap_or(0);
            let body = v["body"].as_str().unwrap_or("").chars().take(80).collect::<String>();
            format!("Agent {from} → Agent {to}: '{body}'")
        }
        // Unknown event types: show the type name so they appear in results
        other => format!("[{other}]"),
    };
    Some(line)
}
