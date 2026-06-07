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

## Phase B — App ecosystem (next)

**Terminal window**
- Add `xterm.js` + `xterm-addon-fit.js` to `ui/lib/` (~200KB)
- New window type `terminal` with `<div id="terminal-container">`
- WS protocol extension: new `TermInput` / `TermOutput` events on the bus
- OR: wire directly to `run_command` tool — each Enter sends a `UserPrompt` with
  the command, agent runs it in auto-edit mode, streams stdout back
- Simpler WS-native approach: `/terminal-ws` endpoint that opens a PTY via tokio-pty

**Camera window**
- `win-camera-content`: `<img id="camera-snapshot">` + Refresh button
- Calls `run_command` tool: `capture_visual` or `capture_night` → reads file → base64 → img src
- OR: dedicate a `/api/snapshot` GET route that shells out and returns the JPEG directly

**Policy rules GET**
- Add `/api/policy/rules` GET in gateway — reads `PolicyConfig` from `Arc<RwLock<PolicyEngine>>`
  and returns per-tool rules as JSON
- Needs `policy_arc: Arc<RwLock<PolicyEngine>>` added to `GatewayState`
- Unlocks the rules table in Settings → Policy tab

**Wallpaper options**
- Static: current ASCII logo watermark ✓
- Live thermal: WebSocket sensor readings → canvas heatmap render
  - `<canvas id="thermal-canvas">` in `#desktop-wallpaper`
  - `updateThermalWallpaper(frame)` called from `onSensorReading` when `kind === 'thermal_frame'`
  - Map 32×24 pixel values to colour gradient (cool=blue, hot=red, opacity ~15%)

---

## Phase C — Full OS feel (later)

**IDE window**
- Monaco editor (`monaco-editor` CDN or bundle via `ui/lib/monaco/`)
- File browser panel: `list_dir` tool → tree view, click to `read_file` into editor
- Save button → `write_file` tool
- Heavy (~2MB bundle) — worth it for the embedded IDE use case

**Sub-agent window**
- When `agent_spawn` virtual tool fires, bus emits a `SubAgentStarted { session_id }` event
- Desktop: intercepts event, opens a new `agent-{session_id}` window
- Second chat panel reuses the same `output`/`input-row` HTML structure
- `desktop-app.js` creates a fresh DOM clone of `win-agent-content` and mounts it

**Multi-wallpaper + dark/light mode**
- Wallpaper picker in Settings → toggle: ASCII / thermal canvas / dark gradient / light
- CSS custom properties for dark/light theme — toggle via `body.light-mode` class

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
