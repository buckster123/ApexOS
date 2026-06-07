# Self-Evolution Integration Checklist

Work through phases in order. Check off items in the same commit as the code.
No "almost done" — item is checked when tested and committed.

---

## Phase 0 — Types + Stub ✓

**Goal:** Evolution events flow through the bus, log, and WebSocket. No real apply logic yet.

- [x] `crates/core/src/types.rs` — add `EvolutionId(u64)` newtype (after `ActionId`)
- [x] `crates/core/src/types.rs` — add `PolicyMode` enum (Suggest/AutoEdit/Yolo, default Suggest)
- [x] `crates/core/src/types.rs` — add `Subsystem` enum
- [x] `crates/core/src/types.rs` — add `EvolutionProposal` enum (all 5 variants)
- [x] `crates/core/src/types.rs` — add `EvolutionProposed`, `EvolutionApplied`, `EvolutionRolledBack` to `Event`
- [x] `crates/plugins/src/policy.rs` — remove local `Mode` enum; `pub use apexos_core::PolicyMode`; update `PolicyConfig.mode: PolicyMode`; lib.rs re-export updated
- [x] `crates/core/src/state.rs` — add match arms for 3 new Event variants + 3 unit tests (round-trip, kind tag, kebab-case serde)
- [x] `crates/plugins/src/supervisor.rs` — add `propose_evolution` virtual tool dispatch: parse EvolutionProposal, emit EvolutionProposed, return ToolResult immediately
- [x] `crates/agentd/src/main.rs` — add `propose_evolution_spec()` + wire into `gather_tools()`
- [x] `agentd/config/policy.toml` — add `"propose_evolution" = "allow"` rule
- [x] All 36 tests pass (0 failures)
- [x] Smoke test on Pi: EvolutionProposed appears on WS stream and in JSONL log (verified)
- [x] Commit: `feat(evolution): add EvolutionProposal event types and stub tool`
- [x] Update `docs/claude/core-types.md` with new types

---

## Phase 1 — Procedural Memory Loop ✓

**Goal:** Agent recalls and stores procedures as a natural part of conversation.

- [x] `agentd/config/soul.md` — written; identity + procedural memory instructions + self-evolution + principles
- [x] `crates/agent/src/turn.rs` — `system: Arc<RwLock<String>>`; `system_arc()` accessor; `with_system()` creates new Arc or inherits parent's; `run_turn` reads per-call
- [x] `crates/agentd/src/main.rs` — `load_soul()` with fallback chain; `Some(load_soul())` passed to `TurnEngine::new()`; `_soul_arc` extracted for Phase 2
- [x] `store_procedure` / `record_procedure_outcome` / `find_relevant_procedures` available via CerebroCortex MCP (66 tools); soul.md instructs the agent when/how to use them
- [x] 36 tests passing (0 failures)
- [x] Integration test on Pi: store_procedure called in turn 1; find_relevant_procedures called in turn 2 — loop verified
- [x] Commit: `feat(evolution): soul.md system prompt and Arc hot-reload wiring`
- [x] Update `docs/claude/agent-turn-engine.md`

---

## Phase 2 — Config Evolution + Hot-Reload ✓

**Goal:** Agent can propose and apply real structural changes without daemon restart.

- [x] `crates/agentd/Cargo.toml` — add `toml_edit = "0.22"` (lossless TOML editing)
- [x] `crates/plugins/src/supervisor.rs` — `SupervisorCmd::{SpawnPlugin, KillPlugin, HotReload}`; `cmd_tx()` accessor; `Arc<RwLock<PolicyEngine>>` replaces owned field
- [x] `crates/agentd/src/main.rs` — `spawn_evolution_applier()` + `apply_evolution()`:
  - `UpdateSystemPrompt`: write soul.md + Arc swap (live, no restart)
  - `UpdatePolicyRule`: toml_edit patch + Arc swap (live, no restart)
  - `RegisterMcpServer`: toml_edit append + `SupervisorCmd::SpawnPlugin`
  - `UnregisterMcpServer`: toml_edit remove + `SupervisorCmd::KillPlugin`
  - `HotReloadSubsystem`: in-memory reload of agent/policy; stub for plugins/gateway
- [x] All proposals: emit `Event::EvolutionApplied` on success, `Event::Error` on failure
- [x] `agentd/config/policy.toml` — `"propose_evolution" = "ask"` (gates apply at approval UX)
- [x] `agentd/deploy/agentd.service` — `ReadWritePaths=/etc/agentd` (evolution applier writes config)
- [x] Pi config ownership — `chown agentd:agentd /etc/agentd/{soul.md,policy.toml,plugins.toml}`
- [x] End-to-end test on Pi: `UpdateSystemPrompt` → `ApprovalPending` → approve → `EvolutionApplied` → soul.md updated on disk and in-memory (verified)
- [x] Commit: `feat(evolution): config evolution and hot-reload`
- [x] Update `docs/claude/plugin-supervisor.md`

**Deferred to Phase 4:**
- [ ] Cerebro episode wrapping around each apply (episode_start/add_step/episode_end)
- [ ] Full RegisterMcpServer end-to-end test with a real new plugin

