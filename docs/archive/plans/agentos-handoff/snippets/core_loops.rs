// Agent OS — the three core loops (reference shapes, not final code).
// These show the SHAPE and the load-bearing details. Fill in against the
// real Pi + CerebroCortex in Claude Code.

// ============================================================================
// LOOP 1 — THE STATE ACTOR (the hub)
// crates/core/src/bus.rs
// ============================================================================
//
// Every task talks to this and ONLY this. It owns canonical state and a
// broadcast channel that fans every event out to all subscribers
// (frontends via the gateway, the log writer, a CerebroCortex bridge).

use tokio::sync::{broadcast, mpsc};
use crate::types::Event;

pub struct Bus {
    inbox:    mpsc::Receiver<Event>,       // everything flows IN here
    outbound: broadcast::Sender<Event>,    // ...and fans OUT to subscribers
    state:    SystemState,                 // the one source of truth
}

/// Handle other tasks use to talk to the bus. Cheap to clone.
#[derive(Clone)]
pub struct BusHandle {
    tx: mpsc::Sender<Event>,
}

impl BusHandle {
    pub async fn emit(&self, e: Event) {
        // In production: handle send error (bus gone = shutting down).
        let _ = self.tx.send(e).await;
    }
}

impl Bus {
    pub fn new(state: SystemState) -> (Self, BusHandle, broadcast::Sender<Event>) {
        let (tx, inbox) = mpsc::channel(1024);
        let (outbound, _) = broadcast::channel(1024);
        let handle = BusHandle { tx };
        let bus = Self { inbox, outbound: outbound.clone(), state };
        (bus, handle, outbound)
    }

    pub async fn run(mut self) {
        while let Some(event) = self.inbox.recv().await {
            // 1. fold into canonical state (pure fn — see state_apply.rs)
            self.state.apply(&event);
            // 2. re-broadcast to every subscriber
            let _ = self.outbound.send(event);
        }
    }
}

// Subscribers do: `let mut rx = outbound.subscribe();` then `while let Ok(e) = rx.recv().await { ... }`
// - gateway: forward to the right WebSocket(s)
// - store:   append to the NVMe event log
// - cerebro: pick out memory-worthy events and persist


// ============================================================================
// LOOP 2 — THE AGENT TURN ENGINE
// crates/agent/src/turn.rs
// ============================================================================
//
// Consumes a UserPrompt, streams from the cloud, emits text + tool requests.
//
// TWO load-bearing details:
//  (a) THINKING BLOCK RETENTION — the assistant turn appended to history must
//      include thinking blocks, or the API rejects the next turn in a
//      tool-use loop. (This is the 4.7 migration footgun.)
//  (b) SEMAPHORE — a permit is acquired before each stream so heavy sub-agent
//      fan-out can't open unbounded concurrent API connections.

use std::sync::Arc;
use tokio::sync::Semaphore;

pub struct InferenceClient {
    http: reqwest::Client,
    sem:  Arc<Semaphore>,   // global cap on in-flight API calls
}

