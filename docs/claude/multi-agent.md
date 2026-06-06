# Multi-agent routing — sub-agent sessions

> Load this when working on sub-agent routing (build-order step 6).

## Status
- [x] `parent: Option<SessionId>` wired in AgentContext
- [x] Output routing: parent==None → TurnComplete, parent==Some → ToolResult to parent
- [x] `agent.spawn` as a policy-gated virtual tool
- [x] Cancellation cascade: UserCancel walks spawned tree and aborts all tasks
- [x] Semaphore shared across root + all children (via `TurnEngine::with_system`)

## Key insight
The agent loop does NOT change for sub-agents. A child runs the identical `run_turn`.
The only difference is where the completed output routes:
- Root session → nothing extra (TurnComplete already on bus)
- Child session → extract final text, emit `ToolResult { session: parent, call: call_id }`

## agent.spawn virtual tool
NOT an MCP plugin. Handled by two layers:
1. **Supervisor** `dispatch_tool`: if `call.tool == "agent.spawn"` → emit `Event::SpawnAgent`
   instead of looking it up in the MCP registry
2. **Agent router** (`main.rs`): catches `SpawnAgent` → creates child session, spawns `child_turn`,
   routes final output as `ToolResult` to parent

The policy engine gates it normally (`"agent.spawn" = "ask"` in policy.toml).

## Session IDs
- Root sessions: assigned by the frontend client (via `UserPrompt.session`)
- Child sessions: assigned by `AtomicU64` counter starting at `1 << 63` (top half of u64)
  to avoid collisions with frontend-assigned IDs

## Shared state in agent router (main.rs)
```
abort_handles:    HashMap<SessionId, AbortHandle>     — for cancellation
session_children: HashMap<SessionId, Vec<SessionId>>  — for cascade cancel
session_depths:   HashMap<SessionId, u32>             — for depth limiting
next_child_id:    AtomicU64                           — monotonic child ID counter
```

## Depth limiting
`max_depth` from `policy.toml [subagents]` (default: 4).
On `SpawnAgent`, check `parent_depth + 1 <= max_depth`; emit error ToolResult if exceeded.

## TurnEngine::with_system
Creates a derived engine with a different system prompt but **shares the same semaphore**.
Ensures global concurrency cap applies across root + all child agents.

## Cancellation cascade
`UserCancel { session }` → breadth-first walk of `session_children`, abort all found handles.
Uses `tokio::task::AbortHandle` — tasks are cancelled at their next `.await` point.

## Deferred to keyboard
- Sub-agent tool scoping (child may not need all parent's tools)
- Result streaming (child events visible to parent frontend)
- `inherit_mode` from policy subagents config
