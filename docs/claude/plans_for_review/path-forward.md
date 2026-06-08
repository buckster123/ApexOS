# ApexOS — Path Forward

Steps in priority order. Each is independently shippable.

---

## Step 29 — Council persistence + Cerebro post-council hook

Two tight sub-tasks, ship together.

### 29a — Council JSONL persistence
Each council session gets an append-only JSONL log at `/var/lib/agentd/council/<id>.jsonl`.

Events to write (one JSON object per line):
- `{"type":"start","id":"...","topic":"...","agents":[...],"rounds":N,"ts":...}`
- `{"type":"round_start","round":N,"ts":...}`
- `{"type":"agent_delta","agent":"AZOTH","text":"...","ts":...}` (or batch per round-done)
- `{"type":"round_done","round":N,"convergence":0.72,"ts":...}`
- `{"type":"complete","synthesis":"...","ts":...}`

**Where to wire:** `council_handler.rs` — it already has the full lifecycle. Add a `tokio::fs::File` (append mode, create if missing) keyed on council ID before the `run_council` call. Write events as they arrive from the council channel. Close on complete.

**Directory**: create `/var/lib/agentd/council/` in the systemd unit or on first write (`tokio::fs::create_dir_all`).

**Gateway**: add `GET /api/council/{id}/log` that streams the JSONL file (or returns it whole). Low priority — mainly for debugging.

### 29b — Cerebro post-council hook
After `run_council` returns synthesis, store a memory in CerebroCortex:

```
memory_store(
  content: "Council [{id}] — Topic: {topic}\nAgents: {agents}\nSynthesis: {synthesis}",
  tags: ["council", "apexos"],
  agent_id: "APEX"
)
```

**Where to wire:** `council_handler.rs`, in the `spawn_council_handler` task, after updating the `CouncilRecord` to `"complete"`. Needs `cerebro_tx` (the same MCP channel used by the agent turn engine) or a direct HTTP call to the Cerebro REST API if simpler.

Look at how `agent/src/turn.rs` or `agentd/src/main.rs` sends MCP tool calls — replicate that pattern. Alternatively, `apexos-tools` has `http_fetch`; could POST to Cerebro's REST endpoint directly.

**Fallback**: if Cerebro is unavailable, log and continue — don't fail the council.

---

## ~~Step 30~~ ✓ Agent-to-Agent (A2A) messaging — SHIPPED

Native async messaging between agent sessions — not just parent→child via tool result, but peer-to-peer intent passing.

### What this unlocks
- Council members that can **call tools** (via their own session) rather than just reason in text
- Long-running background agents that receive tasks from the main agent
- Workflow fan-out without blocking the orchestrator on each child

### Design sketch

**New virtual tool: `send_to_agent`**
```json
{
  "tool": "send_to_agent",
  "session_id": "42",          // target session (or "new" to spawn)
  "message": "Analyse this dataset and report back",
  "await_reply": false         // fire-and-forget vs blocking
}
```

**New Event variants** (in `core`):
```rust
AgentMessage { from: SessionId, to: SessionId, body: String, msg_id: Uuid },
AgentMessageAck { msg_id: Uuid, from: SessionId },
```

**Inbox per session**: `HashMap<SessionId, VecDeque<String>>` in the agent context or a new `InboxMap` Arc. Agent sees incoming messages as synthetic `UserPrompt` events injected into its bus stream — same path as scheduled tasks.

**Gateway routes**:
- `POST /api/sessions/{id}/message` — external/UI → agent
- Events `agent_message` / `agent_message_ack` on WebSocket — UI can render inter-agent chat

**Frontend**: extend sub-agent windows with an outbox/inbox panel; show sent messages and replies as a thread below the agent output.

### Relationship to council
Council today: ephemeral per-agent providers, text only, no tool access.
A2A tomorrow: each council member *is* a real session — tools, memory, full turn engine. Council becomes a coordinator that sends tasks to specialist sessions and waits for structured replies. This is the natural upgrade path.

### Implementation order
1. `AgentMessage` / `AgentMessageAck` in `core`
2. `InboxMap` Arc + `send_to_agent` virtual tool in supervisor
3. Inject inbox messages into agent bus (same as `schedule_task` loop)
4. Gateway route + WebSocket events
5. Sub-agent window inbox/outbox panel in desktop UI
6. (Optional) upgrade council to use real sessions instead of ephemeral providers

---

## Parked (good ideas, not yet scheduled)

| Idea | Notes |
|------|-------|
| RAG over event log | Embed daily JSONL → semantic search via Cerebro or local vector store |
| Mobile PWA companion | Connects to Pi over local LAN; mic/speaker/chat; no install |
| Sensor anomaly wakeup | Agent wakes on IAQ > threshold or thermal hotspot, not just voice |
| Multi-Pi mesh | Second Pi as compute/sensor node; A2A over LAN |
