mod session_store;
use session_store::SessionStore;
mod scheduler;
use scheduler::{load_schedules, run_scheduler, spawn_scheduler_handler, SchedulerState};

use apexos_core::{
    ActionId, Bus, ContentBlock, Event, EvolutionId, EvolutionProposal, Message,
    PluginId, PolicyMode, SessionId, SensorReading, Subsystem, SystemState, ToolOutput, ToolSpec,
};
use apexos_gateway::{serve, GatewayState};
use apexos_plugins::{
    load as load_plugins, PluginConfig, PolicyConfig, PolicyEngine, RestartPolicy,
    Supervisor, SupervisorCmd, ToolProxy,
};
use apexos_agent::{AnthropicProvider, TurnEngine, run_turn};
use apexos_store::run_log_writer;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::{broadcast, mpsc, Mutex, RwLock};
use tokio::task::AbortHandle;

fn load_soul() -> (PathBuf, String) {
    let path = std::env::var("AGENTD_SOUL")
        .unwrap_or_else(|_| "/etc/agentd/soul.md".into());
    match std::fs::read_to_string(&path) {
        Ok(s) if !s.trim().is_empty() => {
            eprintln!("[agentd] soul loaded from {path}");
            (PathBuf::from(&path), s)
        }
        _ => {
            let dev = std::env::var("AGENTD_SOUL_DEV")
                .unwrap_or_else(|_| "config/soul.md".into());
            match std::fs::read_to_string(&dev) {
                Ok(s) if !s.trim().is_empty() => {
                    eprintln!("[agentd] soul loaded from {dev}");
                    (PathBuf::from(&dev), s)
                }
                _ => {
                    eprintln!("[agentd] soul.md not found — running without system prompt");
                    // Default write path even when the file doesn't exist yet
                    (PathBuf::from("/etc/agentd/soul.md"), String::new())
                }
            }
        }
    }
}

