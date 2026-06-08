<div align="center">

<!-- Replace with generated banner -->
<!-- ![ApexOS Banner](docs/banner.png) -->

# ApexOS

**Agent-first operating system layer for Raspberry Pi 5**

*A single Rust daemon that gives an AI agent eyes, ears, memory, voice, and a body — running entirely at the edge.*

[![Rust](https://img.shields.io/badge/built_with-Rust-orange?style=flat-square)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/platform-Raspberry_Pi_5-red?style=flat-square)](https://www.raspberrypi.com/products/raspberry-pi-5/)
[![Inference](https://img.shields.io/badge/inference-Anthropic_API-blueviolet?style=flat-square)](https://www.anthropic.com/)
[![License](https://img.shields.io/badge/license-MIT-green?style=flat-square)](LICENSE)

</div>

---

## What is this?

Most "AI at the edge" is a cloud service with a thin client bolted onto hardware. ApexOS is the inverse.

The Pi isn't running inference — **it is the agent's body.** A single Rust binary (`agentd`) wires together sensors, memory, voice, tools, and a windowed desktop UI into a coherent whole. The cloud supplies the LLM brain (Anthropic API); everything else lives on the board.

The result is an agent that:
- **Sees** the room via thermal camera and RGB cameras
- **Hears** you via a USB mic and whisper.cpp wake word detection
- **Speaks** back via Piper neural TTS
- **Remembers** everything via a persistent graph memory system (CerebroCortex)
- **Acts** on the world via 100+ MCP tools (shell, filesystem, HTTP, sensors, music generation)
- **Evolves itself** — proposes and applies changes to its own system prompt and policy rules
- **Wakes on voice** — say "apex" and it listens, responds, speaks

---

## Architecture

```
┌─────────────────────────────── Raspberry Pi 5 ───────────────────────────────┐
│                                                                                │
│  ┌──────────────────────────────────────────────────────────────────────┐     │
│  │                        agentd  (single Rust binary)                   │     │
│  │                                                                        │     │
│  │   core ──── broadcast bus ──── gateway (axum WebSocket + HTTP API)   │     │
│  │    │              │                                                    │     │
│  │  agent          store          plugins (MCP-over-stdio)               │     │
│  │  (turn)        (JSONL)        ┌──────────────────────────┐            │     │
│  │    │                          │ CerebroCortex  (memory)  │            │     │
│  │  policy                       │ apexos-tools   (shell/fs) │            │     │
│  │  engine                       │ sensor-head    (sensors)  │            │     │
│  │                               │ sonus          (music)    │            │     │
│  │                               └──────────────────────────┘            │     │
│  └──────────────────────────────────────────────────────────────────────┘     │
│                                                                                │
│  Hardware sensors          Voice I/O              Display                      │
│  BME688  · air quality     whisper.cpp  (STT)     cage kiosk (Wayland)         │
│  MLX90640 · thermal        piper        (TTS)     or any browser               │
│  camera  · rpicam          apex-wake    (wake)                                 │
│                                                                                │
└────────────────────────────────────────────────────────────────────────────────┘
                                      │
                              Anthropic API
                          (inference only — no data stored)
```

One `Event` type flows through the bus, the event log, and the WebSocket to the browser. Everything is a stream.

---

## Features

### Agent core
- **Streaming turns** with thinking-block retention across multi-turn conversations
- **Policy engine** — `suggest` / `auto-edit` / `yolo` modes × per-tool approval rules
- **Sub-agent orchestration** — `agent_spawn` virtual tool; child sessions with fan-out and cascade cancel
- **Self-evolution** — `propose_evolution` tool; agent can live-patch its own soul.md and policy; rollback snapshots
- **Session persistence** — append-only JSONL per session; history replay with full context on reconnect
- **Scheduled tasks** — cron-driven autonomous turns via `schedule_task` virtual tool

### Senses & voice
- **Wake word** — `apex-wake` service: 3s ALSA chunks → whisper.cpp base.en → trigger
- **STT** — whisper.cpp (`/api/record/start` + `/api/record/stop`), sub-second on Pi 5, no PipeWire needed
- **TTS** — Piper neural voice (lessac-medium, 0.13× real-time on Pi 5)
- **Thermal camera** — MLX90640 32×24 live feed, rendered as canvas wallpaper and sensor stream
- **Air quality** — BME688 BSEC2: IAQ, temperature, humidity, pressure with alert routing
- **Camera** — rpicam-jpeg snapshots via `/api/snapshot`

### Desktop UI
- **Windowed OS shell** — WinBox 0.2.82, Alpine.js, no framework build step, no CDN
- **Agent window** — streaming text, collapsible tool calls, inline approval UX, mic + speaker buttons
- **Terminal** — real PTY via libc `openpty`, full interactive bash (vim, htop, colours)
- **Monaco IDE** — 0.55.1 bundled, Ctrl+S save, language auto-detect
- **File explorer** — two-pane lazy tree, upload, open in IDE/Notes
- **Sketchpad** — HTML5 canvas, pen/eraser, PNG download
- **Media player** — Sonus/Suno AI music generation + HTTP 206 range streaming
- **Sub-agent windows** — each child session gets its own WinBox with streaming output and approval buttons
- **Home dashboard** — live CPU temp, RAM, disk, IAQ badge, thermal mini-canvas, agent stats

### Infrastructure
- **MCP plugin system** — CerebroCortex, apexos-tools, sensor-head, sonus; 103 tools at runtime
- **Event log** — append-only JSONL per day, date-rolling, NVMe-backed
- **Hot reload** — live model swap, soul.md update, policy rule change, plugin registration without restart
- **Notifications** — JSONL log + notify-send toast + Piper TTS + ntfy.sh + Telegram (env-gated)

---

## Hardware

Tested on:
- **Raspberry Pi 5** (8GB) — Debian trixie, NVMe SSD boot
- **BME688** — air quality / environment sensor (BSEC2 library)
- **MLX90640** — 32×24 thermal camera
- **USB microphone** — any ALSA-compatible device
- **Camera** — any rpicam-compatible module

The Pi 5 specifically matters: sub-second whisper.cpp transcription, 0.13× real-time Piper synthesis, and thermal at 30s intervals — all concurrent. Pi 4 would struggle.

---

## Quick start

> Detailed deploy notes in `docs/claude/pi-deploy.md`. This is the condensed path.

```bash
# 1. Clone on Pi
git clone https://github.com/buckster123/ApexOS
cd ApexOS

# 2. Build (on Pi — do not cross-compile)
cd agentd && cargo build --release

# 3. Configure
sudo cp agentd/config/plugins.toml agentd/config/policy.toml /etc/agentd/
echo "ANTHROPIC_API_KEY=sk-ant-..." | sudo tee /etc/agentd/env

# 4. Install systemd service
sudo cp deploy/agentd.service /etc/systemd/system/
sudo systemctl enable --now agentd

# 5. Open browser → http://<pi-ip>:8787
```

```bash
# Optional: voice I/O
bash deploy/setup-voice.sh        # installs ffmpeg, whisper.cpp, piper
sudo cp deploy/apex-wake.service /etc/systemd/system/
sudo cp deploy/apex-wake.py /opt/apex-wake/wake.py
sudo systemctl enable --now apex-wake
```

---

## Build roadmap

26 steps, all complete:

| # | Feature |
|---|---------|
| 1 | Core types + `state.apply()` — pure, unit-tested |
| 2 | Broadcast bus + WebSocket echo round-trip |
| 3 | MCP plugin supervisor — CerebroCortex wired (66 tools) |
| 4 | Agent turn engine — streaming, thinking-block retention, semaphore |
| 5 | Policy engine — suggest/auto-edit/yolo × per-tool rules |
| 6 | Sub-agent routing — parent/child sessions, cascade cancel |
| 7 | Pi deploy — agentd running, full WS round-trip smoke-tested |
| 8 | Event log — append-only JSONL, date-rolling |
| 9 | Frontend UI — terminal aesthetic, streaming, tool calls, approval UX |
| 10 | Cage kiosk — seatd, Wayland, agentos-kiosk user |
| 11 | Frontend controls — cancel, power modal, model selector, policy badge |
| 12 | Self-evolution — `propose_evolution`, live soul.md/policy patching |
| 13 | Session persistence — append-only JSONL, history replay, multi-client sync |
| 14 | apexos-tools — 11 MCP tools: shell, fs, HTTP, sysstat |
| 15 | Scheduled tasks — cron-driven autonomous turns, JSONL persistence |
| 16 | Notifications — TTS + toast + ntfy.sh + Telegram |
| 17 | Sensor bridge — BME688 air quality + MLX90640 thermal, bus events |
| 18 | Desktop skin Phase A — WinBox windowed OS shell |
| 19 | Desktop skin Phase B — start menu, taskbar, terminal, camera, thermal wallpaper |
| 20 | Desktop skin Phase C/D — notes, browser, sketchpad, explorer, Monaco IDE |
| 21 | Sonus music player — AI music generation + HTTP range streaming |
| 22 | Real PTY terminal — libc `openpty`, resize, full interactive shell |
| 23 | Sub-agent windows v2 — tools/results/approval in child WinBox |
| 24 | Home dashboard — live system, environment, agent stats |
| 25 | Voice I/O — whisper.cpp STT + Piper TTS, server-side ALSA recording |
| 26 | Wake word — `apex-wake` service, `Ctrl+Space` manual trigger, auto voice turn |

---

## Philosophy

Current AI deployment is top-down: a general-purpose model in a data centre, thin clients everywhere else. ApexOS is bottom-up. The hardware is the agent's body — not a display terminal, not a retrieval node, a *body*. It has proprioception (sensors), voice, memory, and the ability to rewrite its own behaviour. The cloud supplies cognition; the Pi supplies presence.

This is what embedded AI looks like when you don't treat the hardware as an afterthought.

---

## License

MIT
