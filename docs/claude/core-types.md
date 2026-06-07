# core crate — types, SystemState, apply()

> Load this when working on the `core` crate (build-order step 1).

## Status
- [x] Cargo workspace scaffolded (`agentd/Cargo.toml`, resolver = "2", core member)
- [x] `Event` enum implemented
- [x] `AgentContext` + `SystemState` implemented
- [x] `state.apply()` implemented with unit tests passing (9/9)
- [x] Self-evolution types added: `EvolutionId`, `PolicyMode`, `Subsystem`, `EvolutionProposal`, 3 new Event variants

## Reference
- `docs/reference/core_types.rs` — canonical type definitions
- `docs/reference/state_apply.rs` — SystemState + apply() + test suite

## Key invariants
- `core` has ZERO I/O — no async, no network, no filesystem
- `Event` is `Serialize + Deserialize` — same type on bus, log, and WebSocket
- `apply()` is a pure function — deterministic, no side effects

## Evolution types (Phase 0)

| Type | Location | Notes |
|------|----------|-------|
| `EvolutionId(u64)` | `types.rs` | Sequential ID, same pattern as `ActionId` |
| `PolicyMode` | `types.rs` | Moved here from `plugins::policy`; serde `rename_all = "kebab-case"` so policy.toml `"auto-edit"` still works |
| `Subsystem` | `types.rs` | Plugins / Policy / Agent / Gateway |
| `EvolutionProposal` | `types.rs` | Tagged enum (`kind` field); 5 variants |
| `Event::EvolutionProposed` | `types.rs` | Emitted by `propose_evolution` stub tool |
| `Event::EvolutionApplied` | `types.rs` | Emitted when apply logic lands (Phase 2) |
| `Event::EvolutionRolledBack` | `types.rs` | Emitted by rollback tool (Phase 2) |

`plugins::policy` now re-exports `PolicyMode` from core (`pub use apexos_core::PolicyMode`).
`apexos_plugins` re-exports it as `PolicyMode` (lib.rs) — no call-site changes needed.

## Notes
- `AgentText` / `AgentThinking` are pure fan-out; deltas are NOT accumulated into state (turn engine commits the full assistant message to history on `TurnComplete`)
- `ContentBlock::ToolResult` uses `tool_use_id: String` (not `ActionId`) — matches the Anthropic API's string-keyed tool_use blocks
- JSON serde round-trip test confirms the `#[serde(tag = "type", rename_all = "snake_case")]` strategy works for all event variants
- `EvolutionProposal` uses `#[serde(tag = "kind", rename_all = "snake_case")]` so the tool call JSON just needs `"kind": "register_mcp_server"` etc.
