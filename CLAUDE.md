# ApexOS — Agent-first OS daemon for Raspberry Pi 5

## What this is
Headless, agent-first OS layer. `agentd` (Rust single binary) is the primary
system service; any display (local KVM via cage, or network browser) is a
thin stateless renderer. Inference is multi-backend: Anthropic (default),
Ollama (local or cloud models e.g. `nemotron-3-ultra:cloud`), vllm, OpenRouter.
Pi 5 = control plane, not compute plane.

## Repo layout
```
agentd/           Cargo workspace (binary + 5 library crates)
  crates/
    core/         Event types, SystemState, apply() — ZERO I/O, pure, unit-testable
    agentd/       Binary: wires everything, owns main()
    gateway/      axum WebSocket server — intents in, state out
    agent/        Turn engine: streaming, thinking-block retention, semaphore; OaiProvider + RoutingProvider + CouncilEngine
    plugins/      MCP-over-stdio client + subprocess supervisor + registry
    store/        Append-only JSONL event log to NVMe (date-rolling files)
  config/         plugins.toml + policy.toml (examples, deployed to /etc/agentd/)
  deploy/         systemd units: agentd.service + cage-kiosk.service
docs/reference/   Reference Rust snippets from the handoff package (read-only)
docs/claude/      Lazy-loaded sub-MDs (see ## Docs below)
plans/            Architecture docs and handoff package
ui/               Frontend — CLI skin (index.html/style.css/app.js) + Desktop skin (desktop.html/desktop-style.css/desktop-app.js) + lib/ (WinBox+Alpine+xterm.js, no CDN)
tools/            Separate Cargo workspace for MCP plugins
  crates/
    apexos-tools/ Shell + fs + http + sysstat MCP server (deployed to /usr/local/bin/)
```

