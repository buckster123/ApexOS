# ApexOS Mesh Tiers — Hardware Roles & Expansion Roadmap

Design document for tiered multi-Pi mesh deployment.
Load this when working on role-aware install, hardware capability detection,
or expanding the mesh beyond the initial two-node setup.

---

## Vision

The mesh is a **self-expanding colony of agents**. Each Pi contributes what
it can. A Pi 5 orchestrates; a Pi 3B+ watches a door sensor; a Zero W2
pipes CO₂ data. The agent layer is uniform — `agentd` runs everywhere —
but the *plugin manifest* and *system prompt soul* adapt to hardware reality.

Eventually: an agent that knows it needs a sensor node in the garage orders
a Pi Zero 2W from Pi Supply, bootstraps it when it arrives, and wires it
into the mesh. Zero human steps.

---

## Confirmed Hardware Inventory (Andre's bench, June 2026)

| Unit | Status | Notes |
|------|--------|-------|
| Pi 5 (×2, sensor node + dev) | ✓ Active | Production fleet. Sensor Pi = 192.168.0.158, 8GB, NVMe, USB boot |
| Pi 5 (×2, HTPC + moOde amp) | Off-limits | HTPC daily drivers; moOde + IanCanada DAC/reclocker. PSUs borrowable short-term for mesh tests |
| Pi 5 (×2, need power fix) | Hardware queue | Need PSU repair / soldering. Ideal full nodes once fixed |
| Pi 4 | Wonky | Network/USB controller hit. Check `ip link` before investing. Ethernet may still be fine — USB damage usually hits kbd/storage more than NIC |
| Pi 3B+ | Probably functional | Untested recently. 1GB RAM, ARMv8 64-bit capable. See tier table below |
| Pi Zero W2 | Speculative | Not confirmed in inventory but modelled for future. Can simulate with 3B+ throttled |

**PSU situation**: Pi 5 and Pi 4 = USB-C. Pi 3B+ = MicroUSB. Cables need digging out from garage/junk drawer. HTPC Pi PSUs are the short-term bridge.

---

## Tier Model

### Tier 1 — Full Node (Pi 5)
**Runs everything.**

```
agentd
  ├── CerebroCortex (ChromaDB + Python, ~300MB RAM)
  ├── apexos-tools (shell, fs, http, sysstat)
  ├── sensor-head-mcp (BME688 + MLX90640 if wired)
  ├── hermes-sonus (music gen, optional)
  └── Council engine (multi-agent, local inference or cloud)

Voice: whisper.cpp (STT) + piper (TTS) — full duplex
Wake word: apex-wake.py + apex-wake.service
Cage kiosk: Wayland display if HDMI attached
Backend: Anthropic (default) or Ollama local inference
```

RAM budget: ~600MB with all plugins idle. Comfortable on 8GB Pi 5.

### Tier 2 — Capable Node (Pi 4)
**Runs everything except heavy local inference.**

Community-validated: whisper.cpp STT works on Pi 4 (one-way, overclocked + SSD).
Cerebro confirmed: 4-agent council with simultaneous embedding calls held at 100% CPU
for 2 min, completed clean. This is the worst-case load — normal operation is fine.

```
agentd
  ├── CerebroCortex ✓ (tight but functional, proven under council load)
  ├── apexos-tools ✓
  ├── sensor-head-mcp ✓ (if hardware attached)
  └── Council engine ✓ (cloud inference recommended, not local Ollama)

Voice: STT one-way (whisper tiny.en on SSD, overclocked). TTS (espeak-ng) fine.
Wake word: borderline — worth trying, may miss words under load
Local inference: skip Ollama — use cloud backend (Anthropic/OpenRouter)
```

**Pi 4 PSU note**: USB-C 5V/3A. Same cable family as Pi 5 but lower amperage spec.

### Tier 3 — Sensor/Edge Node (Pi 3B+)
**Runs agentd + tools. No Cerebro. No heavy voice.**