fn load_api_key() -> String {
    // 1. Environment variable (set by systemd EnvironmentFile or shell)
    if let Ok(k) = std::env::var("ANTHROPIC_API_KEY") {
        if !k.is_empty() { return k; }
    }
    // 2. Runtime file written by the browser UI key-entry flow
    let path = std::env::var("AGENTD_KEY_FILE")
        .unwrap_or_else(|_| "/var/lib/agentd/.api_key".into());
    if let Ok(k) = std::fs::read_to_string(&path) {
        let k = k.trim().to_string();
        if !k.is_empty() { return k; }
    }
    String::new()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let (bus, handle, bcast) = Bus::new(SystemState::default());
    tokio::spawn(bus.run());

    // Shared API key + model — readable/writable from both the turn engine and browser UI
    let api_key_str = load_api_key();
    if api_key_str.is_empty() {
        eprintln!("[agentd] ANTHROPIC_API_KEY not set — enter via browser UI at :8787");
    }
    let api_key_arc = Arc::new(RwLock::new(api_key_str));
    let model_arc   = Arc::new(RwLock::new("claude-sonnet-4-6".to_string()));

    // Load policy config and wrap in a shared Arc so the evolution applier can hot-swap it.
    let policy_path = PathBuf::from(
        std::env::var("AGENTD_POLICY_TOML")
            .unwrap_or_else(|_| "config/policy.toml".into())
    );
    let policy_config = match PolicyConfig::load(&policy_path) {
        Ok(c)  => { eprintln!("[agentd] policy mode: {:?}", c.mode); c }
        Err(e) => { eprintln!("[agentd] policy config: {e} — using defaults"); PolicyConfig::default() }
    };
    let policy_mode_str = format!("{:?}", policy_config.mode).to_lowercase().replace("autoedit", "auto-edit");
    let policy_mode_arc: Arc<RwLock<String>> = Arc::new(RwLock::new(policy_mode_str));
    let policy_arc: Arc<RwLock<PolicyEngine>> =
        Arc::new(RwLock::new(PolicyEngine::new(policy_config)));

    // Channel for live policy mode changes from the /api/policy gateway route.
    let (policy_set_tx, mut policy_set_rx) = tokio::sync::mpsc::channel::<String>(8);
    {
        let policy_arc2 = Arc::clone(&policy_arc);
        let policy_mode_arc2 = Arc::clone(&policy_mode_arc);
        tokio::spawn(async move {
            while let Some(mode_str) = policy_set_rx.recv().await {
                let new_mode = match mode_str.as_str() {
                    "auto-edit" => PolicyMode::AutoEdit,
                    "yolo"      => PolicyMode::Yolo,
                    _           => PolicyMode::Suggest,
                };
                policy_arc2.write().await.config.mode = new_mode;
                *policy_mode_arc2.write().await = mode_str.clone();
                eprintln!("[agentd] policy mode changed to: {mode_str}");
            }
        });
    }

    // Gateway
    let ui_dir = PathBuf::from(
        std::env::var("AGENTD_UI").unwrap_or_else(|_| "ui".into())
    );
    let log_dir = PathBuf::from(
        std::env::var("AGENTD_LOG").unwrap_or_else(|_| "events".into())
    );

    // Session store — init early so histories and next_session_id are ready for GatewayState.
    let session_store = Arc::new(SessionStore::new(&log_dir));
    session_store.init().await?;
    let initial_histories = session_store.load_all().await;

    // Server-issued session IDs — start above any IDs already loaded from disk.
    let max_loaded_sid = initial_histories.keys().map(|s| s.0).max().unwrap_or(0);
    let next_session_id = Arc::new(AtomicU64::new(max_loaded_sid + 1));

    // Shared state for the agent router (created early — needed by GatewayState too).
    let tool_reg: Arc<RwLock<HashMap<PluginId, Vec<ToolSpec>>>> =
        Arc::new(RwLock::new(HashMap::new()));
    let histories: Arc<Mutex<HashMap<SessionId, Vec<Message>>>> =
        Arc::new(Mutex::new(initial_histories));

    let sensor_bridge_token = Arc::new(
        std::env::var("SENSOR_BRIDGE_TOKEN").unwrap_or_default()
    );

    // Load soul early so we can share the path with both the gateway (settings UI) and
    // the turn engine below.
    let (soul_path, soul_content) = load_soul();

    eprintln!("[agentd] serving UI from {}", ui_dir.display());
    let gw_state = GatewayState {
        bus:                  handle.clone(),
        bcast:                bcast.clone(),
        api_key:              Arc::clone(&api_key_arc),
        model:                Arc::clone(&model_arc),
        policy_mode:          Arc::clone(&policy_mode_arc),
        policy_set_tx,
        ui_dir,
        events_dir:           log_dir.clone(),
        sessions_dir:         log_dir.join("sessions"),
        histories:            Arc::clone(&histories),
        next_session_id:      Arc::clone(&next_session_id),
        sensor_bridge_token:  sensor_bridge_token,
        soul_path:            soul_path.clone(),
        policy_arc:           Arc::clone(&policy_arc),
    };
    let gw_addr: std::net::SocketAddr = "0.0.0.0:8787".parse()?;
    tokio::spawn(async move {
        if let Err(e) = serve(gw_state, gw_addr).await {
            eprintln!("[gateway] error: {e}");
        }
    });

    // Plugin configs
    let plugins_path = PathBuf::from(
        std::env::var("AGENTD_PLUGINS_TOML")
            .unwrap_or_else(|_| "config/plugins.toml".into())
    );
    let plugin_configs = match load_plugins(&plugins_path) {
        Ok(c)  => { eprintln!("[agentd] loaded {} plugin(s)", c.len()); c }
        Err(e) => { eprintln!("[agentd] plugins config: {e}"); vec![] }
    };

    // Read subagents config from the policy (already loaded above).
    let max_depth = {
        // Re-read so we can get subagents config without holding the Arc lock
        // (the common path; the value doesn't change during normal operation)
        let guard = policy_arc.read().await;
        guard.config.subagents.max_depth
    };

    // Supervisor — pass policy_arc so the evolution applier can hot-swap the engine.
    let mut supervisor = Supervisor::new(handle.clone(), Arc::clone(&policy_arc));
    let sv_cmd_tx      = supervisor.cmd_tx();
    // Rollback channel: applier receives (session, call_id, evolution_id) requests.
    let (rollback_tx, rollback_rx) = mpsc::channel::<(SessionId, ActionId, EvolutionId)>(16);
    supervisor.set_rollback_tx(rollback_tx);
    tokio::spawn(supervisor.run(plugin_configs, bcast.subscribe()));

    // Agent turn engine — shares key + model Arcs so browser UI changes take effect immediately
    let engine: Arc<TurnEngine> = Arc::new(TurnEngine::new(
        AnthropicProvider::new_shared(Arc::clone(&api_key_arc), Arc::clone(&model_arc)),
        16,
        Some(soul_content),
    ));
    let soul_arc = engine.system_arc();

    // Share soul_arc with the supervisor so read_soul_md returns live content.
    if sv_cmd_tx.send(SupervisorCmd::SetSoulArc { arc: soul_arc.clone() }).await.is_err() {
        eprintln!("[agentd] warning: failed to share soul_arc with supervisor");
    }

    // Rollback store: undo snapshots indexed by EvolutionId (in-memory, cleared on restart).
    let rollback_store: Arc<Mutex<HashMap<EvolutionId, EvolutionProposal>>> =
        Arc::new(Mutex::new(HashMap::new()));

    // ToolProxy — lets the evolution applier call Cerebro tools directly for episode tracking.
    let tool_proxy = ToolProxy::new(sv_cmd_tx.clone());

    // Restore rollback snapshots from Cerebro evolution episodes on startup (best-effort).
    // CerebroCortex needs a moment to start; we wait then populate rollback_store from episodes.
    {
        let proxy = tool_proxy.clone();
        let store = Arc::clone(&rollback_store);
        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
            restore_rollback_store(&proxy, &store).await;
        });
    }

    // Evolution applier — subscribes to EvolutionProposed and applies changes live.
    spawn_evolution_applier(
        bcast.subscribe(),
        handle.clone(),
        Arc::clone(&soul_arc),
        soul_path,
        policy_path,
        plugins_path,
        Arc::clone(&policy_arc),
        sv_cmd_tx.clone(),
        rollback_rx,
        Arc::clone(&rollback_store),
        tool_proxy,
    );

    // Scheduler — load persisted schedules and wire into supervisor.
    let schedules_path = log_dir.join("schedules.jsonl");
    let initial_schedules = load_schedules(&schedules_path);
    if !initial_schedules.is_empty() {
        eprintln!("[scheduler] restored {} scheduled task(s)", initial_schedules.len());
    }
    let scheduler_state: SchedulerState = Arc::new(Mutex::new(initial_schedules));
    let (sched_tx, sched_rx) = mpsc::channel::<(SessionId, ActionId, String, serde_json::Value)>(32);
    if sv_cmd_tx.send(SupervisorCmd::SetScheduleTx { tx: sched_tx }).await.is_err() {
        eprintln!("[agentd] warning: failed to wire scheduler channel");
    }
    let root_session = SessionId(0); // scheduled prompts fire on root session unless task specifies
    spawn_scheduler_handler(Arc::clone(&scheduler_state), schedules_path.clone(), handle.clone(), sched_rx);
    tokio::spawn(run_scheduler(Arc::clone(&scheduler_state), handle.clone(), schedules_path, root_session));

    // Subscribe before supervisor so no early PluginUp events are missed.
    let agent_rx = bcast.subscribe();
    spawn_agent_router(agent_rx, bcast.clone(), handle.clone(),
                       tool_reg, histories, engine, max_depth, session_store);

    // Event log
    tokio::spawn(run_log_writer(log_dir, bcast.subscribe()));

    eprintln!("[agentd] ready — gateway ws://0.0.0.0:8787/ws");
    tokio::signal::ctrl_c().await?;
    eprintln!("[agentd] shutting down");
    Ok(())
}

