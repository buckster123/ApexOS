# ApexOS Vast.ai Inference — Step 36 Architecture

Design doc for Track A: Vast.ai GPU rental + local PCIe inference backend.
Load this when working on vast virtual tools, recipe system, tunnel management,
or the desktop Inference UI.

**Scope**: Vast.ai only for now. Together.ai already works via existing
`OaiProvider` (just set `AGENTD_OAI_BASE_URL=https://api.together.xyz/v1`) — zero
new code. vLLM multi-GPU is deferred to Track B / Step 37.

---

## Vision

The Pi 5 is the control plane. The GPU is rented on demand.

Agent needs heavy inference → calls `vast_launch("qwen36-35b-h100")` → agentd
SSHes to Vast, spins up a pre-configured llama-server container, tunnels port 8000
back to the Pi, and hot-swaps the OaiProvider backend. Agent doesn't change its
behaviour — it just got 8× faster with a 35B model. When the task is done, agent
calls `vast_destroy()` and billing stops.

Cost profile: RTX 5090 at $0.34/hr, Qwen3.6-35B-MoE Q5 at 128K ctx. Competitive
with any managed API per token at agentic workloads.

---

## What We Take from LocalRouter

| Source | What we take | What we skip |
|--------|-------------|-------------|
| `vast_up.sh` | GPU filter logic, offer search CLI invocation, instance create, env var wiring, Docker image names | Python TUI, Together paths, vLLM paths |
| `vast_down.sh` | Instance destroy one-liner | — |
| `tools/vast_tunnel.sh` | SSH `-L` forwarding pattern, ControlMaster config, health-check retry, PID management | Shell script itself (we replicate in Rust/tokio) |
| `recipes.toml` | Format: gpu_tier + model_repo + model_quant + ctx + parallel + kv_type + description | 60+ recipes (we keep ~12 curated) |
| Docker images | `ghcr.io/buckster123/vastai-gguf:prebuilt` — already battle-tested | builder/vllm images for now |

We do **not** import LocalRouter as a dependency. We replicate only the mechanics
that work (offer search + create + tunnel) in Rust virtual tool handlers.

---

## Architecture

```
┌─────────────────── Pi 5 (agentd) ─────────────────────────────────┐
│                                                                      │
│  virtual tools (supervisor.rs)                                       │
│    vast_launch() ──→ vastai CLI (subprocess) ──→ Vast.ai API        │
│    vast_destroy() ─┘                                                 │
│    vast_status()                                                     │
│    vast_list_recipes()                                               │
│                         ↓                                            │
│  tunnel manager (VastTunnelHandle)                                   │
│    tokio::process::Child (ssh -L 8000:127.0.0.1:8000 ...)           │
│    keepalive ping every 30s                                          │
│    auto-reconnect on drop                                            │
│                         ↓                                            │
│  RoutingProvider ──→ OaiProvider → http://127.0.0.1:8000/v1         │
│  (hot-swap on VastInstanceReady event)                               │
│                                                                      │
│  gateway routes                                                      │
│    GET  /api/vast/recipes                                            │
│    GET  /api/vast/status                                             │
│    POST /api/vast/launch   { recipe }                                │
│    POST /api/vast/destroy                                            │
│    GET  /api/vast/offers?gpu=5090  (live vastai search)             │
│    GET  /api/vast/hf-search?q=qwen (HuggingFace GGUF browse)        │
│                                                                      │
└──────────────────────────────────────────────────────────────────────┘
         │ SSH -L 8000                    Vast.ai cloud
         └──────────────────────────────► llama-server:8000
                                          (Docker: vastai-gguf:prebuilt)
                                          Model: any GGUF from HF
```

### Shared state (ARC across supervisor + gateway)

```rust
pub struct VastState {
    pub instance: Arc<RwLock<Option<VastInstance>>>,
    pub tunnel: Arc<Mutex<Option<TunnelHandle>>>,
}

pub struct VastInstance {
    pub id: String,
    pub recipe_name: String,
    pub ssh_host: String,
    pub ssh_port: u16,
    pub local_port: u16,         // default 8000
    pub cost_per_hr: f64,
    pub launched_at: SystemTime,
}

pub struct TunnelHandle {
    pub child: tokio::process::Child,
    pub local_port: u16,
}
```

