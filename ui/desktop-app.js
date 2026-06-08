// ─── ApexOS Desktop App — WinBox window manager + Alpine data ────────────────
// Loaded after app.js. Overrides transitionToApp() and sets up the windowed OS.

// ─── Clock ────────────────────────────────────────────────────────────────────
function startClock() {
  const el = document.getElementById('topbar-clock');
  if (!el) return;
  const tick = () => {
    const d = new Date();
    el.textContent =
      String(d.getHours()).padStart(2,'0') + ':' +
      String(d.getMinutes()).padStart(2,'0') + ':' +
      String(d.getSeconds()).padStart(2,'0');
  };
  tick();
  setInterval(tick, 1000);
}

// ─── Wallpaper logo ───────────────────────────────────────────────────────────
const WALLPAPER_LOGO = [
  ' █████╗ ██████╗ ███████╗██╗  ██╗ ██████╗ ███████╗',
  '██╔══██╗██╔══██╗██╔════╝╚██╗██╔╝██╔═══██╗██╔════╝',
  '███████║██████╔╝█████╗   ╚███╔╝ ██║   ██║███████╗',
  '██╔══██║██╔═══╝ ██╔══╝   ██╔██╗ ██║   ██║╚════██║',
  '██║  ██║██║     ███████╗██╔╝ ██╗╚██████╔╝███████║',
  '╚═╝  ╚═╝╚═╝     ╚══════╝╚═╝  ╚═╝ ╚═════╝ ╚══════╝',
].join('\n');

// ─── Thermal canvas wallpaper ─────────────────────────────────────────────────
// ThermalFrame events carry only min_c/mean_c/max_c (no pixel array — core keeps
// events small). We render a 32×24 grid with per-cell noise in the min/max range
// to give the thermal-camera vibe, with colour varying from blue (cold) → red (hot).

function thermalColor(c, alpha) {
  const t = Math.max(0, Math.min(1, (c - 15) / 35));  // 15°C=0, 50°C=1
  let r, g, b;
  if (t < 0.5) {
    const u = t * 2;           // 0..1 over cold→warm
    r = Math.round(10  + u * 40);
    g = Math.round(30  + u * 80);
    b = Math.round(160 - u * 60);
  } else {
    const u = (t - 0.5) * 2;  // 0..1 over warm→hot
    r = Math.round(50  + u * 205);
    g = Math.round(110 - u * 80);
    b = Math.round(100 - u * 90);
  }
  return `rgba(${r},${g},${b},${alpha})`;
}

window.updateThermalWallpaper = function(frame) {
  const canvas = document.getElementById('thermal-canvas');
  if (!canvas) return;
  const W = 32, H = 24;
  const dpr = window.devicePixelRatio || 1;
  const cw = canvas.parentElement.clientWidth || 800;
  const ch = canvas.parentElement.clientHeight || 600;
  canvas.width  = cw * dpr;
  canvas.height = ch * dpr;
  canvas.style.width  = cw + 'px';
  canvas.style.height = ch + 'px';
  const ctx = canvas.getContext('2d');
  ctx.clearRect(0, 0, canvas.width, canvas.height);
  const cw2 = canvas.width, ch2 = canvas.height;
  const cellW = cw2 / W, cellH = ch2 / H;
  const range = Math.max(0.5, (frame.max_c || 30) - (frame.min_c || 25));
  for (let y = 0; y < H; y++) {
    for (let x = 0; x < W; x++) {
      const noise = (Math.random() - 0.5) * range;
      const c = (frame.mean_c || 25) + noise;
      ctx.fillStyle = thermalColor(c, 0.13);
      ctx.fillRect(x * cellW, y * cellH, cellW + 1, cellH + 1);
    }
  }
};

// ─── Window registry + config ─────────────────────────────────────────────────
const wins = {};

const WIN_DEFAULTS = {
  agent: {
    title: '🤖 Agent',
    x: 40, y: 56, width: 700, height: 520,
    background: 'var(--wb-bg)',
  },
  sensors: {
    title: '📡 Sensors',
    x: 760, y: 56, width: 300, height: 420,
    background: 'var(--wb-bg)',
  },
  cerebro: {
    title: '🧠 Cerebro',
    x: 80, y: 56, width: 960, height: 600,
    background: '#0d0f18',
  },
  sensorhead: {
    title: '👁 Sensor Head',
    x: 80, y: 56, width: 960, height: 600,
    background: '#fff',
  },
  settings: {
    title: '⚙ Settings',
    x: 200, y: 80, width: 520, height: 480,
    background: 'var(--wb-bg)',
  },
  terminal: {
    title: '💻 Terminal',
    x: 120, y: 80, width: 720, height: 460,
    background: '#0d0f18',
  },
  camera: {
    title: '📷 Camera',
    x: 180, y: 60, width: 680, height: 520,
    background: '#0d0f18',
  },
  sketchpad: {
    title: '🎨 Sketchpad',
    x: 140, y: 60, width: 720, height: 540,
    background: '#0d0f18',
  },
  notes: {
    title: '📝 Notes',
    x: 160, y: 70, width: 580, height: 480,
    background: 'var(--wb-bg)',
  },
  browser: {
    title: '🌐 Browser',
    x: 100, y: 50, width: 900, height: 620,
    background: '#fff',
  },
  explorer: {
    title: '📁 Explorer',
    x: 160, y: 60, width: 780, height: 520,
    background: 'var(--wb-bg)',
  },
  ide: {
    title: '🖥 IDE',
    x: 60, y: 50, width: 900, height: 620,
    background: '#1e1e1e',
  },
};