// ── evolution applier ─────────────────────────────────────────────────────────

fn spawn_evolution_applier(
    mut bus_rx:      broadcast::Receiver<Event>,
    bus:             apexos_core::BusHandle,
    soul_arc:        Arc<RwLock<String>>,
    soul_path:       PathBuf,
    policy_path:     PathBuf,
    plugins_path:    PathBuf,
    policy_arc:      Arc<RwLock<PolicyEngine>>,
    sv_cmd_tx:       mpsc::Sender<SupervisorCmd>,
    mut rollback_rx: mpsc::Receiver<(SessionId, ActionId, EvolutionId)>,
    rollback_store:  Arc<Mutex<HashMap<EvolutionId, EvolutionProposal>>>,
    tool_proxy:      ToolProxy,
) {
    tokio::spawn(async move {
        loop {
            tokio::select! {
                result = bus_rx.recv() => match result {
                    Ok(Event::EvolutionProposed { id, proposal, proposed_by: _ }) => {
                        let kind = evo_kind(&proposal);

                        // Open a Cerebro episode for this apply (best-effort).
                        let episode_id = episode_start(&tool_proxy, id, &kind).await;

                        // Snapshot current state for rollback BEFORE applying.
                        let undo = compute_undo(
                            &proposal, &soul_arc, &soul_path, &policy_path, &plugins_path,
                        ).await;

                        let proposal_copy = proposal.clone();
                        let result = apply_evolution(
                            id, proposal,
                            &soul_arc, &soul_path, &policy_path, &plugins_path,
                            &policy_arc, &sv_cmd_tx,
                        ).await;
                        match result {
                            Ok(summary) => {
                                eprintln!("[evolution] applied {:?}: {summary}", id);
                                if let Some(undo_proposal) = undo {
                                    // Record undo snapshot in the episode before storing in memory.
                                    if let Some(ref eid) = episode_id {
                                        episode_add_step(&tool_proxy, eid, &undo_proposal, &summary).await;
                                    }
                                    rollback_store.lock().await.insert(id, undo_proposal);
                                }
                                episode_end(&tool_proxy, &episode_id, "success", &summary).await;
                                bus.emit(Event::EvolutionApplied {
                                    id,
                                    proposal:      proposal_copy,
                                    patch_summary: summary,
                                    applied_by:    None,
                                }).await;
                            }
                            Err(e) => {
                                eprintln!("[evolution] apply failed {:?}: {e}", id);
                                episode_end(&tool_proxy, &episode_id, "failed", &e.to_string()).await;
                                bus.emit(Event::Error {
                                    session: None,
                                    message: format!("evolution {}: {e}", id.0),
                                }).await;
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                    Ok(_)  => {}
                },

                Some((session, call_id, evo_id)) = rollback_rx.recv() => {
                    let undo = rollback_store.lock().await.remove(&evo_id);
                    match undo {
                        None => {
                            bus.emit(Event::ToolResult {
                                session,
                                call:   call_id,
                                output: ToolOutput {
                                    ok:      false,
                                    content: serde_json::json!(
                                        format!("no rollback snapshot for evolution {}", evo_id.0)
                                    ),
                                },
                            }).await;
                        }
                        Some(undo_proposal) => {
                            let result = apply_evolution(
                                evo_id, undo_proposal,
                                &soul_arc, &soul_path, &policy_path, &plugins_path,
                                &policy_arc, &sv_cmd_tx,
                            ).await;
                            match result {
                                Ok(summary) => {
                                    eprintln!("[evolution] rolled back {:?}: {summary}", evo_id);
                                    bus.emit(Event::EvolutionRolledBack {
                                        evolution_id:   evo_id,
                                        reason:         "user requested rollback".into(),
                                        rolled_back_by: Some(session),
                                    }).await;
                                    bus.emit(Event::ToolResult {
                                        session,
                                        call:   call_id,
                                        output: ToolOutput {
                                            ok:      true,
                                            content: serde_json::json!({
                                                "status":  "rolled_back",
                                                "summary": summary,
                                            }),
                                        },
                                    }).await;
                                }
                                Err(e) => {
                                    eprintln!("[evolution] rollback failed {:?}: {e}", evo_id);
                                    bus.emit(Event::ToolResult {
                                        session,
                                        call:   call_id,
                                        output: ToolOutput {
                                            ok:      false,
                                            content: serde_json::json!(e.to_string()),
                                        },
                                    }).await;
                                }
                            }
                        }
                    }
                },
            }
        }
    });
}

// ── evolution episode helpers (Cerebro, best-effort) ─────────────────────────

fn evo_kind(proposal: &EvolutionProposal) -> String {
    serde_json::to_value(proposal).ok()
        .and_then(|v| v.get("kind").and_then(|k| k.as_str()).map(str::to_owned))
        .unwrap_or_else(|| "unknown".into())
}

/// Extract the text string from an MCP ToolOutput (content is an array of typed blocks).
fn mcp_text(output: &apexos_core::ToolOutput) -> Option<String> {
    match &output.content {
        serde_json::Value::Array(blocks) => blocks.iter()
            .find_map(|b| b.get("text").and_then(|t| t.as_str()))
            .map(str::to_owned),
        serde_json::Value::String(s) => Some(s.clone()),
        other => Some(other.to_string()),
    }
}

/// Parse an ID from a Cerebro response. Tries JSON first, then the
/// human-readable "... (ID: xxxxx)" format that Cerebro 0.5.1 returns.
fn parse_cerebro_id(output: &apexos_core::ToolOutput, json_key: &str) -> Option<String> {
    let text = mcp_text(output)?;
    if let Some(id) = serde_json::from_str::<serde_json::Value>(&text).ok()
        .and_then(|v| v.get(json_key).and_then(|id| id.as_str()).map(str::to_owned))
    {
        return Some(id);
    }
    // Cerebro 0.5.1: "Episode started (ID: ep_xxxxx)" / "Memory stored (ID: mem_xxxxx)"
    let prefix = "(ID: ";
    let start = text.find(prefix)? + prefix.len();
    let end   = start + text[start..].find(')')?;
    Some(text[start..end].to_owned())
}

async fn episode_start(proxy: &ToolProxy, evo_id: EvolutionId, kind: &str) -> Option<String> {
    match proxy.call("episode_start", serde_json::json!({
        "title":    format!("evolution {}: {kind}", evo_id.0),
        "agent_id": "CLAUDE-APEX",
        "tags":     ["evolution", kind]
    })).await {
        Ok(out) if out.ok => parse_cerebro_id(&out, "episode_id"),
        Ok(out) => { eprintln!("[evolution] episode_start not ok: {:?}", out.content); None }
        Err(e)  => { eprintln!("[evolution] episode_start: {e}"); None }
    }
}

/// Store the undo snapshot as a memory, then link it to the episode as a step.
async fn episode_add_step(proxy: &ToolProxy, episode_id: &str, undo: &EvolutionProposal, summary: &str) {
    let undo_json = serde_json::to_string(undo).unwrap_or_default();
    let content   = format!("evolution apply: {summary}\nundo_snapshot: {undo_json}");

    // Step 1: store the undo snapshot as a memory to get a memory_id.
    let memory_id = match proxy.call("memory_store", serde_json::json!({
        "content": content,
        "tags":    ["evolution", "undo_snapshot"]
    })).await {
        Ok(out) if out.ok => parse_cerebro_id(&out, "memory_id"),
        Ok(out) => { eprintln!("[evolution] memory_store not ok: {:?}", out.content); None }
        Err(e)  => { eprintln!("[evolution] memory_store: {e}"); None }
    };

    let Some(mid) = memory_id else { return };

    // Step 2: link the memory to the episode.
    if let Err(e) = proxy.call("episode_add_step", serde_json::json!({
        "episode_id": episode_id,
        "memory_id":  mid,
        "role":       "event"
    })).await {
        eprintln!("[evolution] episode_add_step: {e}");
    }
}

async fn episode_end(proxy: &ToolProxy, episode_id: &Option<String>, outcome: &str, summary: &str) {
    let Some(eid) = episode_id.as_deref() else { return };
    let valence = match outcome { "success" => "positive", "failed" => "negative", _ => "neutral" };
    if let Err(e) = proxy.call("episode_end", serde_json::json!({
        "episode_id": eid,
        "summary":    summary,
        "valence":    valence
    })).await {
        eprintln!("[evolution] episode_end: {e}");
    }
}

/// On cold-start: read all Cerebro evolution episodes, parse undo snapshots, rebuild rollback_store.
/// Best-effort — if Cerebro is unavailable, rollback_store stays empty and apply still works.
async fn restore_rollback_store(
    proxy:          &ToolProxy,
    rollback_store: &Arc<Mutex<HashMap<EvolutionId, EvolutionProposal>>>,
) {
    let text = match proxy.call("list_episodes", serde_json::json!({
        "agent_id": "CLAUDE-APEX",
        "limit":    200
    })).await {
        Ok(out) if out.ok => match mcp_text(&out) {
            Some(t) => t,
            None    => { eprintln!("[evolution] restore: no text from list_episodes"); return; }
        },
        Ok(out) => { eprintln!("[evolution] restore: list_episodes not ok: {:?}", out.content); return; }
        Err(e)  => { eprintln!("[evolution] restore: list_episodes: {e}"); return; }
    };

    let mut count = 0usize;
    for line in text.lines() {
        let line = line.trim();
        if !line.starts_with("- ep_") { continue; }
        let line = &line[2..]; // strip "- "

        let (episode_id, rest) = match line.split_once(": ") {
            Some(pair) => pair,
            None       => continue,
        };
        let title = match rest.split_once(" | steps:") {
            Some((t, _)) => t,
            None         => rest,
        };
        if !title.starts_with("evolution ") { continue; }

        let evo_id = match parse_evolution_id_from_title(title) {
            Some(id) => id,
            None     => { eprintln!("[evolution] restore: can't parse id from '{title}'"); continue; }
        };

        let mems_text = match proxy.call("get_episode_memories", serde_json::json!({
            "episode_id": episode_id,
            "agent_id":   "CLAUDE-APEX"
        })).await {
            Ok(out) if out.ok => match mcp_text(&out) { Some(t) => t, None => continue },
            _ => continue,
        };

        if let Some(proposal) = parse_undo_snapshot_from_text(&mems_text) {
            rollback_store.lock().await.insert(evo_id, proposal);
            count += 1;
        }
    }

    eprintln!("[evolution] restore: loaded {count} rollback snapshot(s) from Cerebro");
}

fn parse_evolution_id_from_title(title: &str) -> Option<EvolutionId> {
    // "evolution {N}: {kind}"
    let rest  = title.strip_prefix("evolution ")?;
    let colon = rest.find(':')?;
    let n: u64 = rest[..colon].trim().parse().ok()?;
    Some(EvolutionId(n))
}

fn parse_undo_snapshot_from_text(text: &str) -> Option<EvolutionProposal> {
    // Memory content: "evolution apply: {summary}\nundo_snapshot: {compact_json}"
    // compact_json has no literal newlines (serde_json::to_string escapes them).
    let marker = "undo_snapshot: ";
    let start  = text.find(marker)? + marker.len();
    let rest   = &text[start..];
    let end    = rest.find('\n').unwrap_or(rest.len());
    serde_json::from_str(&rest[..end]).ok()
}

/// Snapshot current state to produce an inverse proposal (for rollback).
/// Returns None for proposals that have no meaningful undo (e.g. HotReload).
async fn compute_undo(
    proposal:     &EvolutionProposal,
    soul_arc:     &Arc<RwLock<String>>,
    _soul_path:   &PathBuf,
    policy_path:  &PathBuf,
    plugins_path: &PathBuf,
) -> Option<EvolutionProposal> {
    match proposal {
        EvolutionProposal::UpdateSystemPrompt { .. } => {
            let old = soul_arc.read().await.clone();
            Some(EvolutionProposal::UpdateSystemPrompt {
                content: old,
                reason:  "rollback".into(),
            })
        }
        EvolutionProposal::UpdatePolicyRule { tool_pattern, .. } => {
            let toml_text = tokio::fs::read_to_string(policy_path).await.ok()?;
            let doc       = toml_text.parse::<toml_edit::DocumentMut>().ok()?;
            let old_mode_str = doc.get("rules")?.as_table()?.get(tool_pattern.as_str())?.as_str()?;
            let old_mode = match old_mode_str {
                "auto-edit" => PolicyMode::AutoEdit,
                "yolo"      => PolicyMode::Yolo,
                _           => PolicyMode::Suggest,
            };
            Some(EvolutionProposal::UpdatePolicyRule {
                tool_pattern: tool_pattern.clone(),
                new_mode:     old_mode,
                reason:       "rollback".into(),
            })
        }
        EvolutionProposal::RegisterMcpServer { name, .. } => {
            Some(EvolutionProposal::UnregisterMcpServer {
                name:   name.clone(),
                reason: "rollback".into(),
            })
        }
        EvolutionProposal::UnregisterMcpServer { name, .. } => {
            let toml_text = tokio::fs::read_to_string(plugins_path).await.ok()?;
            let doc = toml_text.parse::<toml_edit::DocumentMut>().ok()?;
            let arr = doc.get("plugin")?.as_array_of_tables()?;
            let tbl = arr.iter().find(|t| {
                t.get("id").and_then(|v| v.as_str()) == Some(name.as_str())
            })?;
            let cmd = tbl.get("cmd")?.as_str()?.to_string();
            Some(EvolutionProposal::RegisterMcpServer {
                name:    name.clone(),
                command: cmd,
                env:     std::collections::HashMap::new(),
                reason:  "rollback".into(),
            })
        }
        EvolutionProposal::HotReloadSubsystem { .. } => None,
    }
}

async fn apply_evolution(
    _id:          EvolutionId,
    proposal:     EvolutionProposal,
    soul_arc:     &Arc<RwLock<String>>,
    soul_path:    &PathBuf,
    policy_path:  &PathBuf,
    plugins_path: &PathBuf,
    policy_arc:   &Arc<RwLock<PolicyEngine>>,
    sv_cmd_tx:    &mpsc::Sender<SupervisorCmd>,
) -> anyhow::Result<String> {
    match proposal {
        EvolutionProposal::UpdateSystemPrompt { content, reason: _ } => {
            tokio::fs::write(soul_path, &content).await?;
            *soul_arc.write().await = content.clone();
            eprintln!("[evolution] soul.md updated ({} chars)", content.len());
            Ok(format!("system prompt updated ({} chars)", content.len()))
        }

        EvolutionProposal::UpdatePolicyRule { tool_pattern, new_mode, reason: _ } => {
            let toml_text = tokio::fs::read_to_string(policy_path).await?;
            let mut doc = toml_text.parse::<toml_edit::DocumentMut>()?;
            let mode_str = match new_mode {
                PolicyMode::Suggest  => "suggest",
                PolicyMode::AutoEdit => "auto-edit",
                PolicyMode::Yolo     => "yolo",
            };
            if let Some(rules) = doc.get_mut("rules").and_then(|v| v.as_table_mut()) {
                rules.insert(&tool_pattern, toml_edit::value(mode_str));
            }
            let new_toml = doc.to_string();
            tokio::fs::write(policy_path, &new_toml).await?;
            let new_config = PolicyConfig::load(policy_path)?;
            *policy_arc.write().await = PolicyEngine::new(new_config);
            eprintln!("[evolution] policy rule '{tool_pattern}' = '{mode_str}'");
            Ok(format!("policy rule '{tool_pattern}' set to '{mode_str}'"))
        }

        EvolutionProposal::RegisterMcpServer { name, command, env, reason: _ } => {
            let toml_text = tokio::fs::read_to_string(plugins_path).await?;
            let mut doc = toml_text.parse::<toml_edit::DocumentMut>()?;
            if let Some(arr) = doc.get_mut("plugin").and_then(|v| v.as_array_of_tables_mut()) {
                let mut tbl = toml_edit::Table::new();
                tbl.insert("id",      toml_edit::value(name.as_str()));
                tbl.insert("cmd",     toml_edit::value(command.as_str()));
                tbl.insert("restart", toml_edit::value("always"));
                if !env.is_empty() {
                    let mut env_inline = toml_edit::InlineTable::new();
                    for (k, v) in &env {
                        env_inline.insert(k, toml_edit::Value::from(v.as_str()));
                    }
                    tbl.insert("env",
                        toml_edit::Item::Value(toml_edit::Value::InlineTable(env_inline)));
                }
                arr.push(tbl);
            }
            tokio::fs::write(plugins_path, doc.to_string()).await?;
            let config = PluginConfig {
                id:      name.clone(),
                cmd:     command,
                args:    vec![],
                env:     if env.is_empty() { None } else { Some(env) },
                cwd:     None,
                restart: RestartPolicy::Always,
            };
            sv_cmd_tx.send(SupervisorCmd::SpawnPlugin { config }).await
                .map_err(|_| anyhow::anyhow!("supervisor channel closed"))?;
            eprintln!("[evolution] registered MCP server '{name}'");
            Ok(format!("registered MCP server '{name}'"))
        }

        EvolutionProposal::UnregisterMcpServer { name, reason: _ } => {
            let toml_text = tokio::fs::read_to_string(plugins_path).await?;
            let mut doc = toml_text.parse::<toml_edit::DocumentMut>()?;
            if let Some(arr) = doc.get_mut("plugin").and_then(|v| v.as_array_of_tables_mut()) {
                let idx = (0..arr.len()).find(|&i| {
                    arr.get(i)
                        .and_then(|t| t.get("id"))
                        .and_then(|v| v.as_str()) == Some(name.as_str())
                });
                if let Some(i) = idx { arr.remove(i); }
            }
            tokio::fs::write(plugins_path, doc.to_string()).await?;
            sv_cmd_tx.send(SupervisorCmd::KillPlugin { id: PluginId(name.clone()) }).await
                .map_err(|_| anyhow::anyhow!("supervisor channel closed"))?;
            eprintln!("[evolution] unregistered MCP server '{name}'");
            Ok(format!("unregistered MCP server '{name}'"))
        }

        EvolutionProposal::HotReloadSubsystem { subsystem } => {
            match subsystem {
                Subsystem::Agent => {
                    let content = tokio::fs::read_to_string(soul_path).await.unwrap_or_default();
                    *soul_arc.write().await = content;
                    eprintln!("[evolution] agent system prompt reloaded from disk");
                    Ok("reloaded agent system prompt from disk".into())
                }
                Subsystem::Policy => {
                    let new_config = PolicyConfig::load(policy_path)?;
                    *policy_arc.write().await = PolicyEngine::new(new_config);
                    eprintln!("[evolution] policy reloaded from disk");
                    Ok("reloaded policy from disk".into())
                }
                Subsystem::Plugins => {
                    Ok("plugins hot-reload: use register_mcp_server / unregister_mcp_server \
                        for individual plugins".into())
                }
                Subsystem::Gateway => {
                    Ok("gateway hot-reload not supported without daemon restart".into())
                }
            }
        }
    }
}

// ── agent router ──────────────────────────────────────────────────────────────

fn spawn_agent_router(
    mut rx:        broadcast::Receiver<Event>,
    bcast:         broadcast::Sender<Event>,
    bus:           apexos_core::BusHandle,
    tool_reg:      Arc<RwLock<HashMap<PluginId, Vec<ToolSpec>>>>,
    histories:     Arc<Mutex<HashMap<SessionId, Vec<Message>>>>,
    engine:        Arc<TurnEngine>,
    max_depth:     u32,
    session_store: Arc<SessionStore>,
) {
    // Per-session abort handles and parent-child tree for cascade cancellation.
    let abort_handles    = Arc::new(Mutex::new(HashMap::<SessionId, AbortHandle>::new()));
    let session_children = Arc::new(Mutex::new(HashMap::<SessionId, Vec<SessionId>>::new()));
    let session_depths   = Arc::new(Mutex::new(HashMap::<SessionId, u32>::new()));
    // Internal session IDs use the top half of u64 to avoid collisions with
    // frontend-assigned IDs (which come in via UserPrompt).
    let next_child_id    = Arc::new(AtomicU64::new(1u64 << 63));

    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                // ── new root turn ────────────────────────────────────────────
                Ok(Event::UserPrompt { session, text }) => {
                    session_depths.lock().await.entry(session).or_insert(0);

                    let user_msg = Message::User {
                        content: vec![ContentBlock::Text { text }],
                    };
                    let mut hist = histories.lock().await;
                    let history  = hist.entry(session).or_default();
                    history.push(user_msg.clone());
                    let snapshot     = history.clone();
                    let snapshot_len = snapshot.len();
                    drop(hist);

                    // Persist user message immediately.
                    {
                        let store = Arc::clone(&session_store);
                        tokio::spawn(async move { store.append(session, &user_msg).await; });
                    }

                    let tools  = gather_tools(&tool_reg).await;
                    let handle = tokio::spawn(root_turn(
                        session, snapshot,
                        bus.clone(), bcast.clone(), tools, engine.clone(),
                        histories.clone(), Arc::clone(&session_store), snapshot_len,
                    ));
                    abort_handles.lock().await.insert(session, handle.abort_handle());
                }

                // ── sub-agent spawn ──────────────────────────────────────────
                Ok(Event::SpawnAgent { parent, call_id, prompt, system }) => {
                    let parent_depth = *session_depths.lock().await
                        .get(&parent).unwrap_or(&0);

                    if parent_depth >= max_depth {
                        let b = bus.clone();
                        tokio::spawn(async move {
                            b.emit(Event::ToolResult {
                                session: parent,
                                call:    call_id,
                                output:  ToolOutput {
                                    ok:      false,
                                    content: serde_json::json!("max sub-agent depth exceeded"),
                                },
                            }).await;
                        });
                        continue;
                    }

                    let child_id = SessionId(next_child_id.fetch_add(1, Ordering::SeqCst));
                    session_depths.lock().await.insert(child_id, parent_depth + 1);
                    session_children.lock().await
                        .entry(parent).or_default().push(child_id);

                    let child_history = vec![Message::User {
                        content: vec![ContentBlock::Text { text: prompt }],
                    }];
                    histories.lock().await.insert(child_id, child_history.clone());

                    let child_engine = Arc::new(engine.with_system(system));
                    let tools        = gather_tools(&tool_reg).await;

                    let handle = tokio::spawn(child_turn(
                        child_id, child_history,
                        bus.clone(), bcast.clone(), tools, child_engine,
                        histories.clone(), parent, call_id,
                    ));
                    abort_handles.lock().await.insert(child_id, handle.abort_handle());
                }

                // ── cancellation ─────────────────────────────────────────────
                Ok(Event::UserCancel { session }) => {
                    cascade_cancel(session, &session_children, &abort_handles).await;
                }

                // ── tool registry updates ────────────────────────────────────
                Ok(Event::PluginUp   { plugin, tools }) => {
                    tool_reg.write().await.insert(plugin, tools);
                }
                Ok(Event::PluginDown { plugin, .. }) => {
                    tool_reg.write().await.remove(&plugin);
                }

                // ── sensor events ────────────────────────────────────────────
                Ok(Event::SensorReading { node_id, reading, timestamp: _ }) => {
                    // Threshold check: fire a turn if a critical condition is seen.
                    let alert: Option<String> = match &reading {
                        SensorReading::Temperature { celsius, sensor_id } if *celsius > 85.0 => {
                            Some(format!(
                                "[sensor alert] {node_id}/{sensor_id} CPU temperature critical: {celsius:.1}°C — please investigate"
                            ))
                        }
                        SensorReading::Motion { detected: true, sensor_id } => {
                            Some(format!(
                                "[sensor alert] {node_id}/{sensor_id} motion detected"
                            ))
                        }
                        SensorReading::AirQuality { iaq, accuracy, sensor_id, .. } if *iaq > 150.0 && *accuracy >= 2 => {
                            Some(format!(
                                "[sensor alert] {node_id}/{sensor_id} air quality degraded: IAQ {iaq:.0} (accuracy {accuracy}/3) — consider ventilating"
                            ))
                        }
                        _ => None,
                    };
                    if let Some(prompt) = alert {
                        let root = SessionId(0);
                        session_depths.lock().await.entry(root).or_insert(0);
                        bus.emit(Event::UserPrompt { session: root, text: prompt }).await;
                    }
                }

                Ok(_) => {}
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        }
    });
}

