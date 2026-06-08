# ApexOS Council — Multi-Agent Deliberation

## What it is

N agents with distinct personas deliberate on a topic over rounds until consensus or
max_rounds. Each agent can run on a different backend/model. André can watch live,
butt in, pause, or let it auto-run. APEX can also call it as a virtual tool.

Reference implementation: `ApexAurum-local/backend/app/api/v1/council.py`

---

## Why it fits ApexOS so well

| ApexAurum (Python/SQLAlchemy) | ApexOS equivalent |
|---|---|
| `DeliberationSession` DB table | JSONL at `/var/lib/agentd/council/` |
| `asyncio.gather(agent_turns)` | tokio `join_all` on N `run_turn()` |
| Per-agent `provider`/`model` | Per-agent `RoutingProvider` (trivial, already per-call) |
| SSE stream to browser | broadcast bus → WS → desktop window |
| Neural memory (village) | CerebroCortex `memory_store` with `visibility=shared` |
| Tool access per agent | `apexos-tools` + `sensor-head` + `cerebro` already wired |

No database. No new process. Reuses everything already built.

---

## New Event types (core crate)

```rust
CouncilStarted   { council_id, topic, agents: Vec<AgentDef> }
CouncilRoundStart { council_id, round: u32 }
CouncilAgentDelta { council_id, round: u32, agent_id: String, text: String }
CouncilAgentDone  { council_id, round: u32, agent_id: String, full_text: String }
CouncilRoundDone  { council_id, round: u32, convergence: f32, agreements: Vec<String> }
CouncilComplete   { council_id, rounds: u32, reason: String } // consensus | max_rounds | stopped
CouncilButtIn     { council_id, message: String }             // human inject → next round
```

---

## CouncilEngine (new sub-module in `agent` crate)

```rust
pub struct CouncilAgent {
    pub id:       String,      // "PRAGMATIST", "CRITIC", etc.
    pub persona:  String,      // system prompt for this agent
    pub backend:  String,      // "anthropic" | "ollama" | ...
    pub model:    String,
    pub color:    String,      // hex for UI
}

pub struct CouncilSession {
    pub id:                  CouncilId,
    pub topic:               String,
    pub agents:              Vec<CouncilAgent>,
    pub max_rounds:          u32,
    pub consensus_threshold: f32,
    pub use_tools:           bool,
    pub state:               CouncilState, // Running | Paused | Complete
    pub current_round:       u32,
    pub pending_butt_in:     Option<String>,
    pub history:             Vec<CouncilRound>,
}

pub struct CouncilRound {
    pub round:       u32,
    pub responses:   Vec<(String, String)>,  // (agent_id, text)
    pub convergence: f32,
    pub human_msg:   Option<String>,
}
```

**Round execution (per round):**
```
1. Build context string from previous rounds (same as build_round_context)
2. For each agent: create ephemeral RoutingProvider with agent's backend/model
3. join_all(N run_turn calls) — fully parallel
4. Broadcast CouncilAgentDelta events as tokens stream
5. Compute convergence score (keyword detection + optional LLM judge)
6. Append to history, persist to JSONL
7. Check stop conditions → emit CouncilRoundDone / CouncilComplete
```

**Convergence detection (two modes):**
- **Keyword** (fast, free): scan for "consensus", "agree", "aligned", etc. → 0.0–1.0
- **LLM judge** (optional): one cheap call (haiku) per round: "Do these responses converge? Score 0-10"

---

## Virtual tool: `convene_council`

APEX can call this as a tool. Blocks until complete, returns synthesis.

```json
{
  "name": "convene_council",
  "description": "Convene a multi-agent council to deliberate on a topic. Agents reason in parallel per round until consensus or max_rounds. Returns synthesised conclusion.",
  "input_schema": {
    "topic": "string — the question to deliberate",
    "agents": [{ "id": "string", "persona": "string", "backend?": "string", "model?": "string", "color?": "string" }],
    "max_rounds": "integer (default 5)",
    "consensus_threshold": "float 0-1 (default 0.8)",
    "use_tools": "bool (default true)"
  }
}
```

When called by APEX, events still fire on the bus (UI can watch), but the tool
result is a final synthesis string: `"After 3 rounds, consensus: [...]"`.

---

## Storage

`/var/lib/agentd/council/<council_id>.jsonl`

One JSON line per event (same append-only pattern as sessions and event log).
On restart: replay from file to restore in-progress sessions (or just list past sessions).

---

## Gateway routes

```
POST   /api/council              — start session (topic, agents, config)
GET    /api/council              — list sessions
GET    /api/council/:id          — session detail + history
POST   /api/council/:id/round    — manual: execute next round
POST   /api/council/:id/auto     — auto: run N rounds (streams WS events)
POST   /api/council/:id/butt-in  — queue human message for next round
POST   /api/council/:id/pause
POST   /api/council/:id/resume
POST   /api/council/:id/stop
```

---

## Desktop UI — Council Chamber window

New WinBox window `council` in desktop. Layout:

```
┌─ COUNCIL ──────────────────────────────────────────────────────────┐
│ Topic: "Should we add a Rust async runtime switch?"                 │
│ Round 2/5  ████████░░░░░░░░ convergence 0.62                       │
├──────────────────┬──────────────────┬──────────────────────────────┤
│ 🟢 PRAGMATIST    │ 🟡 CRITIC        │ 🟣 VISIONARY                 │
│ claude-haiku     │ nemotron:cloud   │ qwen3:1.7b                   │
│                  │                  │                              │
│ The practical    │ I disagree with  │ Long term this opens up...   │
│ trade-off here   │ the premise...   │ (streaming...)               │
│ (done)           │ (streaming...)   │                              │
├──────────────────┴──────────────────┴──────────────────────────────┤
│ [BUTT IN: _________________________] [SEND]  [PAUSE] [STOP]        │
└────────────────────────────────────────────────────────────────────┘
```

- Each agent column streams `CouncilAgentDelta` events in real time
- Convergence bar fills as score rises; turns green at threshold
- Butt-in input queues via `POST /api/council/:id/butt-in`
- "CONVENE" button in dock → opens session-picker or new-session dialog
- Past sessions loadable from session picker

---

## Build order

1. **Core** — add `CouncilStarted`, `CouncilAgentDelta`, etc. to `Event` enum + `apply()`
2. **Agent crate** — `CouncilEngine`: session struct, round loop, parallel `run_turn`, convergence
3. **`convene_council` virtual tool** — wired into the turn engine alongside `agent_spawn`
4. **Gateway** — `/api/council` routes, JSONL persistence
5. **Desktop UI** — Council Chamber WinBox window
6. **CLAUDE.md** — mark step 28 complete

Estimated: 2–3 sessions. Steps 1–3 are the meaty Rust work (~1 session).
Steps 4–5 are familiar patterns, fast to ship.

---

## Key decisions / questions for next session

- **Native agent personas?** ApexAurum has AZOTH/VAJRA/ELYSIAN/KETHER with full lore.
  Do we port those personas or start fresh with user-defined agents only?
- **Streaming to tool caller?** When APEX calls `convene_council` as a tool, stream
  events to bus (so UI is live) but block tool result until complete. ✓ natural.
- **LLM judge or keyword convergence?** Start with keyword (free, instant), add LLM
  judge as a flag later.
- **Cerebro integration?** Round summaries → `memory_store` after each complete session,
  tagged with `council` + topic. Same spirit as ApexAurum's village memories.
