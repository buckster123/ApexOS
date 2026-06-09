# ApexOS — Architecture

> A headless, agent-first OS layer. The agent runtime is the primary system
> service. Any display is a thin, stateless renderer. The Pi 5 is the control
> plane, never the compute plane.

---

## The core inversion

Traditional embedded OS: hardware → OS → applications → optional agent bolted on.

ApexOS: hardware → `agentd` daemon → agents use the OS → optional display renders state.

The display (local KVM via Cage/Wayland, or any browser on the network) holds
zero state and sends only intents. It is a rendering terminal, not an application.
This means the daemon is the single source of truth and multi-client sync is trivial.

---

## Three planes

```
┌─────────────────────────────────────────────────────────┐
│  PRESENTATION (stateless renderers)                      │
│  cage kiosk · browser · mobile PWA · future CLI          │
│  → sends intents (user_prompt, user_approval, hello)     │
│  ← receives event stream over WebSocket                  │
└────────────────────────┬────────────────────────────────┘
                         │ WebSocket (JSON events)
┌────────────────────────▼────────────────────────────────┐
│  ORCHESTRATION  —  agentd (Rust single binary)           │
│                                                          │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌────────┐  │
│  │ Gateway  │  │  Agent   │  │ Plugins  │  │ Store  │  │
│  │ (axum WS)│  │ (turn    │  │ (MCP     │  │ (JSONL │  │
│  │          │  │  engine) │  │  stdio)  │  │  log)  │  │
│  └────┬─────┘  └────┬─────┘  └────┬─────┘  └────┬───┘  │
│       └─────────────┴─────────────┴──────────────┘      │
│                    tokio broadcast Bus                    │
└────────────────────────┬────────────────────────────────┘
                         │ HTTP/stdio
┌────────────────────────▼────────────────────────────────┐
│  INFERENCE  (remote, swappable)                          │
│  Anthropic API · Ollama · vLLM · OpenRouter              │
│  hot-swappable at runtime — no restart needed            │
└─────────────────────────────────────────────────────────┘
```

---

## The event bus

Everything flows as `Event` on a `tokio::broadcast` channel. The `core` crate
defines `Event` and `SystemState`; the pure `apply(event) → state` function is
unit-tested with zero I/O. Every other crate is wired to the bus.

```
WebSocket intent arrives
    │
    ▼
Gateway deserializes → emits Event::UserPrompt on bus
    │
    ▼
Agent router receives → starts turn → calls inference API
    │
    ▼
Agent emits Event::ToolRequested
    │
    ▼
Policy engine checks rule → may emit Event::ApprovalPending
    │
    ▼ (approved / yolo)
Supervisor dispatches to MCP plugin → result arrives
    │
    ▼
Agent emits Event::ToolResult → inference continues
    │
    ▼
Agent emits Event::AssistantMessage
    │
Every emitted Event:
  → Store appends to date-rolling JSONL log (NVMe)
  → Gateway forwards to all connected WebSocket clients
  → State actor folds event into SystemState
```

No two subsystems talk directly. Everything is events.

---

## Codebase map