`VastState` is built in `main.rs`, passed to `GatewayState` and to the
virtual tool handler the same way `peer_registry` is shared with mesh code.

---

## recipes.toml Format

Location: `/etc/agentd/recipes.toml` (writable by agentd user, same as peers.toml).

```toml
# ApexOS Inference Recipes
# Curated GPU/model combos. Edit or add via the desktop Inference panel.

[docker]
prebuilt = "ghcr.io/buckster123/vastai-gguf:prebuilt"

# ── GPU Tiers ──────────────────────────────────────────────────────────────────
[gpu_tiers.3090]
vast_names = ["RTX_3090"]
label = "RTX 3090  24GB  (~$0.18/hr)"
max_price = "0.30"
min_disk_gb = 50
vram_gb = 24

[gpu_tiers.4090]
vast_names = ["RTX_4090"]
label = "RTX 4090  24GB  (~$0.28/hr)"
max_price = "0.45"
min_disk_gb = 60
vram_gb = 24

[gpu_tiers.5090]
vast_names = ["RTX_5090"]
label = "RTX 5090  32GB  (~$0.34/hr)"
max_price = "0.55"
min_disk_gb = 60
vram_gb = 32

[gpu_tiers.h100-sxm]
vast_names = ["H100_SXM", "H100_SXM5", "H100X"]
label = "H100 SXM  80GB  (~$2.50/hr)"
max_price = "3.50"
min_disk_gb = 100
vram_gb = 80

[gpu_tiers.b200-sxm]
vast_names = ["B200_SXM", "B200"]
label = "B200 SXM  192GB  (~$5+/hr)"
max_price = "9.00"
min_disk_gb = 200
vram_gb = 192

# ── Recipes ────────────────────────────────────────────────────────────────────
[[recipes]]
name        = "qwen36-9b-q8-3090"
label       = "Qwen3.6-9B  Q8  32K  (budget)"
gpu         = "3090"
model_repo  = "unsloth/Qwen3.6-9B-GGUF"
model_quant = "Q8_0"
ctx         = 32768
parallel    = 1
kv_type     = "q8_0"
description = "9B at full quality. Fits with room. Fast cold start."

[[recipes]]
name        = "qwen36-27b-q4-4090"
label       = "Qwen3.6-27B  Q4  64K  (4090)"
gpu         = "4090"
model_repo  = "unsloth/Qwen3.6-27B-GGUF"
model_quant = "UD-Q4_K_XL"
ctx         = 65536
parallel    = 1
kv_type     = "q8_0"
description = "Dense 27B at Q4 on 24GB. Good value."

[[recipes]]
name        = "qwen36-27b-q6-5090"
label       = "Qwen3.6-27B  Q6  96K  ★ default"
gpu         = "5090"
model_repo  = "unsloth/Qwen3.6-27B-GGUF"
model_quant = "UD-Q6_K_XL"
ctx         = 98304
parallel    = 1
kv_type     = "q8_0"
description = "Dense 27B near-lossless. Best single-user throughput on 5090."

[[recipes]]
name        = "qwen36-35b-moe-q5-5090"
label       = "Qwen3.6-35B-MoE  Q5  128K"
gpu         = "5090"
model_repo  = "unsloth/Qwen3.6-35B-A3B-GGUF"
model_quant = "UD-Q5_K_XL"
ctx         = 131072
parallel    = 1
kv_type     = "q8_0"
description = "MoE 35B Q5. Hybrid-linear arch — only 10/40 layers full-KV. Great quality."

[[recipes]]
name        = "carnice-27b-q5-5090"
label       = "Carnice-V2-27B  Q5  96K  🤖 hermes-tuned"
gpu         = "5090"
model_repo  = "kai-os/Carnice-V2-27b-GGUF"
model_quant = "Q5_K_M"
ctx         = 98304
parallel    = 1
kv_type     = "q8_0"
description = "Carnice SFT on Hermes agent traces. A/B vs base Qwen."

[[recipes]]
name        = "carnice-moe-q4-3x256k-5090"
label       = "Carnice MoE 35B-A3B  Q4  3×256K  🤖  (hero)"
gpu         = "5090"
model_repo  = "samuelcardillo/Carnice-Qwen3.6-MoE-35B-A3B-GGUF"
model_quant = "Q4_K_M"
ctx         = 786432
parallel    = 3
kv_type     = "q8_0"
description = "3 parallel 256K slots on 32GB. Hermes orchestration at hero scale."

[[recipes]]
name        = "qwen36-35b-moe-q8-h100"
label       = "Qwen3.6-35B-MoE  Q8  8×256K  (H100)"
gpu         = "h100-sxm"
model_repo  = "unsloth/Qwen3.6-35B-A3B-GGUF"
model_quant = "UD-Q8_K_XL"
ctx         = 2097152
parallel    = 8
kv_type     = "q8_0"
description = "Near-lossless Q8, 8 slots × 256K. Hybrid KV = 2.7GB per slot. ~136 t/s confirmed."

[[recipes]]
name        = "carnice-moe-q8-h100"
label       = "Carnice MoE 35B-A3B  Q8  8×256K  🤖  (H100)"
gpu         = "h100-sxm"
model_repo  = "samuelcardillo/Carnice-Qwen3.6-MoE-35B-A3B-GGUF"
model_quant = "Q8_0"
ctx         = 2097152
parallel    = 8
kv_type     = "q8_0"
description = "Carnice hermes-tuned at H100 scale. A/B vs base for agentic evals."

[[recipes]]
name        = "qwen36-35b-moe-q8-10slot-b200"
label       = "Qwen3.6-35B-MoE  Q8  10×256K  (B200)"
gpu         = "b200-sxm"
model_repo  = "unsloth/Qwen3.6-35B-A3B-GGUF"
model_quant = "UD-Q8_K_XL"
ctx         = 2621440
parallel    = 10
kv_type     = "q8_0"
description = "10 parallel 256K slots on 192GB. Baller tier."
```

