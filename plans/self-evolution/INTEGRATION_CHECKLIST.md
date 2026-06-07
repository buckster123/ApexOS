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
- [ ] Smoke test on Pi: send `propose_evolution` from browser → EvolutionProposed appears in WS stream and JSONL log
- [ ] Commit: `feat(evolution): add EvolutionProposal event types and stub tool`
- [ ] Update `docs/claude/core-types.md` with new types

---

## Phase 1 — Procedural Memory Loop

**Goal:** Agent recalls and stores procedures as a natural part of conversation.

- [ ] Create `/etc/agentd/soul.md` (or `agentd/config/soul.md` as the example/template)
- [ ] `crates/agentd/src/main.rs` — load soul.md into `Arc<RwLock<String>>` at startup (same pattern as `model_arc`); fall back to hardcoded default string if missing
- [ ] `crates/agentd/src/main.rs` — pass soul arc into `TurnEngine` so `run_turn` reads it per-call
- [ ] `crates/agent/src/turn.rs` — change `system: Option<String>` to `system: Arc<RwLock<Option<String>>>` so hot-reload works
- [ ] `agentd/config/soul.md` — write initial system prompt with procedural memory instructions (when to call `find_relevant_procedures`, `store_procedure`, `record_procedure_outcome`)
- [ ] Wire `store_procedure` and `record_procedure_outcome` as explicitly-described tools in the tool registry (they're already CerebroCortex MCP tools — this is about making them show up with good descriptions)
- [ ] Integration test (manual on Pi): store a procedure in one turn, recall it in the next
- [ ] Commit: `feat(evolution): procedural memory loop via soul.md system prompt`
- [ ] Update `docs/claude/agent-turn-engine.md`

---

## Phase 2 — Config Evolution + Hot-Reload

**Goal:** Agent can propose and apply real structural changes without daemon restart.

- [ ] `crates/plugins/Cargo.toml` — add `toml_edit = "0.22"` (lossless TOML editing, distinct from `toml = "0.8"`)
- [ ] `crates/plugins/src/supervisor.rs` — add `SupervisorCmd::HotReload { id: PluginId }` and handler (SIGTERM → 2s → spawn_plugin → handshake → re-register)
- [ ] `crates/agentd/src/main.rs` — implement full `propose_evolution` handler:
  - `RegisterMcpServer` / `UnregisterMcpServer`: patch plugins.toml via `toml_edit`, send `SupervisorCmd::HotReload`
  - `UpdatePolicyRule`: patch policy.toml via `toml_edit`, swap `Arc<RwLock<PolicyEngine>>`
  - `UpdateSystemPrompt`: write soul.md, swap `Arc<RwLock<String>>` for system prompt
  - `HotReloadSubsystem`: dispatch appropriate reload without file change
- [ ] All proposals: emit `Event::EvolutionApplied` on success, `Event::Error` on failure (no partial state)
- [ ] `agentd/config/policy.toml` — add `evolution.*` rule (default `"ask"`)
- [ ] `crates/agentd/src/main.rs` — implement `rollback_evolution(id: EvolutionId)`: restore pre-patch snapshot from Cerebro, re-apply
- [ ] Cerebro episode wrapping: `episode_start` → apply → `record_procedure_outcome` → `episode_end`
- [ ] End-to-end test on Pi: propose `RegisterMcpServer` → approve in browser → plugin reloads, new tools appear, no daemon restart
- [ ] Commit: `feat(evolution): config evolution and hot-reload`
- [ ] Update `docs/claude/plugin-supervisor.md`, `docs/claude/policy-engine.md`
- [ ] Update CLAUDE.md: move "Hot-reload mechanics" from "Deferred" to "Resolved"

---

## Phase 3 — Polish + Self-Observation

- [ ] Frontend: display `EvolutionProposed` / `EvolutionApplied` events in output (already streams — just add rendering in `app.js`)
- [ ] Frontend: evolution history panel (reuses history modal pattern)
- [ ] `crates/gateway/src/lib.rs` — add `/api/evolution/history` endpoint (reads JSONL log filtered to evolution events)
- [ ] Self-update loop: agent proposes `UpdateSystemPrompt` after discovering improved phrasing → user approves → new soul.md
- [ ] Metrics: `/api/evolution/stats` returning counts per variant, rollback rate
- [ ] Update this checklist and CLAUDE.md via the evolution mechanism itself (dogfood)

---

## Blocked (do not start until Phase 0 lands)

- Full hot-reload implementation (needs `EvolutionApplied` event shape finalized)
- Rollback tool (needs Cerebro episode shape agreed)
- Any change to tokio task-shutdown ordering (still deferred in CLAUDE.md)
- Rust-native code generation for new tools (Phase 4+ idea, out of scope for now)
