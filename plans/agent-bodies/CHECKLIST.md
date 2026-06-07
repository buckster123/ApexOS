# Phase 6 — Agent Bodies

Agent's own wishlist. Priority order matches what the agent said:
"Without shell I'm a brain in a jar." Start there; everything else layers on top.

---

## Philosophy

The agent IS almost the OS on this machine — the Pi hosts nothing else.
That means **deep access within safety bounds**, not a sandboxed toy.

- Full read everywhere (`/proc`, `/sys`, `/etc`, `/var`, …)
- Full write to `/var/lib/agentd/`, `/home/`, `/tmp/`, `/etc/agentd/`
- System ops (`systemctl`, `apt`, `pip`) via **ask** policy — user approves
- Hard denylist in the binary for operations that could wipe the host (below)
- No jail, no chroot — the policy engine is the gate, not the filesystem

---

## Locked decisions

- All new tools = MCP-over-stdio plugins registered in `plugins.toml`
- Tier 1 + system stats → single Rust binary `apexos-tools` (new workspace crate)
- GPIO → Python MCP (gpiozero, no usable Rust bindings for Pi 5 yet)
- Scheduler → virtual tool baked into agentd (no new process; fires `UserPrompt` on the bus)
- Notifications → decision deferred; see Phase 6c options
- Policy defaults: shell=`ask`, filesystem write=`auto-edit`, read/fetch/stats=`yolo`

---

## Shell denylist (hard-coded in `apexos-tools`, not overridable by policy)

Checked in `run_command` before any exec. Reject if the expanded command matches:

```
# Disk destruction
rm -rf /            (and --no-preserve-root variants)
mkfs.*              (any filesystem format)
dd  … of=/dev/sd*   (raw device writes)
dd  … of=/dev/nvme* 
dd  … of=/dev/mmcblk*
fdisk /dev/…        (partition table edits on real devices)
parted /dev/…
gdisk /dev/…
wipefs

# System self-destruction
rm -rf /usr  /bin  /lib  /sbin  /boot
> /etc/passwd   > /etc/shadow   (truncate auth files)

# Fork bombs (pattern match)
:(){ :|:& };:
```

Everything else: allowed, subject to policy (`ask` by default).

---

## Phase 6a — `apexos-tools` Rust MCP server ✓

**Goal:** shell execution + filesystem + HTTP fetch + Pi system stats in one binary.

New workspace crate: `tools/crates/apexos-tools/`

### Tools

| Tool | Signature | Policy default |
|---|---|---|
| `run_command` | `cmd, cwd?, env?, timeout_secs?` → `{stdout, stderr, exit_code, timed_out}` | `ask` |
| `read_file` | `path, max_bytes?` → `{content, size_bytes, truncated}` | `yolo` |
| `write_file` | `path, content, append?` → `{bytes_written}` | `auto-edit` |
| `list_dir` | `path, recursive?` → `[{name, kind, size, modified}]` | `yolo` |
| `create_dir` | `path` → `{created}` | `auto-edit` |
| `delete_path` | `path, recursive?` → `{deleted}` | `ask` |
| `http_fetch` | `url, method?, headers?, body?` → `{status, body, headers}` | `yolo` |
| `cpu_temp` | — → `{temp_c, sensor}` | `yolo` |
| `disk_usage` | `path?` → `[{mount, total_gb, used_gb, free_gb, pct}]` | `yolo` |
| `memory_info` | — → `{total_mb, available_mb, used_mb, swap_used_mb}` | `yolo` |
| `uptime` | — → `{uptime_secs, load_avg_1, load_avg_5, load_avg_15}` | `yolo` |

### MCP server implementation

Speaks JSON-RPC 2.0 over stdio (same protocol as CerebroCortex client expects).
Must handle: `initialize`, `tools/list`, `tools/call`.
No tokio needed — sync stdio + `std::process::Command` is fine.
Use `reqwest` (blocking feature) for `http_fetch`.

### `plugins.toml` entry (to add after build)

```toml
[[plugin]]
id      = "apexos-tools"
cmd     = "/usr/local/bin/apexos-tools"
restart = "always"
```

### Policy rules to add to `policy.toml`

```toml
"run_command"  = "ask"
"write_file"   = "auto-edit"
"delete_path"  = "ask"
"read_file"    = "yolo"
"list_dir"     = "yolo"
"create_dir"   = "auto-edit"
"http_fetch"   = "yolo"
"cpu_temp"     = "yolo"
"disk_usage"   = "yolo"
"memory_info"  = "yolo"
"uptime"       = "yolo"
```