---

## Virtual Tools

All handled in `agentd/crates/plugins/src/supervisor.rs` (same pattern as
`bootstrap_node`, `list_mesh_peers`). Each handler: `tokio::spawn` + emit
`Event::ToolResult` when done.

### `vast_list_recipes`

```json
{ "name": "vast_list_recipes", "input": {} }
```

Reads `/etc/agentd/recipes.toml`, returns JSON array of recipe summaries
(name, label, gpu, description). No Vast.ai API call.

---

### `vast_launch`

```json
{
  "name": "vast_launch",
  "input": {
    "recipe": "qwen36-27b-q6-5090",
    "geo": "EU_NORDIC"
  }
}
```

**Execution sequence** (all via `tokio::process::Command`):

```
Step 1: CHECK — is there already a running instance?
  Read VastState.instance — if Some: return error with current instance info

Step 2: FIND OFFER
  vastai search offers "<gpu_filter> reliability>0.99 inet_down>500
    dph_total<<max_price> disk_space><min_disk_gb> rentable=true"
  --order dph_total --raw
  | jq '[.[] | select((.geolocation//"") | test("<geo_re>$"))] | .[0].id'

Step 3: CREATE INSTANCE
  vastai create instance <offer_id>
    --image ghcr.io/buckster123/vastai-gguf:prebuilt
    --disk <min_disk_gb>
    --env "MODEL_REPO=... MODEL_QUANT=... CTX=... KV_TYPE=... MODE=thinking
           PARALLEL=... HOST=127.0.0.1"
    --onstart-cmd "MODEL_REPO=... <env vars> bash /app/launch.sh > /var/log/launch.log 2>&1 &"
    --raw
  Save instance_id to VastState + /var/lib/agentd/workspace/vast/instance.json

Step 4: WAIT FOR SSH
  Poll `vastai show instance <id>` every 10s
  Until actual_status == "running" and ssh_host is set
  Timeout 5 min → error event

Step 5: OPEN TUNNEL
  Spawn: ssh -f -N -o StrictHostKeyChecking=no
         -o ControlMaster=auto -o ControlPath=/tmp/apex-vast-cm
         -o ControlPersist=5m -o ServerAliveInterval=30
         -L 8000:127.0.0.1:8000 -p <ssh_port> root@<ssh_host>
  Store Child in VastState.tunnel
  Emit Event::ToolProgress { message: "tunnel open, waiting for model load..." }

Step 6: WAIT FOR MODEL
  Poll GET http://127.0.0.1:8000/health every 15s
  Until response contains "ok"
  Timeout 20 min (model download + load) → error event
  Emit ToolProgress messages every 30s

Step 7: HOT-SWAP BACKEND
  Emit Event::VastInstanceReady { instance_id, recipe, local_port: 8000 }
  This event is caught in main.rs event loop (same as sensor threshold handler)
  → writes backend_arc: BackendConfig { backend: OAI, url: "http://127.0.0.1:8000/v1", model: <recipe model_repo> }
  → emits SystemStateChanged

Step 8: DONE
  Return: { instance_id, recipe, model, cost_per_hr, slots: N, ctx: K, status: "ready" }
```

