# Policy engine — approval modes and rules

> Load this when working on the policy layer (build-order step 5).

## Status
- [ ] Mode enum: suggest / auto-edit / yolo
- [ ] Rules table loaded from policy.toml
- [ ] ToolRequested → policy check → dispatch or ApprovalPending
- [ ] UserApproval event → resume dispatch
- [ ] yolo short-circuit (no code-path difference, pure config)

## Mechanism
Approval is an **event on the bus**, not an inline call.
1. Agent emits `ToolRequested`
2. Policy engine inspects mode + rules table
3. Either: emit `ToolResult` directly (allow) or emit `ApprovalPending`
4. Frontend renders approval prompt, user returns `UserApproval` intent
5. Every approval/denial lands in the event log — free audit trail

## Config
`agentd/config/policy.toml` — mode + per-tool rules + subagent knobs

## Notes
<!-- Fill in: rule evaluation order, workspace path resolution for "workspace" rule -->