// ─── Taskbar tab management ───────────────────────────────────────────────────
function createTaskbarTab(id, title) {
  if (document.getElementById(`tab-${id}`)) return;
  const tab = document.createElement('button');
  tab.className = 'taskbar-tab';
  tab.id = `tab-${id}`;
  tab.textContent = title;
  tab.onclick = () => toggleWin(id);
  document.getElementById('taskbar-tabs').appendChild(tab);
}

function removeTaskbarTab(id) {
  document.getElementById(`tab-${id}`)?.remove();
}

function updateTab(id, state) {
  const tab = document.getElementById(`tab-${id}`);
  if (!tab) return;
  tab.classList.remove('tab-active', 'tab-minimized');
  if (state === 'active')    tab.classList.add('tab-active');
  if (state === 'minimized') tab.classList.add('tab-minimized');
}

// ─── Start menu ───────────────────────────────────────────────────────────────
function toggleStartMenu() {
  document.getElementById('start-menu').classList.toggle('hidden');
}

function closeStartMenu() {
  document.getElementById('start-menu').classList.add('hidden');
}

function launchApp(id) {
  closeStartMenu();
  toggleWin(id);
}

function openWin(id) {
  if (wins[id]) { wins[id].focus(); return; }

  const content = document.getElementById(`win-${id}-content`);
  if (!content) return;

  // Lazy-load iframes only when window opens
  if (id === 'cerebro') {
    const iframe = document.getElementById('cerebro-iframe');
    if (iframe && !iframe.getAttribute('src')) iframe.src = `http://${location.hostname}:8767/ui`;
  }
  if (id === 'sensorhead') {
    const iframe = document.getElementById('sensorhead-iframe');
    if (iframe && !iframe.getAttribute('src')) iframe.src = `http://${location.hostname}:8080`;
  }

  content.style.display = '';

  if (id === 'terminal') setTimeout(initTerminal, 60);
  if (id === 'notes')     setTimeout(notesInit, 30);
  if (id === 'sketchpad') setTimeout(sketchInit, 30);
  if (id === 'explorer')  setTimeout(explorerInit, 30);
  if (id === 'ide')       setTimeout(ideInit, 60);

  const cfg = WIN_DEFAULTS[id] || { title: id, x: 100, y: 80, width: 600, height: 400 };
  wins[id] = new WinBox(cfg.title, {
    ...cfg,
    class: `wb-apexos wb-${id}`,
    mount: content,
    onclose() {
      content.style.display = 'none';
      delete wins[id];
      removeTaskbarTab(id);
      return false;
    },
    onfocus()    { updateTab(id, 'active'); },
    onblur()     { updateTab(id, 'open'); },
    onminimize() { updateTab(id, 'minimized'); },
    onrestore()  { updateTab(id, 'active'); },
  });

  createTaskbarTab(id, cfg.title);
  updateTab(id, 'active');
}

function closeWin(id) {
  if (wins[id]) { wins[id].close(); }
}

function toggleWin(id) {
  if (!wins[id]) { openWin(id); return; }
  if (wins[id].min) { wins[id].restore(); wins[id].focus(); }
  else              { wins[id].minimize(); }
}

// ─── Override transitionToApp for the desktop skin ───────────────────────────
// app.js defines transitionToApp() and `var bootDone` (window.bootDone).
// We replace transitionToApp here so the boot sequence goes to the OS shell.
window.transitionToApp = async function() {
  if (window.bootDone) return;
  window.bootDone = true;   // sets app.js's var bootDone via window property

  const lines = document.getElementById('boot-lines');
  await window._sleep(80);
  if (typeof addBootLine === 'function') {
    addBootLine(lines, 'ALL SYSTEMS', 'NOMINAL', 'ok');
  }
  await window._sleep(700);

  document.getElementById('boot').classList.add('hidden');
  document.getElementById('app').classList.remove('hidden');

  const wl = document.getElementById('wallpaper-logo');
  if (wl) wl.textContent = WALLPAPER_LOGO;

  startClock();
  applyWallpaper(localStorage.getItem('apexos_wallpaper') || 'thermal');

  // Enable the chat input (app.js's enableInput is a function declaration → window)
  if (typeof enableInput === 'function' && ws?.readyState === WebSocket.OPEN) {
    enableInput(true);
  }

  // Auto-open Agent + Sensors windows after a short settle
  setTimeout(() => openWin('agent'),   100);
  setTimeout(() => openWin('sensors'), 300);
};

// Make sleep available for the override above (app.js defines it in local scope)
window._sleep = ms => new Promise(r => setTimeout(r, ms));

// ─── Sensor widget: keep sidebar values in sync ───────────────────────────────
// The updateSensorWidget() in app.js reads element IDs — they exist in win-sensors-content.
// No extra work needed; the WS sensor_reading handler in app.js drives them directly.