---

## Phase 3 — Polish + Self-Observation ✓

- [x] Frontend: `EvolutionProposed` → subtle sys-msg; `EvolutionApplied` → green EVOLVED banner in output stream
- [x] Frontend: evolution history modal (Ctrl+Shift+E or Σ badge in header; fetches `/api/evolution/history`)
- [x] Frontend: `Error { session: null }` now rendered (fixed session filter `!== undefined` → `!= null`)
- [x] `crates/gateway/src/lib.rs` — `/api/evolution/history` endpoint (reads JSONL log, filters `evolution_applied`)
- [x] Rollback: `rollback_evolution` virtual tool + `rollback_evolution_spec()` + `"rollback_evolution" = "ask"` in policy
- [x] Rollback: `compute_undo()` snapshots state before apply; stored in `rollback_store: Arc<Mutex<HashMap<EvolutionId, EvolutionProposal>>>`
- [x] Rollback: supervisor `rollback_tx` / `set_rollback_tx()` channel routes tool call to applier
- [x] Rollback: applier `select!` arm handles rollback_rx, applies undo proposal, emits `EvolutionRolledBack`
- [x] All 37 tests pass (0 failures)
- [x] Self-update loop: agent proposes `UpdateSystemPrompt` after discovering improved phrasing (dogfood) — verified: agent calls `read_soul_md` first, writes correct content, Cerebro episode created with steps
- [x] Metrics: `/api/evolution/stats` returning counts per variant, applied total, rollback rate
- [x] Update this checklist and CLAUDE.md via the evolution mechanism itself (dogfood) — checklist updated here; APEX proposed soul.md update via `propose_evolution` in the UI (the mechanism updating its own config)

---

## Phase 4 — Cerebro Integration + Persistence ✓

- [x] `ToolProxy` — `SupervisorCmd::DirectCall` + oneshot reply; supervisor handles in select loop; applier calls Cerebro tools without going through policy or bus
- [x] Episode wrapping: `episode_start` before apply → `episode_add_step` with undo snapshot on success → `episode_end` with outcome; best-effort (Cerebro down → apply still proceeds)
- [x] `evo_kind()` extracts proposal variant tag for episode titles; `parse_episode_id()` reads Cerebro response (fixed: MCP wraps content in `[{type:"text",text:"..."}]` array, not a bare string)
- [x] `read_soul_md` virtual tool — agent reads live soul_arc before proposing update_system_prompt; wired via `SupervisorCmd::SetSoulArc` after engine init
- [x] Durable rollback: `restore_rollback_store()` at startup — calls `list_episodes` (agent_id: CLAUDE-APEX), `get_episode_memories` per evolution episode, parses `undo_snapshot` JSON from memory content, rebuilds `rollback_store`; EvolutionId parsed from episode title "evolution {N}: {kind}"; verified on Pi (3 episodes found, restore ran at Cerebro ready time ~6s post-start)
- [ ] Full RegisterMcpServer end-to-end test: register a live plugin via propose_evolution, verify PluginUp fires, verify UnregisterMcpServer tears it down
- [ ] Rust-native code generation for new virtual tools (out of scope until Phase 5+)

---

## Phase 5 — Session Persistence + Multi-Client Sync

**Goal:** Sessions survive daemon restarts; cage kiosk and web browser share the same live session.

**Root cause of current disconnect:** JS localStorage assigns session IDs independently per browser tab/client. No server-side session state — histories are in-memory only, wiped on restart.

### 5a — Server-side session persistence ✓
- [x] `agentd/src/session_store.rs` — append-only JSONL per session under `$AGENTD_LOG/sessions/{session_id}.jsonl`; mtime used for last_active
- [x] User message persisted immediately on UserPrompt; assistant delta persisted after each root_turn (updated[snapshot_len..])
- [x] On daemon startup: scan sessions dir, load all sessions into `histories` HashMap (restores conversation across restarts); verified on Pi
- [x] `/api/sessions` gateway endpoint — returns [{session_id, last_active, message_count, preview}] sorted newest-first; verified live

### 5b — Server-side session ID issuance + WS handshake
- [ ] WS connect handshake: client sends `{"type":"hello","resume_session":"sid_xxx"}` (or omits for new session); server responds `{"type":"session_init","session_id":"sid_xxx","history":[...]}`
- [ ] Server issues session IDs (UUID or sequential); client stores in localStorage and sends on reconnect
- [ ] History replay on join: gateway sends history as a `session_history` burst to the newly connected WS client only (not broadcast)
- [ ] Any two clients sending the same session_id join the same live session — cage kiosk + web browser converge

### 5c — UI
- [ ] `app.js`: send hello frame on connect with localStorage session ID; handle `session_init` to pre-populate chat history
- [ ] Session picker modal: list recent sessions from `/api/sessions`, click to resume; keyboard shortcut
- [ ] New session = clear localStorage session ID → server issues fresh ID

---

## Deferred (no start date)

- Any change to tokio task-shutdown ordering (still deferred in CLAUDE.md)