## Build order (each step independently testable)
~~1.~~ ✓ `core` — types + unit-tested `state.apply()` (6 tests, 0 failures)
~~2.~~ ✓ Bus + trivial echo frontend — end-to-end WS round-trip proven (2 integration tests)
~~3.~~ ✓ Plugin supervisor — MCP-over-stdio, CerebroCortex wired (66 tools, recall tested)
~~4.~~ ✓ Agent turn engine — Provider trait, SSE streaming, thinking-block retention, semaphore, bus wiring (6 tests)
~~5.~~ ✓ Policy engine — suggest/auto-edit/yolo × per-tool rules, ApprovalPending/UserApproval flow (12 tests)
~~6.~~ ✓ Sub-agent routing — agent_spawn virtual tool, child→parent ToolResult routing, cascade cancel (3 tests)
~~7.~~ ✓ Pi deploy — agentd running on Pi 5, CerebroCortex 0.5.1 in venv, full WS round-trip smoke-tested
~~8.~~ ✓ Event log — `store` crate, JSONL per day to `AGENTD_LOG`, date-roll, lag-safe (2 tests)
~~9.~~ ✓ Frontend UI — vanilla HTML/CSS/JS in `ui/`; custom `tokio::fs` handler (ServeDir fails in `ProtectSystem=strict`); terminal aesthetic, boot animation, streaming text, collapsible tool calls, inline approval UX; API key entry dialogue
~~10.~~ ✓ Cage kiosk — seatd backend, `agentos-kiosk` user (video group), `cage-kiosk.service` auto-starts; Wayland socket live; headless-safe (3 retries then stops)
~~11.~~ ✓ Frontend controls — cancel (Esc), power modal (reboot/shutdown + 3s countdown), model selector (live Arc swap), policy badge, new session (Ctrl+K + localStorage history), timestamps, collapse-all tools
~~12.~~ ✓ Self-evolution — `propose_evolution` / `rollback_evolution` / `read_soul_md` virtual tools; live apply of UpdateSystemPrompt/UpdatePolicyRule/RegisterMcpServer/HotReload; Cerebro episode wrapping (memory_store→episode_add_step); frontend evolution modal + stats; dogfood verified
~~13.~~ ✓ Session persistence + multi-client sync — `session_store` crate (append-only JSONL per session); server-issued session IDs (AtomicU64, survives restart); WS hello/session_init handshake; history replay with full thinking-block context; session picker modal (Ctrl+Shift+S); cage kiosk + browser share live session; verified: agent quotes prior messages verbatim after F5
~~14.~~ ✓ Agent bodies Phase 6a — `apexos-tools` MCP server (`tools/` workspace); 11 tools: run_command (shell denylist), read_file, write_file, list_dir, create_dir, delete_path, http_fetch, cpu_temp, disk_usage, memory_info, uptime; deployed to Pi, supervisor confirmed `plugin 'apexos-tools' up — 11 tools`
~~15.~~ ✓ Agent bodies Phase 6b — `schedule_task` / `list_schedules` / `cancel_schedule` virtual tools; cron-driven autonomous turns; JSONL persistence at `schedules.jsonl`; 60s poll loop fires `UserPrompt` on bus
~~16.~~ ✓ Agent bodies Phase 6c — `notify` tool in `apexos-tools`; surfaces: JSONL log (always) + notify-send toast + espeak-ng TTS + ntfy.sh (env-gated) + Telegram (env-gated); agentd added to audio group; espeak-ng installed; smoke: 3/3 local surfaces fired clean
~~17.~~ ✓ Agent bodies Phase 6d — `SensorReading` enum + `Event::SensorReading` in core; `/sensor-bridge` WS endpoint; `apex-sensor-bridge` HTTP-polls SensorHead (BME688 BSEC2 + MLX90640 thermal); `AirQuality` + `ThermalFrame` variants; `sensorhead-dashboard.service`; IAQ > 150 alert routing; smoke: IAQ/T/RH/P + thermal frame flowing live in agentd every 30s; `sensor-head-mcp` proxy plugin (8 pull-mode tools); `plugin 'sensor-head' up — 8 tools` confirmed
~~18.~~ ✓ Desktop skin Phase A — WinBox 0.2.82 + Alpine.js windowed OS shell; `desktop.html` + `desktop-app.js` + `desktop-style.css`; `ui/lib/` bundled (no CDN); topbar with live model+policy selectors, clock; dock with 5 app buttons; agent/sensors/cerebro/sensorhead/settings windows; lazy iframe loading for Cerebro (8767) + SensorHead (8080); Alpine settings (soul editor, policy mode, plugin list); `/api/soul` GET+POST + `/api/policy` POST in gateway; `cerebro-api.service` at boot (port 8767)
~~19.~~ ✓ Desktop skin Phase B — Win7-style start menu + dynamic taskbar; `!cmd` passthrough; terminal window (xterm.js); camera window (`/api/snapshot` → rpicam-jpeg 1280×720, night mode flag); thermal canvas wallpaper (32×24 noise grid, blue→red colour map, 13% opacity, live from ThermalFrame events); `/api/policy/rules` GET (reads live PolicyEngine, unlocks settings policy tab); fixed policy.toml TOML parse bug (sensor rules were in [subagents] with unquoted values)
~~20.~~ ✓ Desktop skin Phase C/D — wallpaper picker (thermal/logo/minimal, localStorage); notes window (localStorage + server persist); browser window (iframe + URL bar); sketchpad (HTML5 canvas, pen/eraser, PNG download); file explorer (two-pane, lazy tree via `find -printf`, upload via base64, open-in-notes/IDE); Monaco IDE 0.55.1 (tarball, no npm, Ctrl+S save, lang auto-detect, New/Upload); sub-agent windows (`SubAgentStarted` event on core bus, dynamic WinBox per child session, streaming output + ✓ done badge)
~~21.~~ ✓ Sonus plugin Phase E — `hermes-sonus` 2.0.0 MCP server (17 tools: generate_song, check_status, download_track, extend_track, lyrics, voice clone, album batch, etc.); `/opt/sonus-venv` + `/usr/local/bin/sonus-mcp` wrapper; `SUNO_DOWNLOAD_DIR=/var/lib/agentd/workspace/sonus`; gateway `GET /api/sonus/files` + `GET /api/sonus/stream` (HTTP 206 range requests, full seek); desktop 🎵 Player window; 103 total tools live on Pi
~~22.~~ ✓ Real PTY terminal — `GET /terminal-ws` WebSocket; pure libc `openpty`+`setsid`+`TIOCSCTTY`; two threads bridge blocking PTY I/O to tokio async; binary frames to xterm.js; resize via `TIOCSWINSZ`; full interactive shell (vim, htop, tab-complete, colours); `nix` crate removed, `libc = "0.2"` only
~~23.~~ ✓ Sub-agent window v2 — `tool_requested`/`tool_result`/`approval_pending` routed to child WinBox; `toolBlocks: Map` per watched session for DOM lookup by call_id; `textEl` resets on each tool so text+tools interleave; approval buttons send `user_approval` with child session id; frontend-only
~~24.~~ ✓ Home/Dashboard — 3-card layout (SYSTEM: CPU temp+RAM+disk bars; ENVIRONMENT: IAQ badge+env stats+thermal mini-canvas; AGENT: model/policy/evo stats/plugin dots) + recent sessions; polls `/api/run` shell cmds every 6s; reuses `sensorState`/`iaqLabel`/`iaqColor` from `app.js`; frontend-only
~~25.~~ ✓ Voice I/O — STT: `POST /api/transcribe` (raw WebM → ffmpeg 16kHz mono WAV → whisper.cpp → JSON text); TTS: `POST /api/speak` (text → piper if `PIPER_MODEL` set else espeak-ng); 🎤 mic button (MediaRecorder, pulsing red while recording, ⏳ while transcribing) + 🔊 speaker toggle (auto-reads agent turns when on); `deploy/setup-voice.sh` installs ffmpeg+whisper.cpp+tiny.en+piper+lessac-medium on Pi
~~26.~~ ✓ Wake word — `deploy/apex-wake.py` + `apex-wake.service`; 3s ALSA chunks → whisper.cpp base.en → POST `/api/wake`; `WakeTriggered` event on bus; piper "yes?" ding → 7s server-side record → auto-submit transcript; hallucination filter for silence artefacts; Ctrl+Space manual trigger; base.en (148MB) chosen for proper-noun accuracy; env-configurable via WAKE_PHRASE/WAKE_COOLDOWN/ALSA_CAPTURE_DEVICE
~~27.~~ ✓ Multi-inference backend — `OaiProvider` (OAI-compatible REST: Ollama/vllm/OpenRouter) + `RoutingProvider` (reads `backend_arc` per-call, zero-restart hot-swap); `AGENTD_BACKEND` / `AGENTD_OAI_BASE_URL` / `AGENTD_MODEL` env vars; `GET /api/models` proxies live model list; `GET/POST /api/backend` live-swaps backend+URL; desktop + CLI backend `<select>` (ANTHROPIC/OLLAMA/VLLM/OPENROUTER) + URL input (slides in for local backends, green highlight); model selector fetches dynamically; separate `oai_api_key` arc (OAI_API_KEY / OPENROUTER_API_KEY env vars, `/var/lib/agentd/.oai_api_key` persist); `GET/POST /api/keys` — set Anthropic + OAI keys independently at runtime; desktop Settings → Keys tab; Ollama installed on Pi with `nemotron-3-ultra:cloud` (550B, tools+thinking, zero local storage) — full agentic loop smoke-tested: tool call → policy → MCP → result → streamed response ✓
~~28.~~ ✓ Council engine — `CouncilAgentDef` + 7 `Event` variants in core; `council.rs` in `agent` crate: ephemeral per-agent `AnthropicProvider`/`OaiProvider`, `tokio::spawn` parallel rounds, keyword convergence, synthesis; native personas AZOTH/VAJRA/ELYSIAN/KETHER; `convene_council` virtual tool via `SupervisorCmd::SetCouncilTx` + `council_handler.rs`; `CouncilButtInMap`/`CouncilSessionsMap` shared between handler + gateway; gateway routes `POST/GET /api/council`, `GET /api/council/{id}`, `POST /api/council/{id}/butt-in`; desktop Council Launcher (topic/agent/rounds config) + dynamic Council Chamber WinBox per session (per-agent streaming columns, convergence bar, butt-in input); all 6 council events dispatched in `app.js`; 18 tests, 0 warnings
~~29.~~ ✓ Council persistence + Cerebro hook — broadcast-subscriber JSONL writer task per council session → `$AGENTD_LOG/council/<id>.jsonl` (start/round_start/agent_done/round_done/complete events); post-synthesis `memory_store` to Cerebro via `ToolProxy` (tagged `council`+`apexos`); best-effort, never fails the council
~~30.~~ ✓ A2A messaging — `send_to_agent` virtual tool; `AgentMessage`+`AgentMessageAck` in core; agent router injects as `UserPrompt` on target session; `POST /api/sessions/{id}/message` gateway route; sub-agent window inbox display (`subagent-inbox-msg`); fire-and-forget, no blocking
~~31.~~ ✓ RAG over event log — `query_event_log` virtual tool (hours/types/max params); reads date-rolling JSONL files from events_dir, skips streaming noise, formats each event as a readable sentence; `GET /api/events/recent?hours=24&types=...` gateway route for external consumers; agents can now answer "what happened today?" or batch-store events to Cerebro for semantic search
~~32.~~ ✓ Sensor anomaly wakeup — per-type cooldown (default 30 min, `SENSOR_ALERT_COOLDOWN_SECS`); configurable thresholds (`SENSOR_IAQ_THRESHOLD`, `SENSOR_CPU_TEMP_THRESHOLD`, `SENSOR_THERMAL_THRESHOLD`); `ThermalFrame` hotspot detection added; agent router fires `UserPrompt` on threshold crossing, suppressed until cooldown expires; no-spam guaranteed
~~33.~~ ✓ Event log timeline — desktop window over `GET /api/events/recent`; time range (1h→7d) + type filter + auto-refresh (30s); reverse-chrono list; 16 event types with color-coded badges; sensor readings formatted by kind; frontend-only
~~34.~~ ✓ Mobile PWA companion — `ui/mobile.html` served at `/mobile`; `manifest.json` enables "Add to home screen" on Android Chrome → fullscreen standalone; touch-optimized chat UI (full-height single-column, message bubbles, pinned input bar); voice: server-side ALSA primary + browser MediaRecorder fallback; speaker toggle auto-speaks agent turns; inline approval cards (tap APPROVE/DENY); collapsible tool call cards; wake word handler; API key entry overlay; gateway `/mobile` route + `manifest.json` content-type; no new Rust crates, no WinBox/desktop chrome

