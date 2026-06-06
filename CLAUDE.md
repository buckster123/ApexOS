# ApexOS — Agent-first OS daemon for Raspberry Pi 5

## What this is
Headless, agent-first OS layer. `agentd` (Rust single binary) is the primary
system service; any display (local KVM via cage, or network browser) is a
thin stateless renderer. Inference is cloud (Anthropic API). Pi 5 = control
plane, not compute plane.

## Repo layout
```
agentd/           Cargo workspace (binary + 5 library crates)
  crates/
    core/         Event types, SystemState, apply() — ZERO I/O, pure, unit-testable
    agentd/       Binary: wires everything, owns main()
    gateway/      axum WebSocket server — intents in, state out
    agent/        Turn engine: streaming, thinking-block retention, semaphore
    plugins/      MCP-over-stdio client + subprocess supervisor + registry
    store/        Append-only event log to NVMe
  config/         plugins.toml + policy.toml (examples, deployed to /etc/agentd/)
  deploy/         systemd units: agentd.service + cage-kiosk.service
docs/reference/   Reference Rust snippets from the handoff package (read-only)
docs/claude/      Lazy-loaded sub-MDs (see ## Docs below)
plans/            Architecture docs and handoff package
```

## Build order (each step independently testable)
~~1.~~ ✓ `core` — types + unit-tested `state.apply()` (6 tests, 0 failures)
~~2.~~ ✓ Bus + trivial echo frontend — end-to-end WS round-trip proven (2 integration tests)
~~3.~~ ✓ Plugin supervisor — MCP-over-stdio, CerebroCortex wired (66 tools, recall tested)
~~4.~~ ✓ Agent turn engine — Provider trait, SSE streaming, thinking-block retention, semaphore, bus wiring (6 tests)
~~5.~~ ✓ Policy engine — suggest/auto-edit/yolo × per-tool rules, ApprovalPending/UserApproval flow (12 tests)
~~6.~~ ✓ Sub-agent routing — agent_spawn virtual tool, child→parent ToolResult routing, cascade cancel (3 tests)
~~7.~~ ✓ Pi deploy — agentd running on Pi 5, CerebroCortex 0.5.1 in venv, full WS round-trip smoke-tested

## Locked decisions (do NOT re-litigate)
- Language: Rust (single-binary deploy, low memory next to CerebroCortex)
- Architecture: actor model on tokio broadcast bus; one `Event` type flows on bus, log, and WebSocket
- Plugins: MCP-over-stdio (CerebroCortex runs unmodified)
- Approval: suggest / auto-edit / yolo modes × per-tool rules; approval is an event
- Multi-agent: `parent: Option<SessionId>` on `AgentContext`; no architecture change

## Deferred to keyboard (do not guess)
- Hot-reload mechanics
- Event-log format (lean JSONL v1)
- tokio task-shutdown ordering

## Resolved locked items
- **MCP framing**: newline-delimited JSON, protocol `"2024-11-05"`. Real entrypoint: `/home/andre/Projects/CerebroCortex/cerebro-mcp`

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
- `agentd` user (unprivileged), cage kiosk on tty1 for local KVM display (deferred)
- SSH: `ssh apexos@192.168.0.114` — password `abnudc1337` (local LAN only)
- `ANTHROPIC_API_KEY` in `/etc/agentd/env` (chmod 600, root-owned)
- CerebroCortex 0.5.1 installed in `/opt/cerebro-venv`; wrapper at `/usr/local/bin/cerebro-mcp`
- Always build on Pi (`~/.cargo/bin/cargo build --release`) — do NOT scp x86 binaries
- Pi data: `/var/lib/agentd/{workspace,events,cerebro}`

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
- **Pushing to remote is always manually triggered by André.** Never push
  automatically, never suggest it unless asked.
- **Local git is the floor of resilience.** Cerebro holds session memory;
  git holds code truth. If Cerebro is unavailable, the repo + these docs
  are enough to reconstruct full project context.