// ── turn task helpers ─────────────────────────────────────────────────────────

async fn root_turn(
    session:       SessionId,
    history:       Vec<Message>,
    bus:           apexos_core::BusHandle,
    bcast:         broadcast::Sender<Event>,
    tools:         Vec<ToolSpec>,
    engine:        Arc<TurnEngine>,
    histories:     Arc<Mutex<HashMap<SessionId, Vec<Message>>>>,
    session_store: Arc<SessionStore>,
    snapshot_len:  usize,
) {
    match run_turn(session, history, bus.clone(), bcast, tools, engine).await {
        Ok(updated) => {
            // Persist the assistant messages added during this turn.
            if updated.len() > snapshot_len {
                let delta: Vec<Message> = updated[snapshot_len..].to_vec();
                let store = session_store.clone();
                tokio::spawn(async move {
                    for msg in &delta { store.append(session, msg).await; }
                });
            }
            histories.lock().await.insert(session, updated);
        }
        Err(e) => {
            eprintln!("[agent:{:?}] turn error: {e}", session);
            // Always unblock the frontend — emit error then TurnComplete.
            bus.emit(Event::Error { session: Some(session), message: e.to_string() }).await;
            bus.emit(Event::TurnComplete { session }).await;
        }
    }
}

async fn child_turn(
    child_id:  SessionId,
    history:   Vec<Message>,
    bus:       apexos_core::BusHandle,
    bcast:     broadcast::Sender<Event>,
    tools:     Vec<ToolSpec>,
    engine:    Arc<TurnEngine>,
    histories: Arc<Mutex<HashMap<SessionId, Vec<Message>>>>,
    parent:    SessionId,
    call_id:   ActionId,
) {
    let output = match run_turn(child_id, history, bus.clone(), bcast, tools, engine).await {
        Ok(updated) => {
            let text = extract_final_text(&updated);
            histories.lock().await.insert(child_id, updated);
            ToolOutput { ok: true, content: serde_json::json!(text) }
        }
        Err(e) => ToolOutput { ok: false, content: serde_json::json!(e.to_string()) },
    };
    // Route child output back as a ToolResult so parent's collect_tool_results unblocks.
    bus.emit(Event::ToolResult { session: parent, call: call_id, output }).await;
}

