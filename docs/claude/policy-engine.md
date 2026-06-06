# Policy engine — approval modes and rules

> Load this when working on the policy layer (build-order step 5).

## Status
- [x] Mode enum: suggest / auto-edit / yolo
- [x] Rules table loaded from policy.toml
- [x] ToolRequested → policy check → dispatch or ApprovalPending
- [x] UserApproval event → resume dispatch (or ToolResult with error on denial)
- [x] yolo short-circuit (pure config, zero code-path difference)

## Location
`crates/plugins/src/policy.rs` — pure logic, zero I/O, fully unit-tested (12 tests).
Supervisor owns a `PolicyEngine` and calls `policy.check(tool_name)` before dispatch.

## Mode × Rule → Decision matrix

| Rule \ Mode   | suggest | auto-edit | yolo   |
|---------------|---------|-----------|--------|
| `allow`       | Allow   | Allow     | Allow  |
| `ask`         | Ask     | Ask       | Allow  |
| `workspace`   | Ask     | Allow*    | Allow  |
| (no match)    | Ask     | Ask       | Allow  |

*Workspace path check (is the path inside AGENTD_WORKSPACE?) is deferred to keyboard.
Currently `workspace` + `auto-edit` = Allow unconditionally.

## Wildcard patterns
`"prefix.*"` matches any `"prefix.<something>"`. Exact match wins over wildcard.
Examples: `"cerebro.*"` matches `"cerebro.recall"`, `"cerebro.store"`.

## Flow
1. Agent emits `ToolRequested`
2. Supervisor checks `policy.check(call.tool)` → `Allow` or `Ask`
3. Allow → `dispatch_tool()` immediately (spawns tokio task)
4. Ask → `pending_approvals.insert(...)` + emit `ApprovalPending`
5. Frontend sends `UserApproval { action, granted }`
6. Granted → `dispatch_tool()`; Denied → emit `ToolResult { ok: false, "denied by user" }`

Every approval/denial flows through the bus → event log = free audit trail.

## Supervisor changes for policy
- `Supervisor::new(bus, policy: PolicyEngine)` — policy passed at construction
- `pending_approvals: HashMap<ActionId, PendingApproval>` — tracked in supervisor
- `dispatch_tool(&self, session, call)` — non-async, spawns task; handles unknown-tool error

## Deferred to keyboard
- `workspace` rule path check against `AGENTD_WORKSPACE` env var
- UI for rendering `ApprovalPending` in the browser frontend

## Reference
- `agentd/config/policy.toml` — config file with all rule examples