// Periodically refresh the tool count and WS label in the sensor window
setInterval(() => {
  const tools = typeof pluginCounts !== 'undefined'
    ? Object.values(pluginCounts).reduce((a,b) => a+b, 0)
    : 0;
  const el = document.getElementById('s-tools');
  if (el) el.textContent = tools || '—';
}, 3000);

// ─── Terminal window ──────────────────────────────────────────────────────────
let term     = null;
let termFit  = null;
let termBuf  = '';
let termCwd  = '';

function initTerminal() {
  if (term) {
    // Already open — just refit in case window was resized
    if (termFit) setTimeout(() => termFit.fit(), 30);
    return;
  }
  const container = document.getElementById('terminal-xterm');
  if (!container || typeof window.Terminal === 'undefined') return;

  term = new window.Terminal({
    theme: {
      background: '#0d0f18', foreground: '#c8cdd8', cursor: '#39ff14',
      cursorAccent: '#0d0f18', selectionBackground: 'rgba(108,138,255,0.3)',
      black: '#131620',   red: '#ff6b6b',   green: '#39ff14',  yellow: '#f0b429',
      blue: '#6c8aff',    magenta: '#ff79c6', cyan: '#8be9fd', white: '#c8cdd8',
      brightBlack: '#606680', brightGreen: '#5fffad',
    },
    fontFamily: "'JetBrains Mono', 'Fira Code', 'Cascadia Code', Consolas, monospace",
    fontSize: 13, lineHeight: 1.4, cursorBlink: true, allowTransparency: true,
  });
  termFit = new window.FitAddon.FitAddon();
  term.loadAddon(termFit);
  term.open(container);
  setTimeout(() => termFit.fit(), 30);

  term.writeln('\x1b[32m▸ ApexOS Terminal\x1b[0m  · Ctrl+L clear');
  term.writeln('');
  termPrompt();

  term.onKey(({ key, domEvent }) => {
    const k = domEvent.keyCode;
    if (k === 13) {
      term.writeln('');
      const cmd = termBuf.trim();
      termBuf = '';
      if (cmd) termRun(cmd); else termPrompt();
    } else if (k === 8) {
      if (termBuf.length > 0) { termBuf = termBuf.slice(0, -1); term.write('\b \b'); }
    } else if (domEvent.ctrlKey && domEvent.key === 'l') {
      term.clear(); termPrompt();
    } else if (!domEvent.ctrlKey && !domEvent.altKey && key.length === 1) {
      termBuf += key; term.write(key);
    }
  });
}

function termPrompt() {
  term.write(`\x1b[32m${termCwd || '~'}\x1b[0m \x1b[36m$\x1b[0m `);
}

async function termRun(cmd) {
  if (cmd === 'clear' || cmd === 'cls') { term.clear(); termPrompt(); return; }

  // Track cwd for `cd` commands
  if (cmd.startsWith('cd ') || cmd === 'cd') {
    const dir = cmd.length > 3 ? cmd.slice(3).trim() : '~';
    const res = await termExec(`cd ${dir} 2>&1 && pwd`);
    if (res.ok && res.stdout.trim()) termCwd = res.stdout.trim();
    else if (res.stderr) term.writeln(`\x1b[31m${res.stderr.trimEnd()}\x1b[0m`);
    termPrompt(); return;
  }

  const full = termCwd ? `cd ${termCwd} && ${cmd}` : cmd;
  const res  = await termExec(full);
  if (res.ok) {
    if (res.stdout) term.write(res.stdout.replace(/\r?\n/g, '\r\n'));
    if (res.stderr) term.write('\x1b[31m' + res.stderr.replace(/\r?\n/g, '\r\n') + '\x1b[0m');
    if (!res.stdout && !res.stderr && res.exit_code !== 0)
      term.writeln(`\x1b[31mexit ${res.exit_code}\x1b[0m`);
  } else {
    term.writeln(`\x1b[31merror: ${res.error}\x1b[0m`);
  }
  termPrompt();
}

async function termExec(cmd) {
  try {
    const r = await fetch('/api/run', {
      method: 'POST', headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ command: cmd }),
    });
    return await r.json();
  } catch (e) { return { ok: false, error: String(e) }; }
}

// ─── Sketchpad ────────────────────────────────────────────────────────────────
let sketchCtx = null, sketchDrawing = false, sketchMode = 'pen';

