# Desktop Skin — WinBox OS Shell

## Architecture

Single HTML page (`desktop.html`) served by agentd at `/desktop.html`.
Shares all WebSocket machinery with the CLI skin (`app.js`). Desktop-specific
logic lives in `desktop-app.js` (loaded after `app.js`, overrides
`transitionToApp()`). Libraries bundled locally in `ui/lib/` — no CDN at runtime.

```
ui/
  desktop.html        — shell: topbar, dock, win-content divs (hidden until WinBox mounts)
  desktop-app.js      — window manager (openWin/closeWin/toggleWin), settingsApp() Alpine
  desktop-style.css   — OS shell styles (.wb-apexos WinBox theme, topbar, dock, windows)
  app.js              — shared WS engine (sensor readings, turn engine, approval UX)
  lib/
    winbox.min.js     — WinBox 0.2.82 (~10KB)
    winbox.min.css    — WinBox base styles
    alpine.min.js     — Alpine.js 3.x (~45KB)
```

**Key rules:**
- All element IDs referenced by `app.js` MUST exist in `desktop.html`
  (use hidden spans for ones not visually needed on desktop)
- `window.pluginCounts` must be on `window` (not `const`) so desktop-app.js can read it
- Iframe `src` must be absent or empty string — use `getAttribute('src')` not `iframe.src`
  to check if loaded (the IDL property resolves empty string to current page URL)
- `~` in sudo bash -c expands to root's home — always use absolute paths (`/home/apexos/...`)

## Window types

| Window ID    | Content               | Status    | Notes                                |
|------------- | --------------------- | --------- | ------------------------------------ |
| `agent`      | Chat panel (app.js)   | ✓ Phase A | Auto-opens at boot, non-closeable TBD|
| `sensors`    | Live sensor widget    | ✓ Phase A | Auto-opens at boot                   |
| `cerebro`    | iframe :8767/ui       | ✓ Phase A | Lazy src load on open                |
| `sensorhead` | iframe :8080          | ✓ Phase A | Lazy src load on open                |
| `settings`   | Alpine form           | ✓ Phase A | Soul editor, policy mode, plugin list|
| `terminal`   | xterm.js + run_command| Phase B   | Real shell in a window               |
| `camera`     | Snapshot on demand    | Phase B   | capture_visual / capture_night       |
| `ide`        | Monaco editor         | Phase C   | File browser + read/write via tools  |
| `sub-agent`  | Second chat panel     | Phase C   | Spawned by agent_spawn virtual tool  |

## API routes added (gateway)

| Route              | Method | Purpose                              |
| ------------------ | ------ | ------------------------------------ |
| `/api/soul`        | GET    | Read soul.md content                 |
| `/api/soul`        | POST   | Write soul.md content                |
| `/api/policy`      | POST   | Live-switch policy mode (suggest / auto-edit / yolo) |
| `/api/policy/rules`| GET    | Per-tool rules table (**Phase B**)   |

Policy mode stored as `Arc<RwLock<String>>` in GatewayState, mutated via mpsc channel
so gateway stays decoupled from the policy crate.

---

## Phase A — Windowed shell ✓ DONE

- [x] WinBox + Alpine bundled locally (`ui/lib/`)
- [x] `desktop.html`: topbar (clock, model, policy selectors, WS status, CLI/power btns)
- [x] `desktop.html`: dock (Agent, Sensors, Cerebro, SensorHead, Settings)
- [x] `desktop.html`: win-content divs for all windows (hidden, WinBox mounts them)
- [x] `desktop-app.js`: `openWin` / `closeWin` / `toggleWin` / `dockMark`
- [x] `desktop-app.js`: auto-opens Agent + Sensors at boot
- [x] `desktop-app.js`: lazy iframe load for Cerebro (:8767) and SensorHead (:8080)
- [x] `desktop-app.js`: `settingsApp()` — soul editor, policy mode switcher, plugin list
- [x] `desktop-style.css`: full OS shell stylesheet, `.wb-apexos` WinBox theme
- [x] `app.js`: `window.pluginCounts` (was `const`, not accessible cross-script)
- [x] `app.js`: `switchSkin()` for CLI↔Desktop toggle button
- [x] `ui/app.js` + `ui/index.html`: policy live selector (was read-only badge)
- [x] `agentd/deploy/cerebro-api.service`: CerebroCortex dashboard at boot port 8767
- [x] Gateway: `/api/soul` GET+POST, `/api/policy` POST, `lib/` static file serving