## Locked decisions (do NOT re-litigate)
- Language: Rust (single-binary deploy, low memory next to CerebroCortex)
- Architecture: actor model on tokio broadcast bus; one `Event` type flows on bus, log, and WebSocket
- Plugins: MCP-over-stdio (CerebroCortex runs unmodified)
- Approval: suggest / auto-edit / yolo modes × per-tool rules; approval is an event
- Multi-agent: `parent: Option<SessionId>` on `AgentContext`; no architecture change

## Deferred to keyboard (do not guess)
- Event-log format (lean JSONL v1)
- tokio task-shutdown ordering

## Resolved locked items
- **MCP framing**: newline-delimited JSON, protocol `"2024-11-05"`. Real entrypoint: `/home/andre/Projects/CerebroCortex/cerebro-mcp`
- **Hot-reload mechanics**: `SupervisorCmd::{SpawnPlugin,KillPlugin,HotReload}` via `cmd_tx()`; policy is `Arc<RwLock<PolicyEngine>>` (write-swap for live reload); soul.md is `Arc<RwLock<String>>`. Pi config files writable by `agentd` user (`chown agentd:agentd /etc/agentd/{soul.md,policy.toml,plugins.toml}`).
- **Desktop UI script order**: `xterm.min.js` + `xterm-addon-fit.min.js` MUST load before `monaco/vs/loader.js`. Monaco installs a global AMD `define()` that hijacks xterm's UMD bundle, leaving `window.Terminal` = undefined. Any future AMD-compatible lib must also load before Monaco loader.
- **Multi-backend env vars**: `AGENTD_BACKEND` (anthropic|ollama|vllm|openrouter, default: anthropic), `AGENTD_OAI_BASE_URL` (default: `http://localhost:11434/v1`), `AGENTD_MODEL` (default: per-backend). All hot-swappable via `POST /api/backend` at runtime — no restart needed.

