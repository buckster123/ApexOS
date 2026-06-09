# docs/archive — Pre-implementation planning documents

These are design and planning documents written *before* the code existed. They
describe the intended architecture and capture design decisions that shaped what
was actually built.

**The code is authoritative.** When anything here conflicts with the real code,
trust the code. These docs are kept because they explain the *why* behind design
choices and are useful context for understanding the system.

| File | Contents |
|------|---------|
| `masterplan-v0.md` | Original system architecture vision — the "three planes" model, locked decisions, crate layout, build order |
| `self-evolution-prd.md` | Pre-implementation requirements for the self-evolution layer (step 12) |
| `self-evolution-arch.md` | Data flow and event shapes for the self-evolution subsystem |
| `hermes-comparison.md` | Design note: how the Hermes agent skill model inspired ApexOS procedural memory via CerebroCortex |
| `phase6-agent-bodies.md` | Phase 6 (agent bodies) implementation notes — tool specs, denylist rationale, Pi sensor wiring details |