// ── utilities ─────────────────────────────────────────────────────────────────

/// Gather all plugin tools and inject the synthetic virtual tools.
async fn gather_tools(
    tool_reg: &Arc<RwLock<HashMap<PluginId, Vec<ToolSpec>>>>,
) -> Vec<ToolSpec> {
    let mut tools: Vec<ToolSpec> = tool_reg.read().await
        .values()
        .flatten()
        .cloned()
        .collect();
    tools.push(agent_spawn_spec());
    tools.push(read_soul_md_spec());
    tools.push(propose_evolution_spec());
    tools.push(rollback_evolution_spec());
    tools.push(schedule_task_spec());
    tools.push(list_schedules_spec());
    tools.push(cancel_schedule_spec());
    tools
}

fn agent_spawn_spec() -> ToolSpec {
    ToolSpec {
        name:        "agent_spawn".into(),
        description: "Spawn a focused sub-agent to handle a sub-task. \
                      Returns the sub-agent's final text output.".into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "prompt": {
                    "type":        "string",
                    "description": "The task for the sub-agent to perform."
                },
                "system": {
                    "type":        "string",
                    "description": "Optional system prompt override for the sub-agent."
                }
            },
            "required": ["prompt"]
        }),
    }
}

fn read_soul_md_spec() -> ToolSpec {
    ToolSpec {
        name:        "read_soul_md".into(),
        description: "Read the current live content of /etc/agentd/soul.md (your system prompt). \
                      ALWAYS call this before propose_evolution with kind=update_system_prompt \
                      so you work from the current content, not your in-context snapshot.".into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {},
            "required": []
        }),
    }
}