## Key files
- `docs/reference/core_types.rs` — load-bearing types (Event, ToolCall, AgentContext, Message)
- `docs/reference/state_apply.rs` — SystemState + pure apply() + unit tests → build-order step 1
- `docs/reference/core_loops.rs` — reference shapes for all three async loops
- `agentd/config/policy.toml` — approval model config
- `agentd/config/plugins.toml` — plugin declarations
- `plans/agentos-handoff/MASTERPLAN.md` — full architecture reference

## CerebroCortex
Memory system runs as MCP-over-stdio plugin. In dev, it's at:
`/home/andre/Projects/CerebroCortex/cerebro-mcp`
In production on Pi: declared in `agentd/config/plugins.toml`.
Agent ID for this dev session: **CLAUDE-APEX**

## Pi 5 target
- Debian trixie (not RaspiOS), NVMe `/dev/sda2` 458GB for workspace + event log
- `agentd` user (unprivileged), `agentos-kiosk` user for cage kiosk (video/render/input/tty groups)
- SSH: `ssh apexos@192.168.0.158` — password `abnudc1337` (local LAN only) [sensor Pi, USB boot; 8GB RAM, better cooling]
- `ANTHROPIC_API_KEY` in `/etc/agentd/env` (chmod 600, root-owned)
- CerebroCortex 0.5.1 installed in `/opt/cerebro-venv`; wrapper at `/usr/local/bin/cerebro-mcp`
- Always build on Pi (`~/.cargo/bin/cargo build --release`) — do NOT scp x86 binaries
- Pi data: `/var/lib/agentd/{workspace,events,cerebro,ui}`
- **Binary update**: stop service before copying — `systemctl stop agentd` then `cp`, then `start` (running binary gives "text file busy")
- **Deploy workflow**: commit → push → `git pull` on Pi → `cargo build --release` on Pi → stop/cp/start agentd → `sudo cp ui/* /var/lib/agentd/ui/` (use absolute paths inside sudo, `~` expands to root)