**On any step failure**: emit `Event::ToolResult` with error, do NOT leave
orphaned instance. If create succeeded but later steps fail, call
`vast_destroy()` cleanup before returning error.

---

### `vast_status`

```json
{ "name": "vast_status", "input": {} }
```

Returns current VastState. If instance exists: query `vastai show instance <id>`
for live status + compute uptime cost. If no instance: returns `{ "status": "idle" }`.

---

### `vast_destroy`

```json
{ "name": "vast_destroy", "input": {} }
```

1. Kill tunnel process (SIGTERM on Child, remove ControlMaster socket)
2. `vastai destroy instance <id>`
3. Clear VastState.instance + VastState.tunnel
4. Delete `/var/lib/agentd/workspace/vast/instance.json`
5. Emit `Event::VastInstanceDestroyed`
6. Hot-swap backend back to whatever `AGENTD_BACKEND` / `AGENTD_OAI_BASE_URL` env says

---

## Gateway API Routes

All added to `agentd/crates/gateway/src/lib.rs`.

```
GET  /api/vast/recipes               → parse /etc/agentd/recipes.toml, return JSON
GET  /api/vast/status                → VastState as JSON (instance + tunnel health)
POST /api/vast/launch                → body: { recipe, geo? } → enqueue vast_launch virtual tool
POST /api/vast/destroy               → enqueue vast_destroy virtual tool
GET  /api/vast/offers?gpu=5090       → shell out to vastai search offers, return parsed JSON
GET  /api/vast/hf-search?q=qwen      → fetch https://huggingface.co/api/models?search=&filter=gguf, return JSON
```

`/api/vast/offers` is for the recipe builder in the UI — live offer search before
the user commits to a launch. Returns: `[{ id, gpu_name, dph_total, vram_mb, geolocation, reliability }]`

`/api/vast/hf-search` is for HuggingFace model discovery in the recipe builder.
The gateway `http_fetch` via `reqwest` or `curl` subprocess (consistent with
apexos-tools pattern — check if reqwest is already in gateway deps; if not, use
`tokio::process::Command` with `curl`).

---

## SSH Tunnel in Rust

The tunnel is a `tokio::process::Child` — not a shell script.

```rust
let mut cmd = tokio::process::Command::new("ssh");
cmd.args([
    "-f", "-N",
    "-o", "StrictHostKeyChecking=no",
    "-o", "ControlMaster=auto",
    "-o", &format!("ControlPath=/tmp/apex-vast-cm-{instance_id}"),
    "-o", "ControlPersist=5m",
    "-o", "ServerAliveInterval=30",
    "-o", "ExitOnForwardFailure=yes",
    "-L", &format!("8000:127.0.0.1:{remote_port}"),
    "-p", &ssh_port.to_string(),
    &format!("root@{ssh_host}"),
]);
let child = cmd.spawn()?;
```

**Keepalive task**: spawn a `tokio::spawn` loop that pings
`http://127.0.0.1:8000/health` every 30s. If 3 consecutive failures →
emit `Event::VastTunnelLost` → agent notified, can call `vast_launch` again.

**Startup**: if `instance.json` exists at agentd boot (from a prior session),
attempt tunnel reconnect automatically and update backend URL.

---

## Desktop UI — Inference Window

`launchApp('inference')` → WinBox. Title: `⚡ Inference`.