function sketchInit() {
  const canvas = document.getElementById('sketch-canvas');
  if (!canvas || sketchCtx) return;
  const resize = () => {
    const img = sketchCtx ? sketchCtx.getImageData(0, 0, canvas.width, canvas.height) : null;
    canvas.width  = canvas.parentElement.clientWidth;
    canvas.height = canvas.parentElement.clientHeight;
    sketchCtx = canvas.getContext('2d');
    sketchCtx.fillStyle = '#0d0f18';
    sketchCtx.fillRect(0, 0, canvas.width, canvas.height);
    if (img) sketchCtx.putImageData(img, 0, 0);
    sketchCtx.lineJoin = 'round'; sketchCtx.lineCap = 'round';
  };
  resize();

  const pos = (e) => {
    const r = canvas.getBoundingClientRect();
    const t = e.touches?.[0] ?? e;
    return [t.clientX - r.left, t.clientY - r.top];
  };
  const down = (e) => {
    sketchDrawing = true;
    const [x, y] = pos(e);
    sketchCtx.beginPath(); sketchCtx.moveTo(x, y);
    e.preventDefault();
  };
  const move = (e) => {
    if (!sketchDrawing) return;
    const size  = +document.getElementById('sketch-size').value;
    const color = document.getElementById('sketch-color').value;
    sketchCtx.lineWidth   = sketchMode === 'erase' ? size * 6 : size;
    sketchCtx.strokeStyle = sketchMode === 'erase' ? '#0d0f18' : color;
    const [x, y] = pos(e);
    sketchCtx.lineTo(x, y); sketchCtx.stroke();
    sketchCtx.beginPath(); sketchCtx.moveTo(x, y);
    e.preventDefault();
  };
  const up = () => { sketchDrawing = false; };

  canvas.addEventListener('mousedown', down);
  canvas.addEventListener('mousemove', move);
  canvas.addEventListener('mouseup',   up);
  canvas.addEventListener('touchstart', down, { passive: false });
  canvas.addEventListener('touchmove',  move, { passive: false });
  canvas.addEventListener('touchend',   up);
}

function sketchTool(mode) {
  sketchMode = mode;
  document.getElementById('sketch-pen-btn')  ?.classList.toggle('active', mode === 'pen');
  document.getElementById('sketch-erase-btn')?.classList.toggle('active', mode === 'erase');
}

function sketchClear() {
  if (!sketchCtx) return;
  const c = document.getElementById('sketch-canvas');
  sketchCtx.fillStyle = '#0d0f18';
  sketchCtx.fillRect(0, 0, c.width, c.height);
}

function sketchDownload() {
  const c = document.getElementById('sketch-canvas');
  if (!c) return;
  const a = document.createElement('a');
  a.download = `sketch-${Date.now()}.png`;
  a.href = c.toDataURL('image/png');
  a.click();
}

// ─── Wallpaper ────────────────────────────────────────────────────────────────
function applyWallpaper(mode) {
  const canvas = document.getElementById('thermal-canvas');
  const logo   = document.getElementById('wallpaper-logo');
  if (mode === 'thermal') {
    if (canvas) canvas.style.display = '';
    if (logo)   logo.style.opacity   = '1';
  } else if (mode === 'logo') {
    if (canvas) canvas.style.display = 'none';
    if (logo) { logo.style.opacity = '1'; logo.style.color = 'rgba(57,255,20,0.12)'; }
  } else {  // minimal
    if (canvas) canvas.style.display = 'none';
    if (logo)   logo.style.opacity   = '0';
  }
}

// ─── Notes window ─────────────────────────────────────────────────────────────
function notesInit() {
  const ed = document.getElementById('notes-editor');
  if (!ed) return;
  const key = 'apexos_note_' + (document.getElementById('notes-filename')?.value || 'scratch.md');
  ed.value = localStorage.getItem(key) || '';
  ed.oninput = () => {
    localStorage.setItem('apexos_note_' + (document.getElementById('notes-filename')?.value || 'scratch.md'), ed.value);
    const st = document.getElementById('notes-status');
    if (st) { st.textContent = 'saved'; clearTimeout(st._t); st._t = setTimeout(() => st.textContent = '', 1500); }
  };
}

async function notesSave() {
  const ed  = document.getElementById('notes-editor');
  const fn  = document.getElementById('notes-filename')?.value?.trim() || 'scratch.md';
  const st  = document.getElementById('notes-status');
  if (!ed) return;
  localStorage.setItem('apexos_note_' + fn, ed.value);
  // Also persist to server workspace
  const path = `/var/lib/agentd/workspace/${fn}`;
  try {
    const r = await fetch('/api/run', {
      method: 'POST', headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ command: `mkdir -p /var/lib/agentd/workspace && cat > ${path}` }),
    });
    // /api/run doesn't support stdin — use tee via echo workaround
    const escaped = ed.value.replace(/'/g, "'\\''");
    await fetch('/api/run', {
      method: 'POST', headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ command: `printf '%s' '${escaped}' > ${path}` }),
    });
    if (st) { st.textContent = `✓ ${fn}`; clearTimeout(st._t); st._t = setTimeout(() => st.textContent = '', 2500); }
  } catch { if (st) st.textContent = '(local only)'; }
}

// ─── Browser window ───────────────────────────────────────────────────────────
function browserGo() {
  const input = document.getElementById('browser-url');
  const frame = document.getElementById('browser-iframe');
  if (!input || !frame) return;
  let url = input.value.trim();
  if (url && !url.startsWith('http')) url = 'http://' + url;
  frame.src = url;
}

function browserBack() {
  const frame = document.getElementById('browser-iframe');
  if (frame?.contentWindow) frame.contentWindow.history.back();
}