## Docs
Sub-specialization docs in `docs/claude/`. Load the relevant one when entering
that subsystem — do not load all of them by default.

| File | Load when working on |
|------|----------------------|
| `docs/claude/core-types.md` | `core` crate — Event enum, AgentContext, SystemState, apply() |
| `docs/claude/plugin-supervisor.md` | `plugins` crate — MCP-over-stdio, subprocess lifecycle, CerebroCortex wiring |
| `docs/claude/agent-turn-engine.md` | `agent` crate — streaming, thinking-block retention, Anthropic API, semaphore |
| `docs/claude/policy-engine.md` | `agentd` policy layer — approval modes, rules table, approval-as-event |
| `docs/claude/multi-agent.md` | sub-agent routing — parent/child sessions, fan-out, cancellation cascade |
| `docs/claude/pi-deploy.md` | Pi 5 target — RaspiOS setup, NVMe, systemd, cage kiosk, first deploy |
| `docs/claude/gateway.md` | `gateway` crate — WebSocket server, intent protocol, state stream |
| `docs/claude/mesh.md` | multi-Pi mesh — mDNS discovery, peer registry, bootstrap_node tool, cross-node A2A |

---

## Meta (static — do not modify this section)

**This file is self-evolving.** Update it whenever the project state it describes
becomes stale. Rules:

### When to update CLAUDE.md
- A locked decision changes or a new one is made → update `## Locked decisions`
- A build-order step completes → mark it done (e.g. `~~1.~~  ✓`) and note what was learned
- Workspace layout changes (crate added/removed, folder restructured) → update `## Repo layout`
- A "deferred to keyboard" item gets resolved → move it out of that section with the answer
- Pi target hardware or OS changes → update `## Pi 5 target`
- CerebroCortex entrypoint or path changes → update `## CerebroCortex`

### When to update a sub-MD (not this file)
- Discovering an API quirk, footgun, or non-obvious pattern in a subsystem
- Completing a build step — record what the reference snippet needed to change, test results
- Any detail that's too long for this file but belongs to a specific crate or deployment topic

### What never goes here or in sub-MDs
- Task progress, session logs, completed-work summaries → use Cerebro (`remember` / `session_save`)
- Git SHAs, PR numbers, version pins → stale in days, belong in git history
- Inline commentary on what you just did → belongs in commit messages

### Format rules
- Keep this file under ~120 lines of content (excluding this Meta section)
- Prefer tables and bullets over prose
- Sub-MDs can be as long as needed; this file stays lean
- Sub-MDs use the same format: short header, tables/bullets, no session logs

### Git discipline (always follow these)
- **Tests pass → commit immediately.** Don't accumulate work. Each build-order step
  completion = at minimum one commit.
- **Docs travel with code.** Update the relevant `docs/claude/*.md` status checklist
  and notes in the same commit as the code it describes. CLAUDE.md + sub-MDs are
  first-class version-controlled artifacts.
- **Commit message format:** imperative, lowercase, descriptive.
  `implement core Event types and state.apply() with unit tests`
  `wire plugin supervisor MCP handshake against CerebroCortex`
  `fix thinking-block retention in agent turn loop`
- **Never amend a commit that already has later commits on top of it.**
- **Never force-push.** Local history is the safety net — protect it.
- **Push after every successful commit.** Tests pass → commit → push. No manual trigger needed for this repo.
- **Local git is the floor of resilience.** Cerebro holds session memory;
  git holds code truth. If Cerebro is unavailable, the repo + these docs
  are enough to reconstruct full project context.