pub async fn run_turn(
    session: SessionId,
    mut history: Vec<Message>,
    bus:    BusHandle,
    tools:  ToolRegistry,      // snapshot of currently-registered capabilities
    api:    Arc<InferenceClient>,
) -> anyhow::Result<()> {
    loop {
        // (b) bound concurrency — released when _permit drops at loop end
        let _permit = api.sem.acquire().await?;

        let mut stream = api.messages_stream(&history, &tools).await?;
        let mut assistant_blocks: Vec<ContentBlock> = Vec::new();
        let mut pending_tools: Vec<ToolCall> = Vec::new();

        while let Some(chunk) = stream.next().await {
            match chunk? {
                Chunk::TextDelta(t) => {
                    bus.emit(Event::AgentText { session, delta: t.clone() }).await;
                    push_text(&mut assistant_blocks, t);
                }
                Chunk::ThinkingDelta(t) => {
                    bus.emit(Event::AgentThinking { session, delta: t.clone() }).await;
                    push_thinking(&mut assistant_blocks, t); // (a) keep it
                }
                Chunk::ToolUse(call) => {
                    assistant_blocks.push(ContentBlock::ToolUse {
                        id: call.id_string(), name: call.tool.clone(), input: call.args.clone(),
                    });
                    pending_tools.push(call);
                }
                Chunk::Done => break,
            }
        }

        // (a) commit the FULL assistant turn — text + thinking + tool_use
        history.push(Message::Assistant { content: assistant_blocks });

        if pending_tools.is_empty() {
            bus.emit(Event::TurnComplete { session }).await;
            return Ok(());
        }

        // Emit ToolRequested for each tool, then await matching ToolResult
        // events off the bus. Append tool results to history as a User turn,
        // then loop. (Approval is handled by the policy engine BEFORE dispatch;
        // the turn engine just waits for results.)
        for call in &pending_tools {
            bus.emit(Event::ToolRequested { session, call: call.clone() }).await;
        }
        let results = await_tool_results(&bus, session, &pending_tools).await?;
        history.push(Message::User { content: results }); // tool_result blocks
        // permit drops here → reacquired at top of loop for the next round-trip
    }
}


// ============================================================================
// LOOP 3 — THE PLUGIN SUPERVISOR  (TOP-TIER ITEM)
// crates/plugins/src/supervisor.rs
// ============================================================================
//
// Contract: a plugin is ANY process that speaks MCP over stdio.
// Supervisor spawns it, does the MCP handshake, registers its advertised
// tools into the capability registry, and watches for death.
// CerebroCortex needs ZERO modification.

use tokio::process::{Child, Command};
use std::process::Stdio;

pub struct Plugin {
    pub id:     PluginId,
    pub child:  Child,
    pub client: McpClient,   // stdio JSON-RPC framing
}

pub struct Supervisor {
    bus:     BusHandle,
    plugins: std::collections::HashMap<PluginId, Plugin>,
}

impl Supervisor {
    /// Boot a plugin from config, perform MCP handshake, register tools.
    pub async fn spawn(&mut self, spec: &PluginConfig) -> anyhow::Result<()> {
        let mut child = Command::new(&spec.cmd)
            .args(&spec.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())     // capture for diagnostics
            .kill_on_drop(true)         // don't leak subprocesses
            .spawn()?;

        // MCP: initialize handshake, then enumerate tools.
        let mut client = McpClient::attach(&mut child).await?;
        client.initialize().await?;                 // MCP `initialize`
        let tools: Vec<ToolSpec> = client.list_tools().await?;

        self.bus.emit(Event::PluginUp { plugin: spec.id.clone(), tools }).await;

        // Supervise: spawn a task watching child.wait(); on exit emit
        // PluginDown and apply restart policy (always | on-failure | never).
        self.plugins.insert(spec.id.clone(), Plugin { id: spec.id.clone(), child, client });
        Ok(())
    }

    /// Route a tool call to whichever plugin owns that tool, await its result.
    pub async fn dispatch(&mut self, session: SessionId, call: ToolCall) -> anyhow::Result<()> {
        let plugin = self.owner_of(&call.tool)?;   // registry lookup
        let output = plugin.client.call_tool(&call.tool, &call.args).await?;
        self.bus.emit(Event::ToolResult { session, call: call.id, output }).await;
        Ok(())
    }
}

// HOT-RELOAD NOTE: build `spawn`/`despawn` so they can run against a live
// Supervisor without dropping active sessions. Boot-time spawning is the v1
// floor; finalize hot-reload at the keyboard with the daemon running.

// agent.spawn (sub-agent creation) is just another tool the policy gates and
// the supervisor — or the agent task manager — handles: it creates a child
// AgentContext (parent = current session) and kicks off run_turn for it.
