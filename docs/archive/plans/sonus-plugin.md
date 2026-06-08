# Sonus Plugin — Hermes Sonus MCP port for ApexOS

## What it is

`hermes-sonus` is a Python MCP server (FastMCP / stdio) for Suno AI music generation,
MIDI composition, and (optionally) OpenBCI EEG felt-experience feedback. Built for the
Nous Research Hermes agent; the MCP server layer is transport-agnostic and drops straight
into ApexOS's plugin supervisor with zero Rust changes.

Source: `https://github.com/buckster123/hermes-sonus`
Pattern: identical to CerebroCortex — Python venv, stdio MCP, registered in `plugins.toml`

---

## Tools exposed (~10 Suno tools)

| Tool | Purpose |
|------|---------|
| `generate_song` | Fire a Suno generation request (prompt, model, tags, BPM, etc.) |
| `check_status` | Poll task status by task_id |
| `download_track` | Download completed audio to local path |
| `extend_track` | Extend an existing generated track |
| `generate_lyrics` | Generate lyrics only (no audio) |
| `clone_voice_validate` | Generate a voice clone verification phrase |
| `clone_voice_create` | Create a voice clone from a recording |
| `generate_album` | Batch generate from YAML/JSON manifest |
| `check_credits` | Check remaining Suno API credits |
| `list_endpoints` | List all available Suno API endpoints |

EEG tools (OpenBCI) — defer until hardware available.

---

## Environment

```
SUNO_API_KEY        — sunoapi.org API key (add to /etc/agentd/env)
SUNO_BASE_URL       — default: https://api.sunoapi.org
SUNO_DOWNLOAD_DIR   — default: /var/lib/agentd/workspace/sonus/
```

---

## Deploy plan (Pi)

```bash
# 1. Clone repo
cd /opt
sudo git clone https://github.com/buckster123/hermes-sonus sonus
sudo chown -R agentd:agentd /opt/sonus

# 2. Create venv (reuse cerebro venv pattern)
sudo -u agentd python3 -m venv /opt/sonus-venv
sudo -u agentd /opt/sonus-venv/bin/pip install -e "/opt/sonus[mcp]"

# 3. Wrapper script (same pattern as /usr/local/bin/cerebro-mcp)
sudo tee /usr/local/bin/sonus-mcp > /dev/null << 'EOF'
#!/bin/bash
source /opt/sonus-venv/bin/activate
exec python -m hermes_sonus.mcp.server "$@"
EOF
sudo chmod +x /usr/local/bin/sonus-mcp

# 4. Register in plugins.toml (see below)
# 5. Add SUNO_API_KEY to /etc/agentd/env
# 6. Restart agentd
```

## plugins.toml entry

```toml
[[plugin]]
id       = "sonus"
command  = "/usr/local/bin/sonus-mcp"
env      = { SUNO_DOWNLOAD_DIR = "/var/lib/agentd/workspace/sonus" }
```

---

## Desktop media player window (Phase E)

**Files:**
- `win-player-content` div in `desktop.html`
- `openWin('player')` entry in `WIN_DEFAULTS` in `desktop-app.js`
- `player` dock button
- Gateway route: `GET /api/sonus/files` → reads `/var/lib/agentd/workspace/sonus/*.mp3`

**UI layout:**
```
┌─────────────────────────────────────────────┐
│  🎵 Sonus Player                         ✕  │
├─────────────────────────────────────────────┤
│  Track list                                  │
│  > track_abc123.mp3          02:34  ▶        │
│    track_def456.mp3          03:12  ▶        │
├─────────────────────────────────────────────┤
│  ████████████░░░░░░░░  01:24 / 02:34         │
│  [◀◀] [▶ PLAY] [▶▶]  🔊 ────────           │
└─────────────────────────────────────────────┘
```

**Agent DJ loop:**
1. User says "play me something chill" or agent decides based on context
2. Agent calls `generate_song` with prompt → gets `task_id`
3. Agent polls `check_status` until terminal state
4. Agent calls `download_track` → file saved to `SUNO_DOWNLOAD_DIR`
5. Desktop player receives a `SonusTrackReady` event on the bus (or polls `/api/sonus/files`)
6. Track auto-appears in player; agent can queue next

**Bus event (future):** `Event::SonusTrackReady { path, title, duration_secs }` — avoids
polling; gateway broadcasts to all WS clients when a new track lands.

---

## MIDI (optional, Phase E+)

`hermes-sonus` ships MIDI composition tools. On Pi these would write `.mid` files.
Playback would need `timidity` or `fluidsynth`. Low priority — tackle after Suno works.

---

## Audio output on Pi

Pi 5 has no 3.5mm headphone jack by default. Options:
- USB audio adapter (cheapest, plug-and-play)
- HDMI audio (works if display is connected)
- Bluetooth speaker

`<audio>` element in the browser plays through whatever the Pi's audio sink is configured.
`espeak-ng` (already installed for notify tool) uses the same sink — test that first.
agentd is already in the `audio` group.

---

## Deferred

- OpenBCI EEG layer — requires hardware
- Stem separation (`spleeter`) — heavy, optional
- Music video generation — Suno premium feature
- Voice cloning — requires recording workflow
