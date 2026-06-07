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

## Phase 6b — Scheduler virtual tool ✗

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

- [ ] Add `cron = "0.12"` to `agentd/Cargo.toml`
- [ ] `agentd/crates/agentd/src/scheduler.rs` — `ScheduledTask` struct, load/save, `run_scheduler()`
- [ ] `schedule_task_spec()` — tool spec registered in `gather_tools()`
- [ ] `list_schedules_spec()`, `cancel_schedule_spec()`
- [ ] Supervisor dispatch: handle `schedule_task` / `list_schedules` / `cancel_schedule` in `propose_tool_dispatch()`
- [ ] `run_scheduler` spawned in `main()` — tokio task, fires `UserPrompt` on the bus
- [ ] Policy rules: all three = `yolo` (agent manages its own schedule)
- [ ] Smoke test: schedule `"* * * * *"` (every minute) "what time is it?"; verify agent self-fires within 2 minutes
- [ ] Cancel it; verify no more fires
- [ ] Daemon restart; verify schedule restored
- [ ] Commit: `feat(scheduler): schedule_task virtual tool — cron-driven autonomous turns`

---

## Phase 6c — Notifications ✗

**Goal:** Agent can reach the user proactively, without user initiating.

### Service options

| Option | Pros | Cons |
|---|---|---|
| **Telegram** | Widely used, reliable Bot API, file/photo send | Heavy app, makes phone sluggish |
| **ntfy.sh** | No account needed, self-hostable, tiny app, REST | Less familiar, no 2-way |
| **Pushover** | Clean app, priority levels, iOS/Android | Paid after trial |
| **Email (SMTP)** | Universal | Async, may hit spam |
| **Home Assistant webhook** | If HA is running, deeply integrated | Only useful if HA is in play |

**Decision (2026-06-07):** Start with a **local stub** — write the `notify` tool structure but target something dead-simple first (e.g. write to a local file `/var/lib/agentd/notifications.jsonl` that the UI can poll, or just a desktop notification via `notify-send`). Wire Telegram or ntfy.sh as a second step once the plumbing is proven. This keeps 6c unblocked without requiring an account or phone setup.

### MCP tools (regardless of service)

| Tool | Description |
|---|---|
| `notify` | `message: String, title?: String, priority?: String` |
| `notify_file` | `path: String, caption?: String` — attach a file/image |

### Checklist

- [ ] **Decide notification service** (see options above)
- [ ] Write notify tool — either in `apexos-tools` (add tools there) or separate `apex-notify` binary
- [ ] Store credentials/topic in `/etc/agentd/env` (never in source)
- [ ] Policy: `notify` = `yolo` (agent should be able to reach user without asking)
- [ ] Smoke test: ask agent "send me a test notification" — verify receipt on phone
- [ ] Commit: `feat(notify): notification tool via [chosen service]`

---

## Phase 6d — GPIO ✗

**Goal:** Agent touches the physical world. Pi has pins — use them.

### Hardware reality (2026-06-07)

**The ApexOS Pi 5 has a Hailo-8L HAT connected** — the 40-pin header is occupied.
GPIO phase is therefore a stub for future non-Hailo Pi setups, not for this machine.

**SensorHead** — a second Pi (offline, PSU repurposed for the ApexOS Pi) that had
environmental sensors connected. Was exposed as Python MCP tools via the SensorHead
service. Hardware: sensors wired to a Pi GPIO header, served over the network.

**Future direction — dedicated body-pi:**
Rather than Python MCP wrappers (what SensorHead was), the better architecture is a
dedicated "body-pi" where sensors are wired directly and the firmware is Rust:
- Sensor reads via `rppal` crate (pure Rust GPIO/I2C/SPI for Pi)
- MCP server in Rust, speaks over network or stdio to ApexOS
- No Python layer, no SensorHead intermediary
- Sensors become first-class Event sources on the ApexOS bus (Temperature, Humidity, Motion, etc.)
- This would make a clean `SensorEvent` variant in `core/types.rs`

For now: implement GPIO phase as a **Rust stub** using `rppal` that compiles and
runs on a bare Pi without Hailo. Real wiring happens when a body-pi is assembled.

### Rust MCP server (stub)

`tools/crates/apex-gpio/` — Rust binary using `rppal` crate.
Compiles on any Pi; on ApexOS Pi with Hailo HAT, tools return a clear
"GPIO pins occupied by Hailo HAT" message rather than failing silently.
Real activation happens on a body-pi with free header.

### Tools

| Tool | Description |
|---|---|
| `gpio_read` | `pin: int` → `{value: 0\|1}` |
| `gpio_write` | `pin: int, value: 0\|1` |
| `gpio_pulse` | `pin: int, on_ms: int, off_ms?: int, count?: int` |
| `gpio_pwm` | `pin: int, frequency_hz: float, duty_cycle: float` |
| `gpio_list` | — list known pins and their current state |

### Checklist

- [ ] **Decide what's connected** (or plan a first test circuit)
- [ ] Add `rppal = "0.17"` to `tools/crates/apex-gpio/Cargo.toml`
- [ ] Write `apex-gpio` Rust MCP server; detect Hailo HAT presence and return informative stub response
- [ ] Add `agentd` user to `gpio` group: `sudo usermod -a -G gpio agentd`
- [ ] Register in `plugins.toml`
- [ ] Policy: `gpio_read`=`yolo`, `gpio_write`/`gpio_pulse`/`gpio_pwm`=`ask`
- [ ] Smoke test: blink an LED from agent command
- [ ] Commit: `feat(gpio): apex-gpio MCP server — physical world control`

---

## Body-pi architecture notes

When a dedicated sensor Pi is assembled (not the ApexOS Hailo Pi):
- Use `rppal` crate for direct GPIO/I2C/SPI — no Python layer
- Define `SensorEvent` variants in `core/types.rs` (Temperature, Humidity, Motion, etc.)
- Body-pi runs a lightweight Rust daemon that emits these events to the ApexOS bus
  (either via WS to the gateway, or if on same LAN, via a UDP/TCP bridge)
- SensorHead (the old Python approach) was proof-of-concept — this is the clean version
- Hailo HAT stays on ApexOS Pi for inference offload; sensor Pi is a separate node

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