### Checklist

- [x] Create `tools/` workspace alongside `agentd/` (separate Cargo workspace — keeps build times independent)
- [x] `tools/crates/apexos-tools/` — binary crate, `Cargo.toml` with `serde_json`, `reqwest` (blocking)
- [x] Implement MCP stdio loop: read line → parse JSON-RPC → dispatch → write response
- [x] `tools/list` handler — enumerate all tools with JSON Schema for each parameter
- [x] `run_command` — `std::process::Command` with timeout via mpsc channel; denylist check first
- [x] `read_file` — `fs::File::read` with optional byte cap
- [x] `write_file` — `OpenOptions` with append support; creates parent dirs
- [x] `list_dir` — `fs::read_dir` with optional recursion (depth-limited, max 3 levels)
- [x] `create_dir` — `fs::create_dir_all`
- [x] `delete_path` — `fs::remove_file` or `fs::remove_dir_all` (denylist + protected-path check)
- [x] `http_fetch` — blocking reqwest, 30s timeout, body size cap 4MB
- [x] `cpu_temp` — read `/sys/class/thermal/thermal_zone*/temp` (divide by 1000), all zones reported
- [x] `disk_usage` — `/proc/mounts` + `df -B1` per mount; filters pseudo-fs
- [x] `memory_info` — parse `/proc/meminfo`
- [x] `uptime` — parse `/proc/uptime` + `/proc/loadavg`
- [x] Build on Pi: `cd ~/ApexOS/tools && ~/.cargo/bin/cargo build --release` (1m 12s)
- [x] Copy binary: `sudo cp target/release/apexos-tools /usr/local/bin/`
- [x] Register in `plugins.pi.toml` + policy rules in `policy.toml`; deployed to `/etc/agentd/`
- [x] Smoke test: `uname -a` → `Linux ApexOS 6.12.75+rpt-rpi-2712 aarch64 GNU/Linux`; `/etc/os-release` read; `http_fetch` all pass
- [x] Smoke test: agent self-reported vitals — uptime 22.6h, CPU 39.7°C, RAM 1121/4045MB used — all live on first turn after deploy
- [x] Verify denylist: `rm -rf /` → `"BLOCKED: rm -rf / is blocked"` — caught cleanly, never executed
- [x] Commit: `feat(tools): apexos-tools MCP server — shell, fs, http, sysstat`

---

## Phase 6b — Scheduler virtual tool ✓

**Goal:** Agent can schedule future tasks without human trigger. Real daemon behaviour.

Baked into `agentd/crates/agentd/src/main.rs` as a new virtual tool (same pattern as `propose_evolution`, `rollback_evolution`, `read_soul_md`).

### Virtual tools

| Tool | Description |
|---|---|
| `schedule_task` | `cron: String, prompt: String, session_id?: u64` — add a scheduled task |
| `list_schedules` | — list all active scheduled tasks |
| `cancel_schedule` | `schedule_id: String` — cancel by ID |

### Storage

`/var/lib/agentd/schedules.jsonl` — one JSON object per line:
```json
{"id":"sched_abc123","cron":"0 8 * * *","prompt":"morning health check","created_at":1234567890,"last_run":null}
```

### Runtime

- Background tokio task polls schedules every 60s
- When a cron expression fires: emit `Event::UserPrompt { session: root_session, text: prompt }`
- Loaded on startup from `schedules.jsonl`; persisted on every add/cancel
- Use `cron` crate for expression parsing

### Checklist

- [x] Add `cron = "0.12"` + `chrono = "0.4"` + `serde` to `agentd/crates/agentd/Cargo.toml`
- [x] `agentd/crates/agentd/src/scheduler.rs` — `ScheduledTask` struct, JSONL load/save, `run_scheduler()`, `spawn_scheduler_handler()`
- [x] `schedule_task_spec()`, `list_schedules_spec()`, `cancel_schedule_spec()` — registered in `gather_tools()`
- [x] `SupervisorCmd::SetScheduleTx` + `schedule_tx` field + `set_schedule_tx()` in supervisor
- [x] Supervisor dispatch: `schedule_*` tools forwarded via `(session, call_id, tool, args)` channel
- [x] `run_scheduler` + `spawn_scheduler_handler` spawned in `main()` after evolution applier
- [x] Policy rules: all three = `yolo`
- [x] Smoke test: hotloaded without restart — agent scheduled, fired autonomously, cancelled; all pass
- [x] Cancel it; verified no more fires
- [ ] Daemon restart; verify schedule restored (deferred — low risk given JSONL persistence)
- [x] Commit: `feat(scheduler): schedule_task virtual tool — cron-driven autonomous turns`