**Layout (top → bottom):**

```
┌─────────────────────────────────────────────────────────────────────┐
│  ⚡ INFERENCE                                              [_][□][X] │
├──────────────┬──────────────────────────────────────────────────────┤
│  ACTIVE      │  backend: Vast.ai  model: Qwen3.6-35B-MoE            │
│  INSTANCE    │  instance: 12345678  status: ● running               │
│              │  recipe: qwen36-35b-moe-q5-5090                      │
│              │  uptime: 00:43:12   cost so far: $0.25               │
│              │  slots: 1  ctx: 128K  geo: SE                        │
│              │  [  Destroy Instance  ]                               │
├──────────────┴──────────────────────────────────────────────────────┤
│  RECIPES                            [+ New Recipe]  [🔄 Refresh]    │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │ 3090                                                         │    │
│  │  qwen36-9b-q8      Q8  32K  budget            [$0.18/hr] ▶  │    │
│  │ 4090                                                         │    │
│  │  qwen36-27b-q4     Q4  64K                    [$0.28/hr] ▶  │    │
│  │ 5090                                                         │    │
│  │  qwen36-27b-q6 ★   Q6  96K  default           [$0.34/hr] ▶  │    │
│  │  qwen36-35b-moe-q5 Q5  128K                   [$0.34/hr] ▶  │    │
│  │  carnice-27b-q5  🤖 Q5  96K  hermes           [$0.34/hr] ▶  │    │
│  │  carnice-moe-hero🤖 Q4  3×256K hero           [$0.34/hr] ▶  │    │
│  │ H100                                                         │    │
│  │  qwen36-35b-q8     Q8  8×256K                 [$2.50/hr] ▶  │    │
│  │  carnice-moe-q8  🤖 Q8  8×256K hermes         [$2.50/hr] ▶  │    │
│  │ B200                                                         │    │
│  │  qwen36-35b-q8     Q8  10×256K                [$5.00/hr] ▶  │    │
│  └─────────────────────────────────────────────────────────────┘    │
│                                                                      │
│  ┌── Launch ─────────────────────────────────────────────────────┐  │
│  │  Recipe: qwen36-27b-q6-5090   Geo: [EU_NORDIC ▾]  [Launch]   │  │
│  │  Live offers: RTX_5090 @ $0.34  SE  rel=0.998  ↓847 Mbps     │  │
│  └───────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────┘
```

**Recipe Builder** ("+New Recipe" button opens a sub-panel):
1. Pick GPU tier (dropdown → loads live offers via `GET /api/vast/offers?gpu=`)
2. Browse HF models (search box → `GET /api/vast/hf-search?q=`) → pick repo + quant
3. Configure ctx, parallel, kv_type
4. Name + save → `POST /api/vast/recipes` → appends to recipes.toml

---

## State & Persistence

```
/var/lib/agentd/workspace/vast/
  instance.json       # current instance (write on launch, delete on destroy)
  events.jsonl        # launch/destroy/cost events (append-only)
```

`instance.json`:
```json
{
  "id": "12345678",
  "recipe": "qwen36-27b-q6-5090",
  "ssh_host": "ssh4.vast.ai",
  "ssh_port": 13337,
  "local_port": 8000,
  "cost_per_hr": 0.34,
  "launched_at": "2026-06-09T18:00:00Z"
}
```

---

## Environment Variables

```
VAST_API_KEY=               # required for any vast_* tool call
VAST_LOCAL_PORT=8000        # local port for SSH tunnel (default 8000)
VAST_DEFAULT_GEO=EU_NORDIC  # geo preference (EU_NORDIC | EU | US | ANY)
VAST_MIN_CUDA=12.8          # minimum CUDA version filter
RECIPES_TOML=/etc/agentd/recipes.toml
```

`VAST_API_KEY` stored in `/etc/agentd/env` alongside `ANTHROPIC_API_KEY`
(same chmod 600, root-owned pattern). Exposed to agentd process via systemd
`EnvironmentFile`. Also settable via desktop Settings → Keys tab (add VAST tab).

---

## New Event Variants

Added to `agentd/crates/core/src/types.rs`:

