# ApexOS Self-Evolution — Product Requirements

**Status:** Phase 0 ready — spike can start immediately  
**Owner:** André (buckster123)  
**Reference agent ID:** CLAUDE-APEX  
**Target:** Additive self-evolution layer on top of the existing agentd architecture. Zero changes to locked decisions.  
**Inspired by:** Hermes Agent's skill system and self-improving loop (MIT). Not a port — the mechanics are adapted to fit ApexOS's event-bus architecture and CerebroCortex memory system.

---

## 1. Vision

ApexOS should accumulate capability over time without manual code changes:

- The agent discovers a new workflow, edge case, or optimization.
- It persists that knowledge as a **procedural memory** in CerebroCortex.
- Future turns automatically retrieve and apply relevant procedures.
- The agent can propose **structural changes** to its own config, plugin set, system prompt, or policy rules — all flowing through the existing policy engine (suggest/auto-edit/yolo).
- Over weeks/months the daemon becomes measurably better at André's specific environment.

Key property (preserved from Hermes): **evolution is opt-in, auditable, policy-gated, and never surprises the user.**

---

## 2. Why This Fits ApexOS

- **Headless Pi deployment** — manual iteration is expensive (SSH, rebuild, restart). Self-evolution reduces that loop.
- **Long-lived daemon** — lives for days/weeks; should accumulate wisdom.
- **MCP plugin model** already gives polyglot extensibility. Evolution closes the loop back into config + behavior.
- **Policy engine already exists** — evolution is a natural extension of suggest/auto-edit/yolo.
- **Event log + CerebroCortex** already provide the full ingestion and memory path.
- **`Arc<RwLock<String>>` pattern** already proven for api_key and model — same pattern for system prompt hot-reload.

---

## 3. Core Requirements (MVP)

### 3.1 Procedural Memory Loop

The agent calls `find_relevant_procedures` at the start of each relevant turn (via CerebroCortex MCP, as a normal first tool call). Matching procedures are incorporated into its reasoning before it responds.

Procedures are stored via `store_procedure` with:
- Tags (`["apexos", "skill", domain...]`)
- Success/failure outcomes via `record_procedure_outcome`
- Context links to the episode that discovered them

This requires **zero changes to the Rust turn engine** for Phase 1. The system prompt instructs the agent when and how to do the recall. Rust-side pre-injection is a Phase 3 optimization if latency becomes measurable.

### 3.2 Self-Modification Pipeline

New virtual tool: `propose_evolution(proposal: EvolutionProposal)`.

`EvolutionProposal` variants (see `EVOLUTION_EVENT_TYPES.rs` for exact Rust):

| Variant | What it does | Affected files |
|---------|-------------|----------------|
| `RegisterMcpServer` | Add a new MCP plugin to plugins.toml | `/etc/agentd/plugins.toml` |
| `UnregisterMcpServer` | Remove an MCP plugin | `/etc/agentd/plugins.toml` |
| `UpdatePolicyRule` | Change a tool's approval mode | `/etc/agentd/policy.toml` |
| `UpdateSystemPrompt` | Propose a patch to the agent's own system prompt | `/etc/agentd/soul.md` |
| `HotReloadSubsystem` | Reload plugins/policy/gateway in-place | in-memory only |

Every proposal becomes `Event::EvolutionProposed`, flows through the policy engine under the `evolution.*` rule namespace, and on approval becomes `Event::EvolutionApplied`. All of this flows through the existing event bus, JSONL log, and WebSocket — for free.

### 3.3 System Prompt Evolution (`UpdateSystemPrompt`)

The agent's system prompt is loaded from `/etc/agentd/soul.md` at startup into an `Arc<RwLock<String>>` (same pattern as `api_key` and `model` in `GatewayState`). On an approved `UpdateSystemPrompt` proposal:
1. Write new content to `/etc/agentd/soul.md`
2. Write-lock the Arc, replace the string
3. Takes effect on the next turn — no restart needed

This is the highest-leverage evolution target: the agent can improve its own identity, instructions, and defaults over time.

### 3.4 Hot-Reload Mechanics

Resolves the currently deferred "Hot-reload mechanics" item in CLAUDE.md.

- **Plugins**: `Supervisor` gains a `SupervisorCmd::HotReload { id: PluginId }` variant on the existing internal channel. Handler: SIGTERM old process → wait 2s → start new process → MCP handshake → re-register tools on bus.
- **Policy**: `PolicyEngine` is already a pure struct. Reload = load new `PolicyConfig` from disk, swap the `Arc<RwLock<PolicyEngine>>`.
- **System prompt**: As above — Arc swap, next-turn effect.
- **Gateway**: No state to reload (stateless by design).
- **Full restart**: Only for Rust binary changes — not in MVP scope.

### 3.5 Audit and Rollback

