# Agent OS — Claude Code handoff package

Everything needed to continue building Agent OS in Claude Code with the Pi live.
Drop this folder into your working dir and point CC at it.

## What this is

A headless, agent-first OS layer for a **bare Raspberry Pi 5 + NVMe** running
**RaspiOS Lite** (no desktop). The agent is the primary user; any display —
local KVM or network browser — is a thin, stateless view. Inference is
**cloud (Anthropic API)**; the Pi is a control plane, not a compute plane.

## Read in this order

1. **`MASTERPLAN.md`** — the primary reference. Thesis, locked decisions,
   crate layout, the three core loops, approval model, plugin system,
   multi-agent design, process topology, build order, and the list of things
   deliberately deferred to the keyboard. **Start here.**

2. **`snippets/core_types.rs`** — the load-bearing types (`Event`,
   `ToolCall`, `AgentContext`, `Message`). The whole system rests on these.

3. **`snippets/state_apply.rs`** — `SystemState` + the pure `apply()` transition
   fn, with unit tests. This IS build-order step 1: make the tests pass.

4. **`snippets/core_loops.rs`** — reference shapes for the three async loops:
   the state actor (bus), the agent turn engine (thinking-block retention +
   concurrency semaphore), and the plugin supervisor (MCP-over-stdio).

5. **`snippets/config/`** — `plugins.toml` and `policy.toml` examples.

6. **`snippets/systemd/`** — `agentd.service` and `cage-kiosk.service` units.

## Architecture in one paragraph

A Rust single-binary daemon (`agentd`) under systemd, paired with a `cage`
kiosk for the local display. Internally it's an actor system on a `tokio` bus:
a central state actor owns canonical state and an append-only event log on
NVMe, and fans every event out to subscribers. Around it sit four task
subsystems — WS gateway (dumb frontends; intents in, state out), agent turn
engine (streaming, thinking-block-safe, semaphore-capped), plugin supervisor
(MCP over stdio; CerebroCortex runs unmodified), and the policy engine
(suggest/auto-edit/yolo, declarative rules, approval-as-event). Multi-agent is
a routing rule: every session is one `AgentContext`, and a `parent` link
decides whether output streams out to a frontend or up to a parent as a tool
result. Everything — UI, persistence, memory ingestion, audit, multi-agent
observability — rides the one event stream.

## Build order (each step independently testable)

1. `core` crate — types + unit-tested `state.apply()`  (see `state_apply.rs`)
2. Bus + a trivial echo frontend — prove event flow end to end
3. Plugin supervisor against real CerebroCortex — the top-tier item
4. Agent turn engine — streaming, thinking retention, semaphore
5. Policy engine — modes, rules, approval-as-event
6. Sub-agent routing — `parent` field + routing branch + cancel cascade

## Deferred to the keyboard (don't guess these)

- Exact MCP framing vs the real CerebroCortex entrypoint
- Hot-reload mechanics (build reload-ready; prove with daemon running)
- Event-log format (lean JSONL for v1 unless CerebroCortex wants structured queries)
- tokio task-shutdown ordering

## First session checklist

- [ ] Pi 5 on RaspiOS Lite, updated, SSH enabled
- [ ] NVMe mounted, workspace path chosen
- [ ] Confirm the CerebroCortex MCP entrypoint command
- [ ] `cargo new` the workspace per MASTERPLAN §3
- [ ] Start at build-order step 1