// ─── Camera window ───────────────────────────────────────────────────────────
async function cameraSnap(night = false) {
  const img         = document.getElementById('camera-snapshot');
  const placeholder = document.getElementById('camera-placeholder');
  const status      = document.getElementById('camera-status');
  const snapBtn     = document.getElementById('camera-snap-btn');
  const nightBtn    = document.getElementById('camera-night-btn');

  if (!img) return;
  if (snapBtn)  snapBtn.disabled = true;
  if (nightBtn) nightBtn.disabled = true;
  if (status)   status.textContent = night ? 'capturing (night mode)…' : 'capturing…';
  if (placeholder) placeholder.style.display = 'none';

  try {
    const url = `/api/snapshot${night ? '?night=true' : ''}`;
    const r = await fetch(url);
    if (!r.ok) {
      const txt = await r.text();
      if (status) status.textContent = 'error: ' + txt.slice(0, 80);
      if (placeholder) { placeholder.textContent = 'Capture failed'; placeholder.style.display = ''; }
      return;
    }
    const blob = await r.blob();
    const objUrl = URL.createObjectURL(blob);
    if (img.src.startsWith('blob:')) URL.revokeObjectURL(img.src);
    img.src = objUrl;
    img.style.display = 'block';
    const ts = new Date().toLocaleTimeString();
    if (status) status.textContent = `${night ? 'night' : 'snap'} · ${ts}`;
  } catch (e) {
    if (status) status.textContent = 'error: ' + e;
    if (placeholder) { placeholder.textContent = 'Capture failed'; placeholder.style.display = ''; }
  } finally {
    if (snapBtn)  snapBtn.disabled = false;
    if (nightBtn) nightBtn.disabled = false;
  }
}

// ─── Alpine data for Settings window ─────────────────────────────────────────
function settingsApp() {
  return {
    tab: 'soul',
    soul: '',
    saving: false,
    saved: false,
    err: '',
    currentMode: 'suggest',
    rules: [],
    plugins: [],
    wallpaper: localStorage.getItem('apexos_wallpaper') || 'thermal',

    async init() {
      // Load soul.md
      try {
        const r = await fetch('/api/soul');
        const d = await r.json();
        if (d.ok) this.soul = d.content;
        else this.err = d.error;
      } catch (e) { this.err = String(e); }

      // Load current policy mode + rules
      try {
        const r = await fetch('/api/status');
        const d = await r.json();
        if (d.policy_mode) this.currentMode = d.policy_mode;
      } catch {}

      try {
        const r = await fetch('/api/policy/rules');
        const d = await r.json();
        if (d.rules) this.rules = Object.entries(d.rules);
      } catch {}

      // Poll plugin counts from app.js global
      this._refreshPlugins();
      setInterval(() => this._refreshPlugins(), 5000);
    },

    _refreshPlugins() {
      if (typeof pluginCounts !== 'undefined') {
        this.plugins = Object.entries(pluginCounts);
      }
    },

    async saveSoul() {
      this.saving = true; this.saved = false; this.err = '';
      try {
        const r = await fetch('/api/soul', {
          method: 'POST',
          headers: { 'content-type': 'application/json' },
          body: JSON.stringify({ content: this.soul }),
        });
        const d = await r.json();
        if (d.ok) { this.saved = true; setTimeout(() => this.saved = false, 3000); }
        else this.err = d.error;
      } catch (e) { this.err = String(e); }
      this.saving = false;
    },

    setWallpaper(mode) {
      this.wallpaper = mode;
      localStorage.setItem('apexos_wallpaper', mode);
      applyWallpaper(mode);
    },

    async setMode(mode) {
      this.currentMode = mode;
      try {
        await fetch('/api/policy', {
          method: 'POST',
          headers: { 'content-type': 'application/json' },
          body: JSON.stringify({ mode }),
        });
        // Sync the topbar select
        const sel = document.getElementById('policy-select');
        if (sel) { sel.value = mode; setPolicySelect(mode); }
      } catch {}
    },
  };
}

// ─── Sub-agent windows ───────────────────────────────────────────────────────
window.openSubAgentWin = function(ev) {
  const child  = ev.child;
  const prompt = (ev.prompt || '').slice(0, 100);
  const winId  = 'subagent-' + child;
  if (wins[winId]) { wins[winId].focus(); return; }

  const container = document.createElement('div');
  container.id        = 'win-' + winId + '-content';
  container.className = 'win-content subagent-win';

  const header = document.createElement('div');
  header.className   = 'subagent-header';

  const prompt_el = document.createElement('span');
  prompt_el.className = 'subagent-prompt';
  prompt_el.textContent = '▸ ' + prompt;

  const status = document.createElement('span');
  status.className   = 'subagent-status';
  status.textContent = '⟳ running';
  status.style.color = 'var(--accent2)';

  header.append(prompt_el, status);

  const output = document.createElement('div');
  output.className = 'subagent-output';

  container.append(header, output);
  document.body.appendChild(container);

  // Register so app.js routes child session events here
  window.addWatchedSession(child, { outputEl: output, statusEl: status });

  const cfg = {
    title: '🤖 Agent #' + child,
    x: 120 + (child % 6) * 30, y: 90 + (child % 4) * 30,
    width: 640, height: 440,
    background: 'var(--wb-bg)',
  };

  wins[winId] = new WinBox(cfg.title, {
    ...cfg,
    class: 'wb-apexos wb-subagent',
    mount: container,
    onclose() {
      window.removeWatchedSession(child);
      container.remove();
      delete wins[winId];
      removeTaskbarTab(winId);
      return false;
    },
    onfocus()    { updateTab(winId, 'active'); },
    onblur()     { updateTab(winId, 'open'); },
    onminimize() { updateTab(winId, 'minimized'); },
    onrestore()  { updateTab(winId, 'active'); },
  });

  createTaskbarTab(winId, cfg.title);
  updateTab(winId, 'active');
};