Pi 3B+: 1GB RAM, ARMv8 (64-bit capable if running 64-bit OS), 100Mbps ethernet,
no PoE header. Build time: **~45 min** for full release build (bootstrap_node handles
this — backgrounds and returns immediately with PID).

```
agentd
  ├── apexos-tools ✓ (shell, fs, sysstat — lightweight)
  ├── sensor-head-mcp ✓ (sensor data relay)
  └── CerebroCortex ✗ (skip — ChromaDB + Python OOMs on 1GB under load)

Voice: espeak-ng TTS only (no whisper — too slow/heavy)
Wake word: no
Council: no (no embedding backend)
Backend: Anthropic or OpenRouter cloud only (no local Ollama)
```

This node's value is **sensor reach + tool execution in that location**. It feeds
data to a Tier 1/2 hub via A2A and reports sensor anomalies autonomously.

Recommended soul.md for Tier 3: minimal, sensor-focused. No self-evolution proposals
(no Cerebro to track episodes). Read-only policy for most tools.

### Tier 4 — Micro Node (Zero W2, speculative)
**Proof of concept. Bare minimum agentd.**

Pi Zero 2W: 512MB RAM, ARMv8 quad-core 1GHz, WiFi only (no ethernet). Build time: **~2 hours**.

```
agentd
  ├── apexos-tools (subset: sysstat + sensor only — no shell exec)
  └── sensor-head-mcp (if I2C sensor attached)
  
Everything else: no
```

Primary use: ultra-cheap sensor bridge. Could run `apex-sensor-bridge` standalone
(without full agentd) and POST to a Tier 1 node's `/sensor-bridge` endpoint.
Full agentd is borderline — worth testing with swapfile on SD card.

**Simulate now**: throttle a Pi 3B+ to 1 core + 512MB cgroup limit to validate
before buying Zero hardware.

---

## Role-Aware Install

**Current state**: `install.sh` is one-size-fits-all. Installs everything.

**Planned**: `install.sh --role <tier>` flag.

```bash
# Full install (default, Tier 1/2)
bash install.sh

# Sensor/edge install (Tier 3)
bash install.sh --role sensor

# Micro install (Tier 4, experimental)
bash install.sh --role micro
```

What changes per role:

| Component | full | sensor | micro |
|-----------|------|--------|-------|
| agentd binary | ✓ | ✓ | ✓ |
| CerebroCortex | ✓ | ✗ | ✗ |
| apexos-tools (all 11) | ✓ | subset | 2-3 tools |
| whisper.cpp + piper | ✓ | ✗ | ✗ |
| apex-wake.service | ✓ | ✗ | ✗ |
| cage-kiosk.service | ✓ | ✗ | ✗ |
| plugins.toml | full | sensor.toml | micro.toml |
| soul.md | full | sensor soul | micro soul |

**`plugins.toml` variants** to create:
- `config/plugins-sensor.toml` — apexos-tools + sensor-head-mcp only
- `config/plugins-micro.toml` — apexos-tools (sysstat subset) only
- `config/soul-sensor.md` — sensor-focused persona, no evolution
- `config/soul-micro.md` — minimal watchdog persona

**PeerRole already exists in code** (`gateway/src/mesh.rs`): `Full | Sensor | Thin`.
bootstrap_node should pass `--role` to the remote install.sh and register the peer
with the matching role. The Mesh panel already shows role badges.

---

## Live Mesh Test Plan

### Phase 1 — Two Pi 5s (first real mesh)
**Prerequisites**: power fix on the spare Pi 5s (soldering), borrow HTPC PSUs.

1. Bring up second Pi 5 on LAN, fresh Debian trixie install
2. Set hostname: `hostnamectl set-hostname apex-node2`
3. Run `bash install.sh` (full tier) — or use `bootstrap_node` from main node
4. Watch main node's Mesh panel: node2 appears in Discovered within 60s
5. Register → send `send_to_agent(session_id: 0, node: "apex-node2", message: "hello")`
6. **First cross-node agent message** — the milestone