---

## Phase 6c — Notifications ✓

**Goal:** Agent can reach the user proactively, without user initiating.

### Architecture decision (2026-06-07): notification surface stack

Single `notify` tool, tries surfaces in priority order, uses whatever is configured/available:

```
1. JSONL log        ← /var/lib/agentd/notifications.jsonl — always, free, queryable by agent
2. Kiosk toast      ← notify-send → cage compositor → HDMI display (zero config)
3. Audio chime      ← aplay /usr/share/sounds/ (zero config, Pi 5 has HDMI audio)
4. TTS              ← espeak-ng first (always on Debian), piper if installed (neural, better)
5. ntfy.sh          ← if NTFY_TOPIC set in /etc/agentd/env (phone reach-out, no account)
6. Telegram         ← if BOT_TOKEN + CHAT_ID set (for repo users who want it)
```

First three surfaces: zero external deps, work fully offline. TTS on the 8GB Pi with HDMI
display is the flagship local experience — agent literally speaks its notifications.
Telegram included for repo portability (user currently without a daily-driver phone for it).

**TTS default**: `espeak-ng` first (always installed), detect `piper` binary + model and use
if present. No opt-in env var needed — if audio device exists and espeak-ng is installed, use it.
Piper model path: `/var/lib/agentd/piper/` (agent can download a voice model via http_fetch).

**Implementation**: add `notify` tool to existing `apexos-tools` binary (not a separate binary).
Tool signature: `notify(message, title?, priority?, surfaces?)` where surfaces defaults to all configured.

### MCP tool

| Tool | Signature | Policy |
|---|---|---|
| `notify` | `message, title?, priority?` | `yolo` |

### Checklist

- [x] Add `notify` function to `tools/crates/apexos-tools/src/tools.rs`
- [x] Surface 1: append to `/var/lib/agentd/notifications.jsonl` — always first, unconditional
- [x] Surface 2: `notify-send` fire-and-forget (silently succeeds if no daemon)
- [ ] Surface 3: `aplay` chime — deferred (no chime file included; espeak-ng covers audio)
- [x] Surface 4: TTS — `espeak-ng -s 145` (message via arg); piper if PIPER_MODEL env set (passes message via env var to avoid shell-quoting issues)
- [x] Surface 5: ntfy.sh — reqwest POST to `https://ntfy.sh/{NTFY_TOPIC}` if env set
- [x] Surface 6: Telegram — POST Bot API if TELEGRAM_BOT_TOKEN + TELEGRAM_CHAT_ID env set
- [x] Add `notify` to `tools/list` handler and policy.toml (`allow`)
- [x] Fixed policy.toml: Rule enum only supports allow/ask/workspace; yolo→allow, auto-edit→workspace
- [x] Install espeak-ng on Pi: `apt-get install -y espeak-ng`
- [x] Add agentd user to audio group: `usermod -aG audio agentd`
- [x] Build + deploy: 5.82s compile, `apexos-tools` up — 12 tools
- [x] Smoke test as agentd user: `surfaces_fired: ["jsonl","notify-send","tts"]`, surfaces_failed: [] — all three local surfaces clean
- [x] JSONL verified: `{"ts":1780846102,"title":"ApexOS","message":"Agent OS notification system online..."}` written
- [x] Commit + push: `add notify tool` + `fix policy.toml rule values`

---

## Phase 6d — Satellite body-pi + SensorEvent bus ✓

**Goal:** Sensors are first-class bus events, not tool poll responses. Agent perceives
the physical world the same way it perceives WS messages — reactively.

### Architecture decision (2026-06-07): Option B — satellite agentd

Chosen over Option A (MCP bridge) because sensors as *events* fit the actor model better
than sensors as *tool calls*. A temperature spike should interrupt the agent, not wait for polling.

**Topology:**
```
[body-pi]                          [ApexOS Pi]
 rppal reads GPIO/I2C/SPI    →     SensorEvent on broadcast bus
 lightweight Rust daemon      →     agent router reacts (no polling needed)
 apex-sensor-bridge binary    WS→   gateway WS endpoint (new: /sensor-bridge)
```