// ─── Monaco IDE ───────────────────────────────────────────────────────────────
let monacoEditor = null;
let monacoReady  = false;
let ideCurrentPath = '';

const IDE_LANG_MAP = {
  rs: 'rust', js: 'javascript', ts: 'typescript', py: 'python',
  html: 'html', css: 'css', scss: 'scss', less: 'less',
  md: 'markdown', json: 'json', yaml: 'yaml', yml: 'yaml',
  sh: 'shell', bash: 'shell', toml: 'ini', xml: 'xml',
  sql: 'sql', go: 'go', cpp: 'cpp', c: 'c', cs: 'csharp',
  java: 'java', rb: 'ruby', php: 'php', swift: 'swift',
  kt: 'kotlin', lua: 'lua', r: 'r', txt: 'plaintext',
};

function ideLang(path) {
  const ext = (path || '').split('.').pop().toLowerCase();
  return IDE_LANG_MAP[ext] || 'plaintext';
}

function ideSetStatus(msg, ok = true) {
  const el = document.getElementById('ide-status');
  if (!el) return;
  el.textContent = msg;
  el.style.color = ok ? 'var(--accent)' : 'var(--accent3)';
  if (msg) { clearTimeout(el._t); el._t = setTimeout(() => el.textContent = '', 3000); }
}

function ideSetLang(lang) {
  const el = document.getElementById('ide-lang');
  if (el) el.textContent = lang;
  if (monacoEditor && monacoReady) {
    const model = monacoEditor.getModel();
    if (model) monaco.editor.setModelLanguage(model, lang);
  }
}

function ideInit() {
  if (monacoReady) {
    ideApplyPendingFile();
    return;
  }

  require.config({ paths: { vs: '/lib/monaco/vs' } });
  require(['vs/editor/editor.main'], function() {
    const container = document.getElementById('ide-editor');
    if (!container) return;

    monacoEditor = monaco.editor.create(container, {
      value: '',
      language: 'plaintext',
      theme: 'vs-dark',
      fontSize: 13,
      fontFamily: "'JetBrains Mono', 'Fira Code', 'Cascadia Code', Consolas, monospace",
      lineHeight: 20,
      minimap: { enabled: false },
      scrollBeyondLastLine: false,
      wordWrap: 'off',
      renderWhitespace: 'selection',
      smoothScrolling: true,
      cursorBlinking: 'smooth',
      tabSize: 2,
      automaticLayout: true,   // resizes with the WinBox window
    });

    // Ctrl+S to save
    monacoEditor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS, ideSave);

    monacoReady = true;
    ideApplyPendingFile();
  });
}

function ideApplyPendingFile() {
  // Called after Monaco loads — apply window.ideFile if explorer sent one
  if (window.ideFile) {
    const f = window.ideFile;
    window.ideFile = null;
    ideSetContent(f.path, f.content);
  }
}

function ideSetContent(path, content) {
  ideCurrentPath = path;
  const pathEl = document.getElementById('ide-path');
  if (pathEl) pathEl.value = path;
  const lang = ideLang(path);
  ideSetLang(lang);
  if (monacoEditor) {
    monacoEditor.setValue(content);
    monacoEditor.setScrollPosition({ scrollTop: 0 });
    monacoEditor.focus();
  }
}

function ideNew() {
  const defaultPath = '/var/lib/agentd/workspace/untitled.txt';
  ideCurrentPath = defaultPath;
  const pathEl = document.getElementById('ide-path');
  if (pathEl) pathEl.value = defaultPath;
  ideSetLang('plaintext');
  if (monacoEditor) { monacoEditor.setValue(''); monacoEditor.focus(); }
  ideSetStatus('new file', true);
}

function ideUpload() {
  const input = document.getElementById('ide-upload-input');
  if (!input) return;
  input.onchange = (e) => {
    const file = e.target.files?.[0];
    if (!file) return;
    input.value = '';
    const reader = new FileReader();
    reader.onload = (ev) => {
      const content = ev.target.result;
      const path    = '/var/lib/agentd/workspace/' + file.name;
      ideCurrentPath = path;
      const pathEl = document.getElementById('ide-path');
      if (pathEl) pathEl.value = path;
      ideSetLang(ideLang(file.name));
      if (monacoEditor) {
        monacoEditor.setValue(content);
        monacoEditor.setScrollPosition({ scrollTop: 0 });
        monacoEditor.focus();
      }
      ideSetStatus('loaded · hit Save to write', true);
    };
    reader.readAsText(file);
  };
  input.click();
}

