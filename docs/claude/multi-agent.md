# Multi-agent routing — sub-agent sessions

> Load this when working on sub-agent routing (build-order step 6).

## Status
- [ ] `parent: Option<SessionId>` wired in AgentContext
- [ ] Output routing: parent==None → frontend, parent==Some → ToolResult
- [ ] `agent.spawn` as a policy-gated tool call
- [ ] Cancellation cascade: UserCancel walks `spawned` tree
- [ ] Semaphore shared across root + all children

## Key insight
The agent loop does NOT change for sub-agents. A child runs the identical
`run_turn`. The only difference is where `TurnComplete` output routes.
Hierarchy is flat in the HashMap; parent links reconstruct it.

## Cancellation
`UserCancel` on session X → cancel X + walk `AgentContext.spawned` recursively.
Use `tokio::task::AbortHandle` per session stored in the state actor.

## Notes
<!-- Fill in: depth limiting, fan-out behavior, sub-agent tool scoping -->
