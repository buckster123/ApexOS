# Agent OS — Masterplan

> A headless, agent-first operating layer for a bare Raspberry Pi 5 + NVMe.
> The desktop is gone. The agent is the primary user. Any display — local KVM
> or network browser — is a thin view into the agent's world.

**Status:** architecture locked, ready for implementation in Claude Code with the Pi live.
**Target hardware (v1):** Raspberry Pi 5, NVMe drive, RaspiOS Lite (no desktop). No Hailo, no NPU, no add-ons — those are pure additions later.
**Inference:** cloud (Anthropic API). The Pi is a *control plane*, never a compute plane.

---

## 1. Core thesis

Instead of bolting an agent onto a desktop designed for human hands and eyes, we invert: the **agent runtime is the primary system service**, and the GUI is a secondary, stateless renderer. This single inversion is what makes "boot to browser OR boot to attached monitor" a transport detail rather than two separate builds.

Three planes, decoupled, talking over one internal event bus:

- **Presentation** — stateless renderers (KVM kiosk, network browser, future CLI/mobile). Subscribe to a state stream, send intents back. Hold no authoritative state.
- **Orchestration** — the `agentd` daemon. Source of truth. Owns state, the agent loop, plugin supervision, and policy.
- **Inference** — cloud Anthropic API. The Pi orchestrates; the cloud thinks.

Because inference is remote, the always-on daemon is *light* — it marshals events, manages subprocesses, and streams UI. This is what lets it run comfortably on a Pi 5 alongside a resident CerebroCortex instance without bandwidth/memory contention.

---

## 2. The load-bearing decisions (locked)

These are settled and should not be re-litigated during implementation. They were chosen deliberately and each one buys a specific property.

| Decision | Choice | Why |
|---|---|---|
| **Daemon language** | Rust | Single-binary deploy (scp one file + a unit), tiny memory ceiling next to CerebroCortex, safety for a long-lived PID-1-adjacent service. |
| **Internal architecture** | Actor model on a `tokio` bus | Subsystems never call each other directly — they emit/consume `Event`s through one central state actor. Same "dumb subscriber" property at the task level that frontends have at the process level. One mental model, two scales. |
| **Frontend ↔ daemon** | Stateless renderers; intents in, state out | Authority stays in one place. A buggy/malicious frontend can only *ask*, never command. Makes multi-frontend free. |
| **Local display** | `cage` (single-app Wayland kiosk) → fullscreen webview at `localhost` | KVM display renders the *same* web frontend the network sees. GUI built once. |
| **Plugins** | Separate processes speaking **MCP over stdio** | Polyglot by construction. CerebroCortex (Python) runs *unmodified*. The boundary is a protocol, not an API. |
| **Persistence** | Append-only event log on NVMe | The same `Event` enum that flows on the bus is what's logged AND what's pushed to frontends. One type, three uses. Natural ingestion point for CerebroCortex memory. Free audit trail. |
| **Multi-agent** | A routing rule, not an architecture | Every session is one `AgentContext`. A parent link decides whether output routes up (to parent) or out (to frontend). Spawning a sub-agent is a policy-gated tool call; its result is a tool result. |
| **Approvals** | CC-style modes × declarative per-tool rules, approval-as-event | `suggest` / `auto-edit` / `yolo`. yolo (air-gap full-send) is pure config — no separate code path. Every approval is an event → full audit log. |

---

## 3. Workspace layout

```
agentd/
├── Cargo.toml                # workspace
├── crates/
│   ├── agentd/               # the binary — wires everything, owns main()
│   ├── core/                 # Event types, state model, the bus — ZERO I/O
│   ├── gateway/              # axum WebSocket server, intent parsing
│   ├── agent/                # the turn engine + Anthropic client
│   ├── plugins/              # MCP client, subprocess supervisor, registry
│   └── store/                # event-log persistence to NVMe
├── config/
│   ├── plugins.toml          # declared plugins (see §7)
│   └── policy.toml           # approval policy (see §6)
└── deploy/
    ├── agentd.service        # systemd unit for the daemon
    └── cage-kiosk.service    # systemd unit for the local display
```

**`core` has zero I/O on purpose** — pure types + state-transition logic, trivially unit-testable, nothing in it needs a runtime to reason about. All async lives in leaf crates.

---