```
agentd/                         Cargo workspace — the daemon
  crates/
    core/                       Zero-I/O — Event enum, SystemState, apply()
    agentd/                     Binary: main() wires everything together
      src/
        main.rs                 Startup, GatewayState construction, task spawning
        scheduler.rs            schedule_task virtual tool — cron-driven UserPrompt
        council_handler.rs      Council virtual tool dispatch
    gateway/                    axum WebSocket server + all HTTP API routes
      src/lib.rs                Router, all handlers (~1800 lines)
      tests/echo.rs             Integration tests (bus round-trip, broadcast)
    agent/                      Turn engine: streaming, thinking retention, semaphore
      src/
        turn.rs                 Main agent loop
        provider.rs             AnthropicProvider + OaiProvider + RoutingProvider
        council.rs              CouncilEngine — parallel personas, synthesis
    plugins/                    MCP-over-stdio client + supervisor
      src/
        supervisor.rs           Plugin lifecycle + ALL virtual tool dispatch
        vast.rs                 VastState — GPU rental lifecycle
        policy.rs               PolicyEngine — suggest/auto-edit/yolo × per-tool rules
        mesh.rs                 Peer registry + avahi discovery
    store/                      Append-only JSONL event log, date-rolling files
  config/
    plugins.toml                Plugin declarations (deployed to /etc/agentd/)
    policy.toml                 Approval rules (deployed to /etc/agentd/)
    soul.md                     Agent system prompt (deployed to /etc/agentd/)
    recipes.toml                Vast.ai GPU recipes (deployed to /etc/agentd/)

tools/                          Separate Cargo workspace — MCP plugin binaries
  crates/
    apexos-tools/               Shell + fs + http + sysstat + audio tools
      src/tools.rs              All 18 tool implementations

ui/                             Frontend (no build step — vanilla JS)
  desktop.html                  Desktop OS skin — WinBox windows
  desktop-app.js                Desktop logic (~2700 lines)
  desktop-style.css             Desktop styles
  index.html                    CLI/terminal skin
  app.js                        CLI logic
  mobile.html                   Mobile PWA
  lib/                          WinBox 0.2.82, Alpine.js, xterm.js, Monaco IDE

deploy/                         Systemd units + install scripts
  agentd.service
  cage-kiosk.service
  apex-wake.service             Wake-word listener
  apex-wake.py                  Whisper-based wake word daemon
  apexos.avahi.service          mDNS advertisement
  setup-voice.sh                Voice I/O setup (ffmpeg, whisper.cpp, piper)

docs/
  claude/                       AI context docs — lazy-loaded per subsystem
  archive/                      Pre-implementation planning (historical)
  images/                       Banner and press assets
  install.md                    User installation guide
```

---

## Subsystems in detail

### Gateway (`agentd/crates/gateway/src/lib.rs`)

Single axum router. Routes:
- `/ws` — WebSocket: intents in, full event stream out
- `/sensor-bridge` — Authenticated WS for sensor Pi connections
- `/terminal-ws` — PTY-backed terminal (openpty + libc)
- `/api/*` — REST for UI: sessions, soul, policy, models, backend, audio, vast, mesh, etc.
- Fallback: static file server for `ui/`

**GatewayState** is a single `Clone`-able struct shared across all handlers via `axum::extract::State`.

### Agent turn engine (`agentd/crates/agent/`)

Semaphore-capped (one active turn per session). Streaming. Thinking-block retention
across turns (Anthropic requirement for multi-turn extended thinking). Providers:
- `AnthropicProvider` — native Anthropic API, streaming, thinking-block aware
- `OaiProvider` — OpenAI-compatible REST (Ollama / vLLM / OpenRouter)
- `RoutingProvider` — reads `backend_arc` per-call, enabling zero-restart hot-swap

### Plugin supervisor (`agentd/crates/plugins/src/supervisor.rs`)

Manages MCP plugin subprocesses (JSON-RPC 2.0 over stdio). Also handles all
**virtual tools** — tools implemented natively in Rust that look like MCP tools to
the agent: `propose_evolution`, `agent_spawn`, `schedule_task`, `convene_council`,
`vast_launch`, `send_to_agent`, `query_event_log`, `bootstrap_node`, etc.

When a new tool needs to be added:
1. Add a `_spec()` function returning the JSON schema
2. Add the spec to `gather_tools()` in `main.rs`
3. Add the handler in `supervisor.rs` under the `tools/call` match

### Policy engine (`agentd/crates/plugins/src/policy.rs`)

Three modes: `suggest` (emit ApprovalPending, wait for user), `auto-edit` (auto-approve
non-destructive ops), `yolo` (approve everything). Per-tool overrides in `policy.toml`.
Approval is an event — it round-trips through the bus so all clients see it.

### MCP plugin system

Plugins are declared in `plugins.toml` and speak JSON-RPC 2.0 over stdin/stdout.
CerebroCortex (Python, memory system) and apexos-tools (Rust, system tools) both run
as MCP plugins. Adding a new plugin: write the binary, add it to `plugins.toml`.