**body-pi hardware:** The original SensorHead Pi — same Pi that ran the Python MCP service.
ApexOS is on a USB drive, boots from USB or NVMe interchangeably. Switch Pi by plugging
USB drive into sensor Pi and booting. Sensor Pi has free 40-pin header (no Hailo HAT).

**SensorHead was proof-of-concept** — Python MCP wrappers over GPIO. This replaces it
entirely with Rust: `rppal` for GPIO/I2C/SPI, native Rust MCP → WS bridge, no Python layer.

### New types needed in `core/types.rs`

```rust
// SensorEvent variants — first-class bus citizens
pub enum SensorReading {
    Temperature { celsius: f32, sensor_id: String },
    Humidity    { percent: f32, sensor_id: String },
    Pressure    { hpa: f32, sensor_id: String },
    Motion      { detected: bool, sensor_id: String },
    Distance    { cm: f32, sensor_id: String },
    GpioLevel   { pin: u8, high: bool },
}

// New Event variant
Event::SensorReading { node_id: String, reading: SensorReading, timestamp: u64 }
```

### New binaries

| Binary | Crate | Role |
|---|---|---|
| `apex-sensor-bridge` | `tools/crates/apex-sensor-bridge/` | Runs on body-pi; reads sensors via rppal; forwards SensorEvent WS frames to ApexOS gateway |
| `apex-gpio` | `tools/crates/apex-gpio/` | MCP server for direct GPIO on any Pi without Hailo; stub on ApexOS Pi |

### Gateway change

New WS endpoint `/sensor-bridge` accepts authenticated connections from body-pi nodes.
Receives `SensorReading` JSON frames, emits `Event::SensorReading` on the broadcast bus.
Agent router reacts: new `Ok(Event::SensorReading { .. })` arm can trigger a turn if
thresholds crossed (e.g. temp > 80°C, motion detected while scheduled away).

### Build results

- [x] Add `SensorReading` enum + `Event::SensorReading` to `core/types.rs`
- [x] Add `state.rs` apply arm (no-op — SensorReading is consumed by agent router, not folded into SystemState)
- [x] New gateway WS endpoint `/sensor-bridge` — token-gated (SENSOR_BRIDGE_TOKEN env; empty = open for local-only)
- [x] `tools/crates/apex-sensor-bridge/` — Rust binary; tungstenite WS client; reads sysfs CPU temp; reconnects on drop
- [x] Forward loop: read → Event::SensorReading JSON → WS send → gateway emits on broadcast bus
- [x] Agent router: `SensorReading::Temperature > 85°C` → `UserPrompt` on root session (alert turn fires autonomously)
- [x] Agent router: `SensorReading::Motion { detected: true }` → `UserPrompt` on root session
- [x] `apex-sensor-bridge.service` — systemd unit, `Requires=agentd.service`, `Restart=always`
- [x] Deployed: service enabled + started on sensor Pi (192.168.0.158)
- [x] Smoke test: `[sensor-bridge] node connected` + `ApexOS: Temperature { celsius: 35.3, sensor_id: "cpu_thermal" }` in agentd logs
- [x] Both services running: `systemctl status agentd apex-sensor-bridge` both active
- [x] Commit + push: `add SensorEvent bus + /sensor-bridge gateway + apex-sensor-bridge daemon`
- [x] **Real sensors (Phase 6d complete)**: SensorHead installed on USB disk (~/SensorHead), BSEC2 egg copied from NVMe, all sensors live
      - BME688: BSEC2 v2.6.1.0, T/RH/P + IAQ/CO₂eq/VOC flowing as AirQuality events
      - MLX90640: thermal frame (min/max/mean) flowing as ThermalFrame events every 30s
      - i2c-dev modprobe on boot (/etc/modules), i2c-arm=on + 400kHz in config.txt
      - bme68x BSEC2 egg + lgpio .so symlinked into SensorHead venv
      - sensorhead-dashboard.service runs as apexos, BLINKA_LGPIO=1, data at ~/SensorHead/data
      - Agent router: IAQ > 150 with accuracy >= 2 fires autonomous alert turn
      - All three services running: agentd + sensorhead-dashboard + apex-sensor-bridge
      Smoke test: `AirQuality { iaq: 50.0, temperature_c: 21.18, humidity_pct: 61.62 }` + `ThermalFrame { min_c: 25.2, max_c: 33.8 }` in agentd logs