async function ideLoad() {
  const pathEl = document.getElementById('ide-path');
  const path   = pathEl?.value?.trim();
  if (!path) return;
  ideSetStatus('loading…', true);
  try {
    const r = await fetch('/api/run', {
      method: 'POST', headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ command: `cat '${path}'` }),
    });
    const d = await r.json();
    if (d.exit_code !== 0) { ideSetStatus('not found', false); return; }
    ideSetContent(path, d.stdout || '');
    ideSetStatus('loaded', true);
  } catch (e) { ideSetStatus('error: ' + e, false); }
}

async function ideSave() {
  if (!monacoEditor || !ideCurrentPath) return;
  const content  = monacoEditor.getValue();
  const escaped  = content.replace(/'/g, "'\\''");
  ideSetStatus('saving…', true);
  try {
    const r = await fetch('/api/run', {
      method: 'POST', headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ command: `printf '%s' '${escaped}' > '${ideCurrentPath}'` }),
    });
    const d = await r.json();
    if (d.exit_code === 0) ideSetStatus('✓ saved', true);
    else ideSetStatus('save failed', false);
  } catch (e) { ideSetStatus('error: ' + e, false); }
}

// ─── File Explorer ───────────────────────────────────────────────────────────
let explorerRoot     = '/var/lib/agentd/workspace';
let explorerSelected = null;

function explorerFileIcon(name) {
  if (/\.(md|txt|log|csv)$/i.test(name))              return '📄';
  if (/\.(js|ts|rs|py|sh|toml|json|yaml|yml|html|css)$/i.test(name)) return '📜';
  if (/\.(png|jpg|jpeg|gif|svg|webp)$/i.test(name))   return '🖼';
  if (/\.(mp3|wav|ogg|flac|opus)$/i.test(name))       return '🎵';
  return '📄';
}

async function explorerList(path) {
  try {
    const r = await fetch('/api/run', {
      method: 'POST', headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ command: `find '${path}' -maxdepth 1 -mindepth 1 -printf "%f\\t%y\\n" 2>&1 | sort` }),
    });
    const d = await r.json();
    if (!d.ok && d.exit_code !== 0) return [];
    return (d.stdout || '').trim().split('\n').filter(Boolean).map(line => {
      const tab = line.indexOf('\t');
      const name = line.slice(0, tab);
      const type = line.slice(tab + 1).trim();
      return { name, isDir: type === 'd' };
    }).sort((a, b) => {
      if (a.isDir !== b.isDir) return a.isDir ? -1 : 1;
      return a.name.localeCompare(b.name);
    });
  } catch { return []; }
}

async function explorerLoadInto(path, container, depth) {
  container.innerHTML = '<span class="explorer-loading">loading…</span>';
  const entries = await explorerList(path);
  container.innerHTML = '';
  if (!entries.length) {
    container.innerHTML = `<span class="explorer-loading" style="padding-left:${depth*16+8}px">(empty)</span>`;
    return;
  }
  for (const e of entries) {
    const fullPath = path.replace(/\/+$/, '') + '/' + e.name;
    const row = document.createElement('div');
    row.className = 'explorer-row';
    row.style.paddingLeft = (depth * 16 + 8) + 'px';
    row.dataset.path = fullPath;
    const iconEl = document.createElement('span');
    iconEl.className = 'explorer-icon';
    iconEl.textContent = e.isDir ? '📁' : explorerFileIcon(e.name);
    const nameEl = document.createElement('span');
    nameEl.className = 'explorer-name';
    nameEl.textContent = e.name;
    row.append(iconEl, nameEl);

    if (e.isDir) {
      const children = document.createElement('div');
      children.className = 'explorer-children';
      children.style.display = 'none';
      let loaded = false;
      row.addEventListener('click', async (ev) => {
        ev.stopPropagation();
        explorerSelectRow(row, fullPath, true);
        if (children.style.display !== 'none') {
          children.style.display = 'none';
          iconEl.textContent = '📁';
        } else {
          children.style.display = '';
          iconEl.textContent = '📂';
          if (!loaded) { loaded = true; await explorerLoadInto(fullPath, children, depth + 1); }
        }
      });
      container.append(row, children);
    } else {
      row.addEventListener('click', (ev) => {
        ev.stopPropagation();
        explorerSelectRow(row, fullPath, false);
        explorerPreview(fullPath);
      });
      container.append(row);
    }
  }
}

function explorerSelectRow(row, path, isDir) {
  document.querySelectorAll('.explorer-row.selected').forEach(r => r.classList.remove('selected'));
  row.classList.add('selected');
  explorerSelected = { path, isDir };
  const lbl = document.getElementById('explorer-path');
  if (lbl) lbl.textContent = path;
}

async function explorerPreview(path) {
  const pre     = document.getElementById('explorer-preview-content');
  const pathEl  = document.getElementById('explorer-preview-path');
  const actions = document.getElementById('explorer-preview-actions');
  if (!pre) return;
  if (pathEl) pathEl.textContent = path;
  pre.textContent = 'loading…';
  if (actions) actions.style.display = 'none';
  try {
    const r = await fetch('/api/run', {
      method: 'POST', headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ command: `head -120 '${path}' 2>&1` }),
    });
    const d = await r.json();
    pre.textContent = d.stdout || d.stderr || '(empty)';
    if (actions) actions.style.display = '';
  } catch (e) { pre.textContent = 'error: ' + e; }
}