fn propose_evolution_spec() -> ToolSpec {
    ToolSpec {
        name:        "propose_evolution".into(),
        description: "Propose a structural change to agentd: register or remove an MCP plugin, \
                      update a policy rule, update your own system prompt (soul.md), or \
                      hot-reload a subsystem. Every proposal is recorded as an event and \
                      applied immediately (gated by the evolution.* policy rule).".into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "kind": {
                    "type": "string",
                    "enum": [
                        "register_mcp_server",
                        "unregister_mcp_server",
                        "update_policy_rule",
                        "update_system_prompt",
                        "hot_reload_subsystem"
                    ],
                    "description": "The type of evolution to propose."
                },
                "name": {
                    "type":        "string",
                    "description": "Plugin name (register_mcp_server / unregister_mcp_server)."
                },
                "command": {
                    "type":        "string",
                    "description": "Shell command to start the MCP server (register_mcp_server)."
                },
                "env": {
                    "type":        "object",
                    "description": "Environment variables for the MCP server (register_mcp_server)."
                },
                "tool_pattern": {
                    "type":        "string",
                    "description": "Exact tool name or wildcard 'prefix.*' (update_policy_rule)."
                },
                "new_mode": {
                    "type":        "string",
                    "enum":        ["suggest", "auto-edit", "yolo"],
                    "description": "New approval mode (update_policy_rule)."
                },
                "content": {
                    "type":        "string",
                    "description": "Full replacement text for /etc/agentd/soul.md (update_system_prompt). \
                                    Call read_soul_md first to get the current content before editing."
                },
                "subsystem": {
                    "type":        "string",
                    "enum":        ["plugins", "policy", "agent", "gateway"],
                    "description": "Subsystem to reload in-place (hot_reload_subsystem)."
                },
                "reason": {
                    "type":        "string",
                    "description": "Why this change is being proposed."
                }
            },
            "required": ["kind", "reason"]
        }),
    }
}