## 4. The load-bearing types

Two enums and one struct carry the whole system. Get these right; the rest is mechanical. See `snippets/core_types.rs` for the full version with all variants.

The critical property: **`Event` is `Serialize`.** The same enum flows on the internal bus, gets written to the NVMe log, and is pushed to frontends over WebSocket. CerebroCortex ingestion is "subscribe to this stream and persist what's interesting" — no separate instrumentation.

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    // from frontends (intents)
    UserPrompt    { session: SessionId, text: String },
    UserApproval  { session: SessionId, action: ActionId, granted: bool },
    UserCancel    { session: SessionId },
    // from the agent loop
    AgentText     { session: SessionId, delta: String },
    AgentThinking { session: SessionId, delta: String },
    ToolRequested { session: SessionId, call: ToolCall },
    TurnComplete  { session: SessionId },
    // from the plugin supervisor
    ToolResult    { session: SessionId, call: ActionId, output: ToolOutput },
    PluginUp      { plugin: PluginId, tools: Vec<ToolSpec> },
    PluginDown    { plugin: PluginId, reason: String },
    // policy
    ApprovalPending { session: SessionId, call: ToolCall },
    // system
    Error         { session: Option<SessionId>, message: String },
}
```

```rust
pub struct AgentContext {
    pub id:      SessionId,
    pub parent:  Option<SessionId>,   // None = root (talks to frontend). THE multi-agent switch.
    pub history: Vec<Message>,
    pub spawned: Vec<SessionId>,      // children, for cancellation cascade
}
```

---

## 5. The three core loops

### Loop 1 — the state actor (the hub)
Every task talks to this and only this. Owns canonical state + a broadcast channel that fans events to all subscribers (frontends, log writer, CerebroCortex).

```rust
pub struct Bus {
    inbox: mpsc::Receiver<Event>,        // everything flows IN
    outbound: broadcast::Sender<Event>,  // ...fans OUT to all subscribers
    state: SystemState,                  // the one source of truth
}

impl Bus {
    pub async fn run(mut self) {
        while let Some(event) = self.inbox.recv().await {
            self.state.apply(&event);        // pure fn in `core`, unit-testable
            let _ = self.outbound.send(event);
        }
    }
}
```

### Loop 2 — the agent turn engine
Consumes `UserPrompt`, streams from the cloud, emits text + tool requests.
**Footgun handled here:** assistant *thinking blocks* must be retained across tool round-trips or the API rejects the continuation (this is the 4.7 migration bug). The turn engine appends the assistant turn *including thinking blocks* + tool results before looping.
**Concurrency control:** a `tokio::sync::Semaphore` on the inference client caps simultaneous in-flight API calls so a fan-out of N sub-agents doesn't open N connections and hit rate limits.

### Loop 3 — the plugin supervisor (TOP-TIER ITEM)
Contract: **a plugin is any process that speaks MCP over stdio.** Supervisor spawns it, does the MCP handshake, registers advertised tools into the capability registry, watches for death, applies restart policy. CerebroCortex needs zero modification.

Full code shapes for all three are in `snippets/core_loops.rs`.

---

## 6. Approval / safety model

Model on AI CLI harnesses (CC etc.). A global **mode** crossed with **per-tool rules**.

**Modes:**
- `suggest` — every tool call needs approval (safe default). ≈ CC `default`.
- `auto-edit` — reads/edits auto-approve; mutations outside workspace or command execution ask. ≈ CC `acceptEdits`.
- `yolo` — nothing asks. Air-gapped full-send. ≈ CC `bypassPermissions`.

**Mechanism:** approval is an **event on the bus**, not an inline call. Agent emits `ToolRequested` → policy engine inspects → either dispatches immediately OR emits `ApprovalPending` → frontend renders prompt → user's answer returns as `UserApproval` intent.

Payoffs: yolo is pure config (policy auto-approves everything, no code-path difference); every approval is logged (full audit trail of what ran + who approved); sub-agent spawn is itself a gated action.

```toml
# config/policy.toml
mode = "suggest"          # suggest | auto-edit | yolo

