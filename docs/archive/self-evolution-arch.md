# ApexOS Self-Evolution — Architecture Reference

This is the authoritative technical reference for event shapes, data flows, and
subsystem contracts. Update it when any of those change.

---

## Evolution Turn — Data Flow

```
User turn or scheduled trigger
          │
          ▼
Agent Turn Engine  (crates/agent/src/turn.rs)
  │
  │  [Phase 1] First tool call: find_relevant_procedures(query, tags, top_k=3)
  │  → results incorporated into agent reasoning before any user-visible output
  │
  │  [later in turn] Agent decides a structural change is worth proposing:
  │  tool_call("propose_evolution", { kind: "register_mcp_server", ... })
  │
  ▼
Plugin Supervisor  (crates/plugins/src/supervisor.rs)
  │  Dispatches to propose_evolution handler in agentd binary
  │
  ▼
Policy Engine  (crates/plugins/src/policy.rs)
  │  Checks "evolution.*" rule namespace
  │  Default: suggest  →  Event::ApprovalPending → user approves/denies
  │  Yolo mode:        →  auto-approve
  │
  ▼  (on approval)
Evolution Handler  (crates/agentd/src/main.rs)
  │
  ├──► Event::EvolutionApplied on bus
  │      → store crate appends to JSONL log (automatic)
  │      → WebSocket gateway forwards to all browser clients (automatic)
  │
  ├──► File write (plugins.toml / policy.toml / soul.md)
  │
  ├──► Hot-reload (see below)
  │
  └──► CerebroCortex episode: store_procedure + record_procedure_outcome
```

---

## New Types in `crates/core/src/types.rs`

See `EVOLUTION_EVENT_TYPES.rs` for exact copy-paste-ready code. Summary:

```rust
pub struct EvolutionId(pub u64);       // sequential, consistent with ActionId

pub enum PolicyMode { Suggest, AutoEdit, Yolo }   // moved from plugins::policy

pub enum Subsystem { Plugins, Policy, Agent, Gateway }

pub enum EvolutionProposal {
    RegisterMcpServer   { name, command, env, reason }
    UnregisterMcpServer { name, reason }
    UpdatePolicyRule    { tool_pattern, new_mode: PolicyMode, reason }
    UpdateSystemPrompt  { content, reason }     // writes /etc/agentd/soul.md
    HotReloadSubsystem  { subsystem: Subsystem }
}

// New Event variants (in the existing Event enum, "system" section):
Event::EvolutionProposed    { id: EvolutionId, proposal, proposed_by: SessionId }
Event::EvolutionApplied     { id: EvolutionId, proposal, patch_summary, applied_by: Option<SessionId> }
Event::EvolutionRolledBack  { evolution_id: EvolutionId, reason, rolled_back_by: Option<SessionId> }
```

**PolicyMode migration note:**
`plugins/src/policy.rs` currently defines `enum Mode`. That gets removed and replaced
with `use apexos_core::PolicyMode;`. `PolicyConfig.mode` field type changes
from `Mode` to `PolicyMode`. Zero behavior change — it's a rename + move.

---

## System Prompt Evolution (`soul.md`)

```
/etc/agentd/soul.md  ──load at startup──►  Arc<RwLock<String>>  in main.rs
                                                    │
                                       passed into TurnEngine::new()
                                       (same Arc<RwLock<>> pattern as
                                        api_key and model)
                                                    │
                                       run_turn() reads it each call
                                       via .read().await.clone()

On UpdateSystemPrompt approval:
  1. Write new content to /etc/agentd/soul.md
  2. Write-lock the Arc, replace the string
  3. Next run_turn() call picks it up — no restart
```

Rollback: pre-patch content is snapshot in Cerebro (store_procedure with
`memory_type: "evolution_snapshot"`). `rollback_evolution(id)` restores it.

---

## Hot-Reload Contract (`crates/plugins/src/supervisor.rs`)

The Supervisor already has an internal `mpsc::channel<SupervisorCmd>`. Add one variant:

```rust
enum SupervisorCmd {
    PluginDied { id: PluginId },
    HotReload  { id: PluginId },   // NEW
}
```

Handler (inside the select loop):

```rust
SupervisorCmd::HotReload { id } => {
    // 1. Send SIGTERM to old stdio process; wait up to 2s
    // 2. Look up updated PluginConfig from configs hashmap
    //    (already updated by TOML patch applicator before this fires)
    // 3. spawn_plugin(&config, sv_tx.clone()).await
    //    → performs MCP handshake
    //    → emits Event::PluginUp with new tool list
    // 4. On any failure: emit Event::Error, do NOT remove old entry
    //    (let the existing restart logic handle it)
}
```

Policy and agent hot-reload are simpler — they're Arc swaps, not process restarts.

---

## Procedure Memory Shape (CerebroCortex side)

```json
{
  "memory_type": "procedural",
  "tags": ["apexos", "skill", "<domain>"],
  "content": {
    "title": "How to safely add a vision MCP server on Pi 5",
    "trigger": "user asks about image capture or computer vision",
    "steps": ["...", "..."],
    "success_criteria": "MCP handshake completes, tools appear on bus",
    "known_pitfalls": ["seatd socket group is video not seat", "..."]
  },
  "context_ids": ["<episode_id_that_discovered_it>"]
}
```

Retrieval call (agent issues this as its first tool call when relevant):

```json
{
  "query": "<description of the current task>",
  "tags": ["apexos"],
  "min_salience": 0.5,
  "top_k": 3
}
```

The returned procedures are appended to the agent's reasoning context.
Salience increases on `record_procedure_outcome(success=true)`,
decays on failure — CerebroCortex handles this automatically.

---

## Evolution Episode in Cerebro

Each applied evolution is wrapped in an episode for full traceability:

```
episode_start("evolution: <proposal kind> <name>")
  │
  ├─ store_procedure(pre_evolution_snapshot)    ← for rollback
  ├─ store_procedure(evolution_proposal)
  │
  │  [apply happens here]
  │
  └─ record_procedure_outcome(success/failure, notes)
episode_end()
```

---

## Token Budget for Procedure Recall

| Component | Tokens (estimate) |
|-----------|------------------|
| top-3 procedure summaries | 150–300 |
| evolution_proposed event (if pending) | 50–100 |
| Total overhead per turn | < 400 |

This is negligible against the 200K–1M context windows in use.

---

## File Ownership Map

| File on disk | Who writes it | Hot-reload trigger |
|---|---|---|
| `/etc/agentd/plugins.toml` | Evolution handler | `SupervisorCmd::HotReload` |
| `/etc/agentd/policy.toml` | Evolution handler | Arc swap of PolicyEngine |
| `/etc/agentd/soul.md` | Evolution handler | Arc swap of system prompt |
| `/var/lib/agentd/events/*.jsonl` | store crate | N/A (append-only) |

All evolution-modified files are readable by the `agentd` user.
`soul.md` and config files should be owned by root, writable by agentd
(or use `sudo` wrapper similar to the power endpoint).

---

**Update this document when event shapes or reload contracts change.**