fn rollback_evolution_spec() -> ToolSpec {
    ToolSpec {
        name:        "rollback_evolution".into(),
        description: "Revert a previously applied evolution by its ID. \
                      Uses the in-memory undo snapshot taken at apply time. \
                      Only available for evolutions applied in the current daemon session.".into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "evolution_id": {
                    "type":        "integer",
                    "description": "The numeric ID of the evolution to roll back (from EvolutionApplied event)."
                },
                "reason": {
                    "type":        "string",
                    "description": "Why this rollback is being requested."
                }
            },
            "required": ["evolution_id", "reason"]
        }),
    }
}

fn schedule_task_spec() -> ToolSpec {
    ToolSpec {
        name:        "schedule_task".into(),
        description: "Schedule a recurring task using a cron expression. The agent will autonomously \
                      send the given prompt as a new turn at each scheduled time. Use standard 6-field \
                      cron syntax: second minute hour day month weekday.".into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "cron": {
                    "type":        "string",
                    "description": "6-field cron expression, e.g. '0 0 8 * * *' = 8am daily."
                },
                "prompt": {
                    "type":        "string",
                    "description": "The message to send as a new autonomous turn."
                },
                "session_id": {
                    "type":        "integer",
                    "description": "Session to fire in (optional — defaults to root session 0)."
                }
            },
            "required": ["cron", "prompt"]
        }),
    }
}

