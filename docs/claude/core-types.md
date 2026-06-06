# core crate — types, SystemState, apply()

> Load this when working on the `core` crate (build-order step 1).

## Status
- [ ] Cargo workspace scaffolded
- [ ] `Event` enum implemented
- [ ] `AgentContext` + `SystemState` implemented
- [ ] `state.apply()` implemented with unit tests passing

## Reference
- `docs/reference/core_types.rs` — canonical type definitions
- `docs/reference/state_apply.rs` — SystemState + apply() + test suite

## Key invariants
- `core` has ZERO I/O — no async, no network, no filesystem
- `Event` is `Serialize + Deserialize` — same type on bus, log, and WebSocket
- `apply()` is a pure function — deterministic, no side effects

## Notes
<!-- Fill in as implementation progresses: footguns, non-obvious decisions, test patterns -->