- [x] Register sensor-mcp as MCP plugin in plugins.toml (pull-mode: agent queries sensors on demand)
      - Wrote `tools/sensor-head-mcp` proxy (stdlib-only Python, no direct I2C — routes to dashboard REST)
      - 8 tools: sense_environment, read_thermal, sense_thermal, capture_visual, capture_night, detect_objects, classify_scene, get_head_status
      - Registered in /etc/agentd/plugins.toml; `plugin 'sensor-head' up — 8 tools` confirmed
      - Policy rules added (sense/read/detect/classify/status = allow; capture = ask)
      - Smoke: sense_environment returns 20.56°C, 63.42% RH, IAQ 50 (excellent); read_thermal min 25.4 / max 33.6°C
- [ ] Add GPIO level reads (digital sensors) — rppal InputPin, configurable pin list via env
- [ ] `apex-gpio` MCP tool for manual GPIO reads — deferred to Phase 7

---

## Deferred (out of scope for Phase 6)

- **Git operations** — agent can already use `run_command` with `git` once 6a is done
- **Home Assistant** — wire via `http_fetch` for now; dedicated MCP if HA usage grows
- **Web search** (vs fetch) — Brave Search API or SerpAPI; add to `apexos-tools` in a 6a follow-up
- **Self-modifying Rust code** — agent writes a new virtual tool, daemon hot-reloads it (Phase 7?)
- **Voice I/O** — microphone/speaker on the Pi (Phase 7?)

---

## Rehydration (start of a new session)

1. Read `CLAUDE.md` + this file
2. Check which phases have `✓` vs `✗`
3. Pick up at the first unchecked item in the first incomplete phase
4. Pi SSH: `ssh apexos@192.168.0.114` pw `abnudc1337` (LAN only)
5. Build: `cd ~/ApexOS/tools && ~/.cargo/bin/cargo build --release` (6a) or `cd ~/ApexOS/agentd && ~/.cargo/bin/cargo build --release` (6b+)
6. Deploy binary: `sudo systemctl stop agentd && sudo cp ... /usr/local/bin/ && sudo systemctl start agentd`
7. For `apexos-tools`: stop agentd not required — it's a separate process managed by supervisor

## Notes accumulated across sessions

- **run_command timeout**: `JoinHandle::join_timeout` is not stable Rust — use `mpsc::channel` + `recv_timeout` instead.
- **disk_usage**: Using `df -B1` subprocess (one per mount entry) avoids direct `statvfs` FFI; slower but simpler and portable.
- **reqwest blocking feature**: must be declared explicitly (`reqwest = { features = ["blocking"] }`); tokio runtime not needed.
- **`plugins.pi.toml`**: Pi production config; deployed to `/etc/agentd/plugins.toml` by hand. Dev config (`plugins.toml`) uses local paths for cerebro.
- **SensorHead on Pi 5 Debian trixie**: `i2c-dev` module not auto-loaded; must `modprobe i2c-dev` + add to `/etc/modules`. `dtparam=i2c_arm=on` + `dtparam=i2c_arm_baudrate=400000` in `/boot/firmware/config.txt`. Reboot required.
- **SensorHead venv lgpio**: Pi 5 uses `lgpio` but it won't compile from pip. Fix: symlink system `lgpio.py` + `_lgpio.cpython-313-aarch64-linux-gnu.so` from `/usr/lib/python3/dist-packages/` into the venv. Set `BLINKA_LGPIO=1` env var.
- **SensorHead bme68x egg**: copy egg from NVMe `hailo` install OR from any previous SensorHead venv. Add `bme68x.pth` pointing to the egg in venv site-packages. BSEC2 state lives at `SENSORHEAD_DATA_DIR` (default now env-overridable in local config.py patch).
- **sensorhead-dashboard PYTHONPATH**: needs `SensorHead/` dir (for sensor_head package) + bme68x egg path (since pip install -e fails with setuptools flat-layout discovery on pics/data dirs).
- **`/var/lib/agentd/`**: mode 700, owned by `agentd` user. apexos user cannot write there. Use `~/SensorHead/data` for sensorhead state or change owner.
- **scheduler cron format**: 6-field (second minute hour day month weekday), not standard 5-field. `"0 0 8 * * *"` = 8am daily.
- **6d: switch Pi first** — sensor Pi has free 40-pin header (no Hailo). ApexOS USB drive boots on either Pi interchangeably.
- **6d: gateway auth** — `/sensor-bridge` WS endpoint needs a shared secret in `/etc/agentd/env` (`SENSOR_BRIDGE_TOKEN`) so body-pi can authenticate.