fn list_schedules_spec() -> ToolSpec {
    ToolSpec {
        name:        "list_schedules".into(),
        description: "List all active scheduled tasks with their IDs, cron expressions, prompts, \
                      and last run times.".into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {},
            "required": []
        }),
    }
}

fn cancel_schedule_spec() -> ToolSpec {
    ToolSpec {
        name:        "cancel_schedule".into(),
        description: "Cancel a scheduled task by its ID. The task is removed immediately and \
                      will not fire again.".into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "schedule_id": {
                    "type":        "string",
                    "description": "The schedule ID returned by schedule_task."
                }
            },
            "required": ["schedule_id"]
        }),
    }
}

fn extract_final_text(history: &[Message]) -> String {
    history.iter().rev()
        .find_map(|m| match m {
            Message::Assistant { content } => {
                let text: String = content.iter()
                    .filter_map(|b| match b {
                        ContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("");
                if text.is_empty() { None } else { Some(text) }
            }
            _ => None,
        })
        .unwrap_or_default()
}

async fn cascade_cancel(
    session:          SessionId,
    session_children: &Arc<Mutex<HashMap<SessionId, Vec<SessionId>>>>,
    abort_handles:    &Arc<Mutex<HashMap<SessionId, AbortHandle>>>,
) {
    // Walk the subtree breadth-first.
    let mut to_cancel = vec![session];
    let children = session_children.lock().await;
    let mut i = 0;
    while i < to_cancel.len() {
        if let Some(ch) = children.get(&to_cancel[i]) {
            to_cancel.extend_from_slice(ch);
        }
        i += 1;
    }
    drop(children);

    let mut handles = abort_handles.lock().await;
    for s in &to_cancel {
        if let Some(h) = handles.remove(s) {
            h.abort();
            eprintln!("[agent:{:?}] cancelled", s);
        }
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use apexos_core::{ContentBlock, Message};

    #[test]
    fn extract_final_text_gets_last_assistant_text() {
        let history = vec![
            Message::User      { content: vec![ContentBlock::Text { text: "hi".into() }] },
            Message::Assistant { content: vec![ContentBlock::Text { text: "hello".into() }] },
            Message::User      { content: vec![ContentBlock::Text { text: "more".into() }] },
            Message::Assistant { content: vec![
                ContentBlock::Thinking { thinking: "...".into(), signature: "sig".into() },
                ContentBlock::Text     { text: "final answer".into() },
            ]},
        ];
        assert_eq!(extract_final_text(&history), "final answer");
    }

    #[test]
    fn extract_final_text_skips_non_text_blocks() {
        let history = vec![
            Message::Assistant { content: vec![
                ContentBlock::Thinking { thinking: "hmm".into(), signature: "s".into() },
            ]},
            Message::Assistant { content: vec![
                ContentBlock::Text { text: "result".into() },
            ]},
        ];
        assert_eq!(extract_final_text(&history), "result");
    }

    #[test]
    fn agent_spawn_spec_has_required_prompt() {
        let spec = agent_spawn_spec();
        assert_eq!(spec.name, "agent_spawn");
        let required = spec.input_schema["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v.as_str() == Some("prompt")));
    }

    #[test]
    fn propose_evolution_spec_has_required_fields() {
        let spec = propose_evolution_spec();
        assert_eq!(spec.name, "propose_evolution");
        let required = spec.input_schema["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v.as_str() == Some("kind")));
        assert!(required.iter().any(|v| v.as_str() == Some("reason")));
    }
}
