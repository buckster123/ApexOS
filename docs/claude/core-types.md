# core crate — types, SystemState, apply()

> Load this when working on the `core` crate (build-order step 1).

## Status
- [x] Cargo workspace scaffolded (`agentd/Cargo.toml`, resolver = "2", core member)
- [x] `Event` enum implemented
- [x] `AgentContext` + `SystemState` implemented
- [x] `state.apply()` implemented with unit tests passing (6/6)

## Reference
- `docs/reference/core_types.rs` — canonical type definitions
- `docs/reference/state_apply.rs` — SystemState + apply() + test suite

## Key invariants
- `core` has ZERO I/O — no async, no network, no filesystem
- `Event` is `Serialize + Deserialize` — same type on bus, log, and WebSocket
- `apply()` is a pure function — deterministic, no side effects

## Notes
- `AgentText` / `AgentThinking` are pure fan-out; deltas are NOT accumulated into state (turn engine commits the full assistant message to history on `TurnComplete`)
- `ContentBlock::ToolResult` uses `tool_use_id: String` (not `ActionId`) — matches the Anthropic API's string-keyed tool_use blocks
- JSON serde round-trip test confirms the `#[serde(tag = "type", rename_all = "snake_case")]` strategy works for all event variants