- Every evolution event is appended to the JSONL event log automatically (no new code — it's just more `Event` variants).
- `rollback_evolution(id: EvolutionId)` tool: loads the pre-evolution snapshot from the log and re-applies it.
- CerebroCortex stores an evolution episode linking proposal → apply → outcome (via `episode_start`/`episode_end`).

---

## 4. Architecture Integration

| Component | Change | File |
|-----------|--------|------|
| `core` types | Add `EvolutionId`, `PolicyMode` (moved from plugins), `Subsystem`, `EvolutionProposal`, three new `Event` variants | `crates/core/src/types.rs` |
| `plugins` policy | Remove `Mode` enum, import `PolicyMode` from core | `crates/plugins/src/policy.rs` |
| `agent` turn engine | System prompt loaded from Arc (Phase 2) | `crates/agent/src/turn.rs` |
| `plugins` supervisor | Add `SupervisorCmd::HotReload`, implement `hot_reload()` | `crates/plugins/src/supervisor.rs` |
| `agentd` binary | Wire `propose_evolution` tool; load soul.md; expose system prompt Arc | `crates/agentd/src/main.rs` |
| Policy config | Add `evolution.*` rule namespace (default: `suggest`) | `agentd/config/policy.toml` |
| CerebroCortex | Already has all needed tools — just use them | N/A |
| Event log | Zero change — evolution events are just more variants | `crates/store/` |
| Frontend | New evolution event types stream to browser automatically | `ui/app.js` (Phase 3 display) |

---

## 5. Phased Plan

### Phase 0 — Types + Stub (start here)

1. Move `Mode` enum from `plugins::policy` to `core::types`, rename to `PolicyMode`. Update imports in `plugins/src/policy.rs`.
2. Add `EvolutionId(u64)` newtype to `core::types` (consistent with `ActionId(u64)` pattern — no new deps).
3. Add `Subsystem`, `EvolutionProposal`, and the three `Event` variants to `core::types`.
4. Add unit tests in `crates/core/src/state.rs` (pure, no I/O).
5. Add stub `propose_evolution` tool in `agentd/src/main.rs`: validates JSON, emits `EvolutionProposed`, returns `{"ok": true, "status": "proposed"}`.
6. Smoke test: agent calls the tool → event appears on WS and in JSONL log.
7. Commit: `feat(evolution): add EvolutionProposal event types and stub tool`

### Phase 1 — Procedural Memory Loop

1. Add `soul.md` load at agentd startup: read `/etc/agentd/soul.md` into `Arc<RwLock<String>>`, use as system prompt. Fall back to hardcoded default if missing.
2. Add to system prompt: instructions for when to call `find_relevant_procedures`, `store_procedure`, and `record_procedure_outcome`.
3. Wire `store_procedure` and `record_procedure_outcome` as first-class tools in the tool registry (same pattern as existing MCP passthrough — these are already CerebroCortex tools; this is about surfacing them explicitly in the registry with good descriptions).
4. Test on Pi with real CerebroCortex: store a procedure in one turn, recall it in the next.
5. Commit + update `docs/claude/agent-turn-engine.md`.

### Phase 2 — Config Evolution + Hot-Reload

1. Implement `hot_reload()` in supervisor: `SupervisorCmd::HotReload` handler (SIGTERM → handshake → re-register).
2. Add `toml_edit` to `plugins/Cargo.toml` (lossless TOML editing — different from `toml = "0.8"`).
3. Implement `propose_evolution` handler fully: TOML patch application, file write, hot-reload trigger.
4. Add `evolution.*` rules to `policy.toml` (default `suggest`, can be set to `yolo` for auto-apply).
5. Implement `UpdateSystemPrompt`: write `/etc/agentd/soul.md`, Arc swap.
6. Implement `rollback_evolution`: snapshot pre-evolution state in Cerebro episode; restore on demand.
7. End-to-end test on Pi: agent proposes new MCP server → user approves → daemon reloads without restart.
8. Move "Hot-reload mechanics" from "Deferred to keyboard" to "Resolved" in CLAUDE.md.

### Phase 3 — Polish + Self-Observation

- Evolution history tab in the frontend (events already stream to the browser).
- Automatic episode creation in Cerebro for every evolution.
- Metrics endpoint `/api/evolution/stats`.
- Agent proposes doc patches (CLAUDE.md, soul.md updates) via `UpdateSystemPrompt` — the self-improving loop dogfoods itself.

---

## 6. Non-Functional Requirements

- **Cold-start impact**: zero. Evolution features are gated behind `evolution` Cargo feature (default-off for minimal Pi image).
- **Memory footprint**: < 8 MiB additional on Pi 5 (evolution is data-driven TOML + subprocess restarts, not new heap allocations).
- **Safety**: no `unsafe` in the evolution path. Changes are TOML patches, file writes, and subprocess restarts.
- **Backward compat**: if `[evolution]` section is absent from `policy.toml`, daemon behaves exactly as today.

---

## 7. Risks and Mitigations

| Risk | Mitigation |
|------|------------|
| Agent proposes dangerous config changes | Default `evolution.*` rule is `suggest` — always asks user first |
| Hot-reload leaves daemon in inconsistent state | Two-phase: snapshot → apply → verify handshake; rollback on any failure |
| System prompt evolution degrades behavior | soul.md is version-controlled (git); rollback = `git checkout /etc/agentd/soul.md` |
| Cerebro recall adds token overhead | top-k=3 + truncation; ~200-300 tokens per turn maximum |
| Evolution log grows unboundedly | Already handled by the store crate's date-rolling JSONL files |

---

## 8. Success Metrics (30-day)

- ≥ 5 procedures auto-recalled per day with > 80% relevance.
- ≥ 3 evolution proposals accepted per week with < 5% rollback rate.
- agentd cold-start memory increase < 8 MiB.
- Time from "new workflow discovered" to "reusable next turn": < 2 turns.

---

## 9. Out of Scope (for now)

- **New Rust tools via code generation**: requires compile + binary swap. Possible future phase — the `RegisterMcpServer` mechanism covers 95% of the use case without it (new tools can be Python/bash MCP servers).
- **CSS skins via evolution**: deferred by agreement.
- **Multi-agent evolution coordination**: child agents can propose evolutions; parent policy gates them. No new architecture needed.

---

**Next action:** Phase 0 spike. Start with step 1 (PolicyMode move to core) — it unblocks everything else and is a pure refactor with no behavior change.