### Phase 2 — Pi 3B+ as sensor node
**Prerequisites**: find MicroUSB cable, flash SD card, test Ethernet.

1. Fresh Debian trixie on SD (or USB if the 3B+ supports it — it does with bootloader update)
2. `bash install.sh --role sensor` (once role flag is implemented)
3. Set hostname: `hostnamectl set-hostname apex-sensor2`
4. Bootstrap from main node OR manual install
5. Validate: agentd up, apexos-tools and sensor-head tools visible, NO Cerebro
6. Attach a BME688 or DHT22 if available — feed readings into main node's timeline

### Phase 3 — Pi 4 viability check
**Prerequisites**: check ethernet with `ip link show eth0`, basic connectivity test.

1. If eth0 is up and stable: proceed with full or Tier 2 install
2. If eth0 is flaky: use WiFi dongle or accept it's a desktop-only node
3. Test council mode: 4-agent round, watch CPU. Already validated by Andre — should hold.

---

## The Self-Expanding Mesh Vision

Once A2A routing and bootstrap_node are solid:

```
Agent on apex-main detects sensor gap ("I have no temperature coverage in zone 3")
→ Checks mesh peers — no node covers that zone
→ Decides a Pi Zero 2W + DHT22 ($12 total) would fix it
→ Calls notify() to inform Andre: "Ordering sensor node for zone 3 — approve?"
→ (With MESH_AUTO_BOOTSTRAP + yolo policy): places order via http_fetch to Pi Supply API
→ When hardware arrives, bootstrap_node provisions it automatically
→ Mesh panel shows new node online
→ Sensor readings start flowing
```

This requires:
- A procurement tool (http_fetch already exists — just needs an e-commerce endpoint)
- Andre's approval gate (suggest mode = human confirms order)
- bootstrap_node already handles the provisioning side

The agent ordering its own hardware is not a future fantasy — every component
exists or is one tool away. It's a policy question, not an engineering one.

---

## Key Constraints to Remember

| Constraint | Detail |
|-----------|--------|
| Always build on target Pi | Never scp x86 binaries. ARMv7 vs ARMv8 vs AArch64 are different targets |
| Pi 3B+ build time | ~45 min release build. bootstrap_node already handles this (nohup + returns PID) |
| CerebroCortex = 1GB minimum | Hard floor. 3B+ gets OOM under embedding load. Tier 3 = no Cerebro |
| ws_url format | Store as `ws://host:8787` — NO `/ws` suffix. HTTP proxy strips `ws://` → `http://`, appends API path |
| MicroUSB cables | Pi 3B+ = MicroUSB 5V/2.5A. Pi 4 = USB-C 5V/3A. Pi 5 = USB-C 5V/5A. Don't mix PSUs |
| MESH_SUBNET_GUARD=true | Default on. Only peers in same /24 as eth0 are considered. Prevents mesh sprawl into building LAN |
| Pi 4 NIC check | `ip link show eth0` before investing time. USB controller damage ≠ NIC damage (different silicon) |

---

## Open Items / Future Steps

| Item | When |
|------|------|
| `install.sh --role <tier>` flag | Before Phase 2 (3B+ test) |
| `plugins-sensor.toml` + `soul-sensor.md` | Same commit as role flag |
| `bootstrap_node` passes `--role` arg to remote install | Same |
| Peer health check: `PeerLost` after 3 missed polls | After two-node Phase 1 is proven |
| Cross-node council (personas spawn on peer nodes) | After A2A is live-tested |
| Cerebro sync across nodes (shared via CEREBRO_URL env) | Later — per-node Cerebro is default |
| Zero W2 validation (or 3B+ throttle simulation) | After Phase 2 |
| Procurement tool (agent orders hardware) | When mesh is stable and trusted |