async function explorerInit() {
  // Ensure workspace dir exists
  await fetch('/api/run', {
    method: 'POST', headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ command: 'mkdir -p /var/lib/agentd/workspace' }),
  });
  const tree = document.getElementById('explorer-tree');
  if (tree) await explorerLoadInto(explorerRoot, tree, 0);
  const lbl = document.getElementById('explorer-path');
  if (lbl) lbl.textContent = explorerRoot;

  const uploadInput = document.getElementById('explorer-upload');
  if (uploadInput && !uploadInput._wired) {
    uploadInput._wired = true;
    uploadInput.onchange = (e) => {
      explorerHandleUpload([...e.target.files]);
      e.target.value = '';
    };
  }
}

function explorerUpload() {
  document.getElementById('explorer-upload')?.click();
}

async function explorerHandleUpload(files) {
  const destDir = (explorerSelected?.isDir ? explorerSelected.path : explorerRoot);
  for (const file of files) {
    const buf   = await file.arrayBuffer();
    const bytes = new Uint8Array(buf);
    // Chunked to avoid stack overflow on large files
    let binary = '';
    for (let i = 0; i < bytes.byteLength; i += 8192) {
      binary += String.fromCharCode(...bytes.subarray(i, Math.min(i + 8192, bytes.byteLength)));
    }
    const b64  = btoa(binary);
    const dest = destDir.replace(/\/+$/, '') + '/' + file.name;
    await fetch('/api/run', {
      method: 'POST', headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ command: `echo '${b64}' | base64 -d > '${dest}'` }),
    });
  }
  explorerRefresh();
}

function explorerOpenInIDE() {
  if (!explorerSelected || explorerSelected.isDir) return;
  fetch('/api/run', {
    method: 'POST', headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ command: `cat '${explorerSelected.path}'` }),
  }).then(r => r.json()).then(d => {
    // IDE reads window.ideFile on init
    window.ideFile = { path: explorerSelected.path, content: d.stdout || '' };
    openWin('ide');
  });
}

async function explorerRefresh() {
  explorerSelected = null;
  const pre = document.getElementById('explorer-preview-content');
  const actions = document.getElementById('explorer-preview-actions');
  const pathEl = document.getElementById('explorer-preview-path');
  if (pre) pre.textContent = 'Select a file to preview';
  if (actions) actions.style.display = 'none';
  if (pathEl) pathEl.textContent = '';
  const lbl = document.getElementById('explorer-path');
  if (lbl) lbl.textContent = explorerRoot;
  const tree = document.getElementById('explorer-tree');
  if (tree) await explorerLoadInto(explorerRoot, tree, 0);
}

async function explorerNewDir() {
  const base = (explorerSelected?.isDir ? explorerSelected.path : explorerRoot);
  const name = prompt('New directory name:');
  if (!name?.trim()) return;
  await fetch('/api/run', {
    method: 'POST', headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ command: `mkdir -p '${base}/${name.trim()}'` }),
  });
  explorerRefresh();
}

async function explorerDelete() {
  if (!explorerSelected) return;
  if (!confirm(`Delete ${explorerSelected.path}?\n\nThis cannot be undone.`)) return;
  await fetch('/api/run', {
    method: 'POST', headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ command: `rm -rf '${explorerSelected.path}'` }),
  });
  explorerSelected = null;
  const pre = document.getElementById('explorer-preview-content');
  const actions = document.getElementById('explorer-preview-actions');
  const pathEl  = document.getElementById('explorer-preview-path');
  if (pre) pre.textContent = 'Select a file to preview';
  if (actions) actions.style.display = 'none';
  if (pathEl) pathEl.textContent = '';
  explorerRefresh();
}

function explorerOpenInNotes() {
  if (!explorerSelected || explorerSelected.isDir) return;
  const fn = explorerSelected.path.split('/').pop();
  fetch('/api/run', {
    method: 'POST', headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ command: `cat '${explorerSelected.path}'` }),
  }).then(r => r.json()).then(d => {
    const ed = document.getElementById('notes-editor');
    const fnInput = document.getElementById('notes-filename');
    if (fnInput) fnInput.value = fn;
    if (ed) { ed.value = d.stdout || ''; ed.dispatchEvent(new Event('input')); }
    openWin('notes');
  });
}

// ─── Init ─────────────────────────────────────────────────────────────────────
document.addEventListener('DOMContentLoaded', () => {
  // Power modal
  document.getElementById('power-btn')?.addEventListener('click', () => {
    if (typeof showPowerModal === 'function') showPowerModal();
  });
  // Evo badge click
  document.getElementById('hdr-evo')?.addEventListener('click', () => {
    if (typeof showEvoModal === 'function') showEvoModal();
  });
  // Logo dblclick → sessions modal
  document.getElementById('hdr-logo')?.addEventListener('dblclick', () => {
    if (typeof showSessionModal === 'function') showSessionModal();
  });

  // Click outside start menu to close it
  document.addEventListener('click', (e) => {
    const menu = document.getElementById('start-menu');
    const startBtn = document.getElementById('start-btn');
    if (menu && !menu.classList.contains('hidden') &&
        !menu.contains(e.target) && e.target !== startBtn) {
      closeStartMenu();
    }
  });
});