```rust
VastInstanceLaunched { instance_id: String, recipe: String, cost_per_hr: f64 },
VastInstanceReady    { instance_id: String, local_port: u16 },
VastInstanceDestroyed { instance_id: String },
VastTunnelLost       { instance_id: String },
```

`VastInstanceReady` is caught in `main.rs` event loop to trigger backend hot-swap
(same pattern as `MESH_AUTO_BOOTSTRAP` handling UserPrompt events).

---

## Local PCIe GPU

**Zero new code.** Just config docs:

```
# Pi 5 + PCIe GPU (M.2 HAT+ or Thunderbolt adapter)
# Install Ollama with CUDA/ROCm support
# In /etc/agentd/env:
AGENTD_BACKEND=ollama
AGENTD_OAI_BASE_URL=http://localhost:11434/v1
AGENTD_MODEL=qwen3.6-9b:Q8_0  # (after: ollama pull ...)
```

Pi 5 PCIe is gen 3 x1 (~1 GB/s). Inference is memory-bandwidth-bound on the GPU
itself — the Pi→GPU link ships tokens/KV only. An RTX 3060 12GB on Qwen3.6-9B Q8
runs normally. Tested by community. Document in `docs/claude/pi-deploy.md`.

---

## Build Phases

| Phase | What | Status |
|-------|------|--------|
| 36a | `recipes.toml` format + loader, `GET /api/vast/recipes`, `vast_list_recipes` virtual tool — no Vast API yet | ☐ |
| 36b | `vast_launch` + `vast_destroy` + `vast_status` virtual tools — full Vast.ai lifecycle, `instance.json` persistence | ☐ |
| 36c | SSH tunnel manager (tokio::process::Child, keepalive ping, `VastTunnelLost` event, auto-reconnect on boot) | ☐ |
| 36d | Backend hot-swap on `VastInstanceReady` + revert on `VastInstanceDestroyed`; gateway `/api/vast/*` routes | ☐ |
| 36e | Desktop Inference window — recipe browser, instance status, cost ticker, launch/destroy controls | ☐ |
| 36f | Recipe builder — live offer search (`/api/vast/offers`), HF model search (`/api/vast/hf-search`), save to recipes.toml | ☐ |

**Deploy requirement**: `vastai` CLI must be installed on the Pi (`pip install vastai`).
Add to `install.sh`. `VAST_API_KEY` set in `/etc/agentd/env`.

---

## Deferred (Track B — Step 37 Nursery)

| Item | Notes |
|------|-------|
| Dataset collection | `collect_dataset(session_id)` — extract high-quality turns from session JSONL → ShareGPT format |
| Training dispatch | `train_apprentice(dataset_id, base_model)` — Together fine-tune API or Vast + Axolotl/Unsloth container |
| Apprentice registry | `apprentices.jsonl` in workspace (no Postgres — JSONL like everything else in ApexOS) |
| `activate_apprentice(id)` | Push trained model to HF → pull in Ollama → hot-swap backend |
| Nursery desktop window | Training job progress, apprentice roster, A/B eval launcher |
| Together.ai fine-tune | Together has a working fine-tune API, tested once in ApexAurum-local. Slot in when nursery is ready. |
| vLLM multi-GPU | Multi-GPU tensor-parallel recipes. Already have the Docker image. Deferred until single-GPU is solid. |

---

## Key Constraints

| Constraint | Detail |
|-----------|--------|
| `vast_launch` is async | Returns immediately (ToolProgress events during boot). Model load can take 10-20 min on slow nodes. |
| Tunnel is fragile | Pi home network → Vast geo → SSH. Keepalive essential. ControlMaster reduces per-call latency from ~500ms to ~RTT. |
| `HOST=127.0.0.1` in container | llama-server binds only localhost inside container — SSH tunnel is the ONLY access path |
| VAST_API_KEY required | Tools fail gracefully with clear error if key not set |
| One instance at a time | No multi-instance for now. `vast_launch` refuses if instance already exists. |
| pi 3B+ cannot run vastai CLI | Python-heavy. Vast tools are a Tier 1/2 node feature only. |
| Recipe editor UI saves recipes.toml | agentd must have write permission to `/etc/agentd/recipes.toml` (same as peers.toml — already in ReadWritePaths) |
