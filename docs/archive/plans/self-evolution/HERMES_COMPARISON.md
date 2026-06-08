# Hermes → ApexOS Self-Evolution Mapping

| Hermes Concept | ApexOS Equivalent | Notes |
|---|---|---|
| `SKILL.md` files (in-repo + user-local) | Procedural memories in CerebroCortex (`memory_type: "procedural"`) | No filesystem required on Pi. Queryable, outcome-weighted, salience-decaying. |
| `skill_manage` tool | `propose_evolution` tool + `store_procedure` | Evolution proposals are the "create skill" action; they go through the policy gate. |
| Skill loader at session start | `find_relevant_procedures` call at turn start | Agent issues this as a first tool call when context suggests it's useful. System prompt provides the when/how. |
| Dual-mode plugins (native + MCP) | MCP servers + native Rust tools in agentd | Already the architecture. Evolution adds runtime registration of new MCP servers without restart. |
| Persistent memory (Cerebro) | CerebroCortex (already wired) | `record_procedure_outcome` after every tool call that came from a recalled procedure closes the learning loop. |
| `SOUL.md` (agent identity/personality) | `/etc/agentd/soul.md` loaded into Arc<RwLock<String>> | Evolves via `UpdateSystemPrompt` proposal. Hot-reloads into the turn engine without daemon restart. |
| `.hermes.md` (project instructions) | `CLAUDE.md` (already exists, already self-evolving) | Agent proposes patches via `UpdateSystemPrompt` equivalent for project docs. |
| Hot-reload of skills/plugins | `SupervisorCmd::HotReload` in plugins crate | Resolves the "Hot-reload mechanics" deferred item in CLAUDE.md. |
| Approval modes (yolo etc.) | Already exists in policy engine | Evolution rules live under `evolution.*` namespace in policy.toml. |
| Episode tracking | Cerebro episodes: proposal → apply → outcome | Full audit trail linkable back to the conversation that discovered the need. |

---

## Key Architectural Difference (ApexOS is cleaner)

Hermes stores skills as files in a directory tree. ApexOS stores procedures in
CerebroCortex, which is:
- **Queryable by semantic content** (not just filename)
- **Outcome-weighted** (successful procedures surface more readily over time)
- **Already running** as an MCP server — no new infrastructure

For a long-lived embedded daemon with limited filesystem ops, CerebroCortex-based
procedural memory is a better fit than file-based skills.

---

## Migration Path: Hermes Skills → ApexOS Procedures

Any workflow first developed in a Hermes session (e.g. on Krackan during development)
can be exported as a structured procedure and ingested into CerebroCortex:

```python
# In a Hermes session:
# 1. Export the skill to structured JSON
# 2. Tag it: ["apexos", "ported-from-hermes", "<domain>"]
# 3. store_procedure(agent_id="CLAUDE-APEX", ...)
```

The ApexOS agent will discover it naturally on next relevant turn.