### Inference backends

Set via `AGENTD_BACKEND` env var or `POST /api/backend`. Hot-swappable at runtime.
The `RoutingProvider` reads the `backend_arc` on each turn, so switching is instant.
Vast.ai GPU rental (`VastState`) auto-swaps the backend when a GPU instance is ready
and reverts when destroyed.

---

## Key patterns for contributors

### Adding a new MCP tool (to `apexos-tools`)

Edit `tools/crates/apexos-tools/src/tools.rs`:
1. Add tool spec JSON to `list()` array
2. Add match arm to `call()`
3. Implement the function (sync, shells out to subprocess or uses stdlib)

Build: `cd tools && cargo build --release`
Deploy: `sudo cp target/release/apexos-tools /usr/local/bin/apexos-tools`
No agentd restart needed — supervisor restarts the plugin process automatically.

### Adding a gateway API route

Edit `agentd/crates/gateway/src/lib.rs`:
1. Add route to `router()` fn
2. Implement handler (async fn, returns `impl IntoResponse`)
3. Add required body/query structs with `#[derive(Deserialize)]`

### Adding a UI window

In `ui/desktop.html`: add button to start menu + window content div (`#win-{id}-content`)
In `ui/desktop-app.js`: add entry to `WIN_DEFAULTS`, add init/stop hooks in `openWin()`/`onclose`
In `ui/desktop-style.css`: add window and component styles

### Adding a virtual tool

In `agentd/crates/agentd/src/main.rs`: add `{name}_spec()` function + call in `gather_tools()`
In `agentd/crates/plugins/src/supervisor.rs`: add handler in the `tools/call` dispatch block

---

## Locked architectural decisions

These were made deliberately and should not be revisited without a strong reason.

| Decision | Rationale |
|----------|-----------|
| Rust single binary | Low memory footprint on Pi 5, simple deploy, no runtime deps |
| tokio broadcast bus (not channels) | All subscribers see all events — UI, logger, and multi-agent all work without explicit fan-out |
| MCP-over-stdio for plugins | CerebroCortex and other Python/any-language plugins run unmodified |
| Approval as event | Approval requests are bus citizens — multi-client sync and event log are automatic |
| `parent: Option<SessionId>` for multi-agent | Sub-agents are sessions with a parent link; no architecture change needed |
| Vanilla JS frontend (no build step) | Zero toolchain friction on the Pi; `sudo cp ui/* /var/lib/agentd/ui/` is the deploy |
| Pi is control plane only | Inference is remote; Pi orchestrates, cloud thinks. GPU workloads rent via Vast.ai |

---

## Data persistence

| Data | Location | Format |
|------|----------|--------|
| Event log | `/var/lib/agentd/events/YYYY-MM-DD.jsonl` | Append-only JSONL |
| Session histories | `/var/lib/agentd/sessions/{id}.jsonl` | JSONL per session |
| Scheduled tasks | `/var/lib/agentd/schedules.jsonl` | JSONL |
| Agent memory | CerebroCortex (`/var/lib/agentd/cerebro/`) | Vector DB + graph |
| Council sessions | `/var/lib/agentd/council/{id}.jsonl` | JSONL |
| Vast.ai instance | `/var/lib/agentd/workspace/vast/instance.json` | JSON |
| Audio workspace | `/var/lib/agentd/workspace/sonus/` | MP3/WAV files |
| Config | `/etc/agentd/` | TOML + Markdown |

---

## Multi-agent model

```
agentd root session (main agent loop)
  │
  ├── SubAgent A  (spawned via agent_spawn tool)
  │     │  parent = root session id
  │     │  output streams to root as ToolResult
  │     └── SubAgent A1  (further nesting supported)
  │
  └── SubAgent B
        cross-node: send_to_agent { node: "other-pi", session: id, text }
        → HTTP POST to peer's /api/sessions/{id}/message
```

Council sessions are a separate parallel construct — N ephemeral agents synthesized
into one result, not a parent/child tree.