**Known issues fixed:**
- `hdr-logo` / `hdr-sessions` missing from desktop.html → null.addEventListener crash, boot never started
- `iframe.src` vs `iframe.getAttribute('src')` — empty src="" resolves to page URL, Cerebro/SensorHead iframes never loaded
- `const pluginCounts` → `window.pluginCounts` — const is script-scoped, not visible cross-script
- `let bootDone` → `var bootDone` — let is script-scoped, desktop-app.js couldn't set it, enableInput never called, chat input stayed disabled
- Removed `Object.defineProperty(window,'bootDone',...)` hack — was correctly intercepting the let variable, now unnecessary with var
- Win-content divs started `display:block` in body (outside #app) → inflated body height → flex column miscalculated, dock appeared to fill half screen. Fix: `display:none` until openWin mounts them
- `#desktop-canvas { min-height: 0 }` — flex:1 child needs explicit min-height:0 to shrink below content size in column layout
- `fonts-noto-color-emoji` installed on Pi — dock emoji icons invisible in cage kiosk without it
- Logo dblclick → sessions modal (server-side) not history modal (localStorage apexos_history never written)

---

## Phase B — System tools (next)

**CLI `!` passthrough + slash commands**
- `!cmd` prefix in CLI input → intercepted client-side → `/api/run` POST → streams stdout back into chat output
- Slash commands: `/help`, `/status`, `/sessions` etc. — pure frontend dispatch
- Same `/api/run` endpoint powers the desktop terminal window
- Gateway route: `POST /api/run` → calls `run_command` tool directly (bypasses agent turn engine), streams response as SSE

**Terminal window (desktop)**
- Add `xterm.js` + `xterm-addon-fit.js` to `ui/lib/` (~200KB total)
- New `terminal` entry in WIN_DEFAULTS + `win-terminal-content` div with `<div id="terminal-xterm">`
- Option A (simple): wire to `/api/run` — each Enter sends command, stdout streams back. No PTY, no interactive programs.
- Option B (full): `/terminal-ws` WebSocket endpoint + `tokio-pty` crate → real PTY, interactive programs (vim, top, etc.)
- Start with Option A; upgrade to B if needed.

**Dock redesign — start menu + minimize-to-taskbar**
- Remove sticky app icons from dock; replace with a single **⬡ Start** button (bottom-left)
- Start button opens a drop-up grid of all available apps (agent, sensors, cerebro, sensorhead, settings, terminal, camera, etc.)
- **Taskbar**: running/minimized apps appear as labeled tabs at the bottom. Tab disappears when app is fully closed.
- WinBox `.minimize()` already works — wire `onminimize` to add a taskbar tab, `onrestore`/`onclose` to remove it
- CSS: tabs in `#dock` are dynamically created `<button class="taskbar-tab">` elements

**Camera window**
- `win-camera-content`: `<img id="camera-snapshot">` + Refresh button
- Gateway `GET /api/snapshot` → shells `capture_visual` or `capture_night` → returns JPEG bytes directly

**`/api/policy/rules` GET endpoint**
- Add `policy_arc: Arc<RwLock<PolicyEngine>>` to `GatewayState`
- Return per-tool rules as JSON → unlocks the rules table in Settings → Policy tab

**Thermal canvas wallpaper**
- `<canvas id="thermal-canvas">` in `#desktop-wallpaper`
- `updateThermalWallpaper(frame)` called from sensor_reading events when `kind === 'thermal_frame'`
- 32×24 pixel grid → colour gradient (cool=blue, hot=red, opacity ~15%)

---

## Phase C — Full OS feel

**IDE window**
- Monaco editor (bundle via `ui/lib/monaco/`, ~2MB)
- File browser panel: `list_dir` tool → tree view, click to `read_file` into editor
- Save button → `write_file` tool

**Sub-agent window**
- `agent_spawn` virtual tool → bus emits `SubAgentStarted { session_id }`
- Desktop intercepts event, opens a new `agent-{session_id}` WinBox window
- DOM clone of `win-agent-content` structure, fresh output div

**Multi-wallpaper + dark/light mode**
- Wallpaper picker in Settings → toggle: ASCII / thermal canvas / dark gradient / light
- CSS custom properties for dark/light theme — toggle via `body.light-mode` class

---

## Phase D — App ecosystem

**File explorer window**
- `win-explorer-content`: sidebar tree (via `list_dir` tool) + main pane (file preview via `read_file`)
- Click directory → expand subtree. Click file → open in preview pane (or send to IDE window).
- Toolbar: New File, New Dir, Delete (all via apexos-tools MCP)

**Notes / Notepad window**
- Simple textarea, saves to a user-chosen path via `write_file` tool
- Auto-saves on blur; title bar shows filename

**Sketchpad window**
- HTML5 canvas with basic draw tools (pen, eraser, colour, stroke width)
- Save button → canvas.toDataURL() → `write_file` as PNG to `/var/lib/agentd/workspace/sketches/`
- Agent can then `read_file` the PNG path to see the sketch (describe_image via Cerebro vision or agent reads path)

**Browser / Webview window**
- iframe + URL bar input at top
- Useful for internal dashboards (Cerebro, SensorHead, any local service)
- Cross-origin limits apply; mainly a quality-of-life launcher for known local ports

---

## Phase E — Sonus / Media

See `plans/sonus-plugin.md` for full detail.

**Hermes Sonus MCP port** (Python, low effort — already a FastMCP server)
- Clone `buckster123/hermes-sonus` on Pi → venv → `pip install "hermes-sonus[mcp]"`
- Entry point: `hermes-sonus-mcp` (stdio transport, same pattern as CerebroCortex)
- Register in `agentd/config/plugins.toml`; set `SUNO_API_KEY` in `/etc/agentd/env`
- ~10 tools: `generate_song`, `check_status`, `download_track`, `extend_track`,
  `generate_lyrics`, `clone_voice_*`, `check_credits`, `generate_album`
- EEG layer (OpenBCI) deferred — skip for now

**Media player window**
- `win-player-content`: track list + waveform/progress bar + play/pause/skip controls
- Agent calls `generate_song` → polls `check_status` → `download_track` to `/var/lib/agentd/workspace/sonus/`
- Gateway serves `/api/sonus/files` → returns file list; player fetches and plays via HTML `<audio>`
- Agent DJ: agent can queue next track based on context, sensor data, or user prompt

---

## Deploy reference

```bash
# Standard cycle (always git-first):
git add ... && git commit -m "..." && git push

# On Pi:
cd ~/ApexOS && git pull
cd agentd && ~/.cargo/bin/cargo build --release

# Deploy binary:
sudo systemctl stop agentd
sudo cp /home/apexos/ApexOS/agentd/target/release/agentd /usr/local/bin/agentd
sudo systemctl start agentd

# Deploy UI only (no binary change):
sudo cp /home/apexos/ApexOS/ui/*.js   /var/lib/agentd/ui/
sudo cp /home/apexos/ApexOS/ui/*.css  /var/lib/agentd/ui/
sudo cp /home/apexos/ApexOS/ui/*.html /var/lib/agentd/ui/
sudo cp /home/apexos/ApexOS/ui/lib/*  /var/lib/agentd/ui/lib/
sudo chown -R agentd:agentd /var/lib/agentd/ui/
```

Note: `~` inside `sudo bash -c "..."` expands to root's home. Always use
`/home/apexos/...` absolute paths.
