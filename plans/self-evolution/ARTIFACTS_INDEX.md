# Self-Evolution Artifacts Index

All files in `plans/self-evolution/`. These are living documents — update them
when decisions change or phases complete, exactly like CLAUDE.md.

| File | Purpose |
|------|---------|
| `PRD.md` | Vision, requirements, phased plan, risks, metrics — start here |
| `ARCHITECTURE.md` | Data flows, event shapes, hot-reload contracts, token budgets — authoritative reference |
| `EVOLUTION_EVENT_TYPES.rs` | Exact Rust additions for `crates/core/src/types.rs` — copy-paste ready |
| `INTEGRATION_CHECKLIST.md` | Per-phase, per-crate checklist — use as the TODO list during implementation |
| `HERMES_COMPARISON.md` | Hermes → ApexOS concept mapping and key architectural differences |

---

## How to Use in a Fresh Session

1. Read `PRD.md` end-to-end for context and current phase status.
2. Read `ARCHITECTURE.md` for event shapes and subsystem contracts.
3. Open `INTEGRATION_CHECKLIST.md` and find the first unchecked item.
4. Use `EVOLUTION_EVENT_TYPES.rs` as the copy-paste source for Phase 0 code.
5. `HERMES_COMPARISON.md` is background — read it if you need to understand why a decision was made.

---

## Relationship to Other Docs

- `plans/agentos-handoff/MASTERPLAN.md` — top-level architecture. Self-evolution is additive.
- `CLAUDE.md` — when Phase 2 lands, move "Hot-reload mechanics" from "Deferred" to "Resolved".
- `docs/claude/evolution.md` — does not exist yet. Create it when Phase 0 lands.
- `docs/claude/agent-turn-engine.md` — update in Phase 1 (soul.md + Arc system prompt).
- `docs/claude/plugin-supervisor.md` — update in Phase 2 (hot_reload implementation).
- `docs/claude/policy-engine.md` — update in Phase 2 (evolution.* rules, PolicyMode move).