[rules]
"fs.read"     = "allow"           # never asks
"fs.write"    = "workspace"       # auto if inside workspace, else ask
"shell.exec"  = "ask"             # always asks (in suggest/auto-edit)
"cerebro.*"   = "allow"           # memory ops are safe
"net.*"       = "ask"
"agent.spawn" = "ask"             # gate sub-agent fan-out
```

`yolo` short-circuits the table. Air-gapped users flip one line.

---

## 7. Plugin system

Plugins declared in a config file read at boot. Adding a capability = add a block (or future `agentctl plugin add`).

```toml
# config/plugins.toml
[[plugin]]
id   = "cerebro"
cmd  = "python"
args = ["-m", "cerebrocortex.mcp"]
restart = "always"

[[plugin]]
id   = "shell"
cmd  = "/usr/lib/agentd/plugins/shell-mcp"   # bundled Rust binary
restart = "on-failure"
```

Because the boundary is MCP-over-stdio, any language works and CerebroCortex runs as-is.

**Hot-reload:** build the supervisor *reload-ready* (it should be able to spawn into a running daemon without dropping active sessions), but boot-time spawning is the acceptable v1 floor. Proving hot-reload wants the running daemon, so finalize at the keyboard.

---

## 8. Multi-agent (full sub-agent support)

Hierarchical sub-agents: an agent can spawn child agents, hand them a scoped task, and consume their results as tool calls. **Spawning a sub-agent is a tool call; a sub-agent's final answer is a tool result.** A child is just a session whose output routes to its parent instead of a frontend.

The entire hierarchy reduces to the `parent: Option<SessionId>` field on `AgentContext`:
- `parent == None` → root, output streams to frontend.
- `parent == Some(id)` → child, `TurnComplete` becomes a `ToolResult` in the parent's history.

The agent loop does **not** change — a sub-agent runs the identical `run_turn`. The state actor's `HashMap<SessionId, AgentContext>` holds the whole tree flat; parent links reconstruct hierarchy.

Falls out for free:
- **Policy-gated spawning** — `agent.spawn` is a tool; cap depth, gate fan-out, or let it rip in yolo.
- **Cancellation cascade** — `UserCancel` on a parent walks `spawned` and cancels the subtree.
- **Observability** — sub-agent activity is on the same event log; replay/inspect any child.

The one genuinely new requirement: the **semaphore** on the inference client (see Loop 2) to bound concurrent API calls under heavy fan-out.

---

## 9. Process topology

`systemd` (PID 1) supervises two units:
- **`agentd.service`** — the Rust daemon (single binary). Inside it, tokio tasks: WS gateway, agent loop, plugin supervisor, state store. Spawns plugin subprocesses over stdio; connects out to the cloud API over HTTPS.
- **`cage-kiosk.service`** — fullscreen webview → `localhost`, renders the same frontend on the attached KVM display. Waits for `agentd` to be up.

Unit files in `deploy/`. See `snippets/systemd/`.

---

## 10. Build order for Claude Code

Each step is independently testable — keeps CC sessions tight.

1. **`core` crate** — types + a unit-tested `state.apply()`. No async, no I/O. (See `snippets/state_apply.rs` for the transition spec.)
2. **Bus + trivial echo frontend** — prove the event flow end to end.
3. **Plugin supervisor against real CerebroCortex** — the top-tier item; validate MCP framing against the actual binary.
4. **Agent turn engine** — streaming, thinking-block retention, semaphore.
5. **Policy engine** — modes, rules table, approval-as-event.
6. **Sub-agent routing** — the `parent` field + routing branch + cancellation cascade.

---

## 11. Decisions deferred to the keyboard (do NOT guess these now)

- **Exact MCP framing** against the real CerebroCortex MCP entrypoint.
- **Hot-reload mechanics** — supervisor built reload-ready, but proving it needs a running daemon.
- **Event-log format** — plain JSONL vs SQLite vs `sled`. Depends on how CerebroCortex wants to read it. Lean JSONL for v1 simplicity + greppability unless CerebroCortex prefers structured queries.
- **tokio task-shutdown ordering** — always fiddlier in practice than on paper; finalize live.

---

## 12. First CC session checklist

- [ ] Pi 5 on RaspiOS Lite, updated, SSH enabled. *(André is prepping this.)*
- [ ] NVMe mounted, workspace path chosen.
- [ ] Point CC at the repo + the CerebroCortex MCP entrypoint command.
- [ ] `cargo new` the workspace per §3.
- [ ] Start at build-order step 1.
