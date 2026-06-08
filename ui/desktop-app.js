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
  player: {
    title: '🎵 Sonus Player',
    x: 200, y: 80, width: 520, height: 480,
    background: 'var(--wb-bg)',
  },
  home: {
    title: '🏠 ApexOS Home',
    x: 60, y: 50, width: 860, height: 560,
    background: 'var(--wb-bg)',
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
  // Auto-navigate browser to sensorhead on first open; set input to Pi hostname
  if (id === 'browser') {
    const input = document.getElementById('browser-url');
    if (input && !input.value) input.value = `http://${location.hostname}:8080`;
    setTimeout(() => {
      const frame = document.getElementById('browser-iframe');
      const inp   = document.getElementById('browser-url');
      if (frame && inp && !frame.getAttribute('src')) {
        let url = inp.value.trim() || `http://${location.hostname}:8080`;
        if (!url.startsWith('http')) url = 'http://' + url;
        frame.src = url;
      }
    }, 80);
  }

  content.style.display = '';

  if (id === 'terminal') setTimeout(initTerminal, 120);
  if (id === 'notes')     setTimeout(notesInit, 30);
  if (id === 'sketchpad') setTimeout(sketchInit, 30);
  if (id === 'explorer')  setTimeout(explorerInit, 30);
  if (id === 'ide')       setTimeout(ideInit, 60);
  if (id === 'player')    setTimeout(playerInit, 30);
  if (id === 'home')      setTimeout(homeInit, 30);

  const cfg = WIN_DEFAULTS[id] || { title: id, x: 100, y: 80, width: 600, height: 400 };
  wins[id] = new WinBox(cfg.title, {
    ...cfg,
    class: `wb-apexos wb-${id}`,
    mount: content,
    onclose() {
      content.style.display = 'none';
      if (id === 'home') homeStop();
      delete wins[id];
      removeTaskbarTab(id);
      return false;
    },
    onfocus()    { updateTab(id, 'active'); },
    onblur()     { updateTab(id, 'open'); },
    onminimize() { updateTab(id, 'minimized'); },
    onrestore()  {
      updateTab(id, 'active');
      if (id === 'terminal' && termFit) setTimeout(() => { try { termFit.fit(); } catch(e) {} }, 30);
    },
    onresize()   {
      if (id === 'terminal' && termFit) setTimeout(() => { try { termFit.fit(); } catch(e) {} }, 30);
    },
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
let termWs   = null;

function initTerminal() {
  if (term) {
    if (!termWs || termWs.readyState > 1) termConnectWs();
    if (termFit) setTimeout(() => { try { termFit.fit(); } catch(e) {} }, 80);
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
  // fit() first pass — must not block termConnectWs() if it throws
  setTimeout(() => { try { termFit.fit(); } catch(e) {} }, 80);
  setTimeout(() => { termConnectWs(); }, 100);
  // second fit pass after WinBox animation settles
  setTimeout(() => { try { termFit.fit(); } catch(e) {} }, 400);

  term.onResize(({ cols, rows }) => {
    if (termWs && termWs.readyState === 1)
      termWs.send(JSON.stringify({ type: 'resize', cols, rows }));
  });

  term.onData(data => {
    if (termWs && termWs.readyState === 1) termWs.send(data);
  });
}

function termConnectWs() {
  const proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
  termWs = new WebSocket(`${proto}//${location.host}/terminal-ws`);
  termWs.binaryType = 'arraybuffer';
  termWs.onopen = () => {
    term.writeln('\x1b[32m▸ Terminal connected\x1b[0m');
    termWs.send(JSON.stringify({ type: 'resize', cols: term.cols, rows: term.rows }));
  };
  termWs.onmessage = e => {
    if (e.data instanceof ArrayBuffer) term.write(new Uint8Array(e.data));
    else term.write(e.data);
  };
  termWs.onclose = () => {
    if (term) term.writeln('\r\n\x1b[31m▸ disconnected\x1b[0m');
  };
  termWs.onerror = () => {};
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

function browserForward() {
  const frame = document.getElementById('browser-iframe');
  if (frame?.contentWindow) frame.contentWindow.history.forward();
}

function browserRefresh() {
  const frame = document.getElementById('browser-iframe');
  if (frame?.contentWindow) frame.contentWindow.location.reload();
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

// ─── Media Player ─────────────────────────────────────────────────────────────
let playerAudio  = null;
let playerTracks = [];
let playerIndex  = -1;

function playerFmt(secs) {
  if (!isFinite(secs) || isNaN(secs)) return '0:00';
  const m = Math.floor(secs / 60);
  const s = Math.floor(secs % 60);
  return m + ':' + String(s).padStart(2, '0');
}

function playerInit() {
  const audio = document.getElementById('player-audio');
  if (!audio) return;
  if (playerAudio) { playerRefresh(); return; }
  playerAudio = audio;

  const seek = document.getElementById('player-seek');
  const vol  = document.getElementById('player-vol');
  playerAudio.volume = parseFloat(vol?.value ?? '0.8');

  playerAudio.addEventListener('timeupdate', () => {
    const timeEl = document.getElementById('player-time');
    if (timeEl) timeEl.textContent = playerFmt(playerAudio.currentTime);
    if (seek && playerAudio.duration) {
      seek.value = (playerAudio.currentTime / playerAudio.duration * 100).toFixed(1);
    }
  });
  playerAudio.addEventListener('loadedmetadata', () => {
    const durEl = document.getElementById('player-duration');
    if (durEl) durEl.textContent = playerFmt(playerAudio.duration);
  });
  playerAudio.addEventListener('ended', playerNext);
  playerAudio.addEventListener('play',  () => {
    const btn = document.getElementById('player-play-btn');
    if (btn) btn.textContent = '⏸';
    document.querySelectorAll('.player-track-play').forEach((b, i) => {
      b.textContent = i === playerIndex ? '⏸' : '▶';
    });
  });
  playerAudio.addEventListener('pause', () => {
    const btn = document.getElementById('player-play-btn');
    if (btn) btn.textContent = '▶';
    document.querySelectorAll('.player-track-play').forEach(b => b.textContent = '▶');
  });

  if (seek) {
    seek.addEventListener('input', () => {
      if (playerAudio?.duration)
        playerAudio.currentTime = (parseFloat(seek.value) / 100) * playerAudio.duration;
    });
  }
  if (vol) {
    vol.addEventListener('input', () => {
      if (playerAudio) playerAudio.volume = parseFloat(vol.value);
    });
  }

  playerRefresh();
}

async function playerRefresh() {
  const list = document.getElementById('player-tracklist');
  if (!list) return;
  list.innerHTML = '<div class="player-empty">Loading…</div>';
  try {
    const r = await fetch('/api/sonus/files');
    const tracks = await r.json();
    playerTracks = tracks.map(t => ({ name: t.name, url: t.url }));
    if (!playerTracks.length) {
      list.innerHTML = '<div class="player-empty">No tracks yet — ask the agent to generate one!</div>';
      return;
    }
    list.innerHTML = '';
    playerTracks.forEach((t, i) => {
      const row      = document.createElement('div');
      row.className  = 'player-track-row' + (i === playerIndex ? ' active' : '');
      row.dataset.idx = i;

      const name = document.createElement('span');
      name.className   = 'player-track-name';
      name.textContent = t.name;

      const btn = document.createElement('button');
      btn.className   = 'player-track-play';
      btn.textContent = (i === playerIndex && playerAudio && !playerAudio.paused) ? '⏸' : '▶';
      btn.onclick = (e) => { e.stopPropagation(); playerSelect(i); };

      row.append(name, btn);
      row.addEventListener('click', () => playerSelect(i));
      list.appendChild(row);
    });
  } catch (e) {
    list.innerHTML = `<div class="player-empty">Error: ${e}</div>`;
  }
}

function playerSelect(idx) {
  if (!playerAudio) return;
  if (idx < 0 || idx >= playerTracks.length) return;
  playerIndex = idx;
  const t = playerTracks[idx];

  playerAudio.src = t.url;
  playerAudio.load();
  playerAudio.play().catch(() => {});

  const np = document.getElementById('player-now-playing');
  if (np) np.textContent = t.name;

  document.querySelectorAll('.player-track-row').forEach((row, i) => {
    row.classList.toggle('active', i === idx);
  });
}

function playerToggle() {
  if (!playerAudio) return;
  if (playerIndex < 0 && playerTracks.length > 0) { playerSelect(0); return; }
  if (playerAudio.paused) playerAudio.play().catch(() => {});
  else playerAudio.pause();
}

function playerNext() {
  if (!playerTracks.length) return;
  playerSelect((playerIndex + 1) % playerTracks.length);
}

function playerPrev() {
  if (!playerTracks.length) return;
  // If more than 3s in, restart current track; otherwise go back
  if (playerAudio && playerAudio.currentTime > 3) {
    playerAudio.currentTime = 0;
    return;
  }
  playerSelect(playerIndex <= 0 ? playerTracks.length - 1 : playerIndex - 1);
}

// ─── Home / Dashboard ────────────────────────────────────────────────────────
let homeInterval = null;

function homeStop() {
  clearInterval(homeInterval);
  homeInterval = null;
}

function homeInit() {
  homeRefresh();
  if (!homeInterval) homeInterval = setInterval(homeRefresh, 6000);
}

async function homeRunCmd(cmd) {
  try {
    const r = await fetch('/api/run', {
      method: 'POST', headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ command: cmd }),
    });
    const j = await r.json();
    return j.ok ? j.stdout.trim() : '';
  } catch { return ''; }
}

async function homeRefresh() {
  const set = (id, val) => { const e = document.getElementById(id); if (e) e.textContent = val; };

  // ── system stats (parallel shell calls) ──────────────────────────────────
  const [tempStr, ramStr, diskStr, uptimeRaw] = await Promise.all([
    homeRunCmd("awk '{printf \"%.1f\", $1/1000}' /sys/class/thermal/thermal_zone0/temp 2>/dev/null || echo 0"),
    homeRunCmd("free -m | awk 'NR==2{printf \"%d %d\", $3, $2}'"),
    homeRunCmd("df -BG / | awk 'NR==2{gsub(/G/,\"\"); printf \"%d %d\", $3, $2}'"),
    homeRunCmd("awk '{d=int($1/86400);h=int(($1%86400)/3600);m=int(($1%3600)/60);printf \"%dd %dh %dm\",d,h,m}' /proc/uptime"),
  ]);

  // CPU temp
  const cpuTemp = parseFloat(tempStr) || 0;
  const tempEl = document.getElementById('h-cputemp');
  if (tempEl) {
    tempEl.textContent = cpuTemp.toFixed(1);
    tempEl.style.color = cpuTemp >= 70 ? '#ff4444' : cpuTemp >= 60 ? '#f0b429' : '#39ff14';
  }

  // RAM
  const [ramUsed, ramTotal] = ramStr.split(' ').map(Number);
  if (ramTotal > 0) {
    const pct = Math.round(ramUsed / ramTotal * 100);
    const ramBar = document.getElementById('h-ram-bar');
    if (ramBar) { ramBar.style.width = pct + '%'; ramBar.style.background = pct > 85 ? '#ff4444' : pct > 65 ? '#f0b429' : 'var(--accent)'; }
    set('h-ram-val', `${(ramUsed/1024).toFixed(1)} / ${(ramTotal/1024).toFixed(1)} GB`);
  }

  // Disk
  const [diskUsed, diskTotal] = diskStr.split(' ').map(Number);
  if (diskTotal > 0) {
    const pct = Math.round(diskUsed / diskTotal * 100);
    const diskBar = document.getElementById('h-disk-bar');
    if (diskBar) { diskBar.style.width = pct + '%'; diskBar.style.background = pct > 90 ? '#ff4444' : pct > 75 ? '#f0b429' : 'var(--accent)'; }
    set('h-disk-val', `${diskUsed}G / ${diskTotal}G`);
  }

  // Uptime
  set('h-uptime', uptimeRaw || '—');

  // ── agent status ──────────────────────────────────────────────────────────
  try {
    const st = await fetch('/api/status').then(r => r.json());
    set('h-model',  st.model       || '—');
    set('h-policy', st.policy_mode || '—');
  } catch {}

  // ── evolution stats ───────────────────────────────────────────────────────
  try {
    const ev = await fetch('/api/evolution/stats').then(r => r.json());
    set('h-evo', `${ev.applied_total || 0} applied · ${(ev.rollback_rate || 0).toFixed(1)}% rollback`);
  } catch {}

  // ── sessions ──────────────────────────────────────────────────────────────
  try {
    const sessions = await fetch('/api/sessions').then(r => r.json());
    set('h-sessions-count', sessions.length ? `(${sessions.length})` : '');
    const el = document.getElementById('h-sessions');
    if (el) {
      el.innerHTML = '';
      sessions.slice(0, 5).forEach(s => {
        const item = document.createElement('div');
        item.className = 'hc-session-item';
        const age = homeTimeAgo(s.last_active);
        const preview = (s.preview || '').slice(0, 72) || '(no preview)';
        item.innerHTML =
          `<span class="hc-sid">#${s.session_id}</span>` +
          `<span class="hc-spreview">${esc(preview)}</span>` +
          `<span class="hc-sage">${age}</span>`;
        el.appendChild(item);
      });
    }
  } catch {}

  // ── sensor state (from global already populated by bus events) ────────────
  homeRenderSensors();

  // ── plugins ───────────────────────────────────────────────────────────────
  const pluginsEl = document.getElementById('h-plugins');
  if (pluginsEl) {
    pluginsEl.innerHTML = '';
    const counts = window.pluginCounts || {};
    if (Object.keys(counts).length === 0) {
      pluginsEl.innerHTML = '<span style="color:var(--text-dim);font-size:10px">none online</span>';
    } else {
      Object.entries(counts).forEach(([name, tools]) => {
        const dot = document.createElement('span');
        dot.className = 'hc-plugin-dot';
        dot.title = `${tools} tools`;
        dot.textContent = `● ${name}`;
        pluginsEl.appendChild(dot);
      });
    }
  }
}

function homeRenderSensors() {
  const env     = window.sensorState?.env;
  const thermal = window.sensorState?.thermal;

  const set = (id, val) => { const e = document.getElementById(id); if (e) e.textContent = val; };

  if (env) {
    const iaq = env.iaq ?? 0;
    const iaqEl  = document.getElementById('h-iaq-num');
    const lblEl  = document.getElementById('h-iaq-label');
    if (iaqEl)  { iaqEl.textContent = Math.round(iaq); iaqEl.style.color = iaqColor(iaq); }
    if (lblEl)  { lblEl.textContent = iaqLabel(iaq); lblEl.style.color = iaqColor(iaq); }
    set('h-env-temp',  env.temperature_c != null ? `${env.temperature_c.toFixed(1)} °C` : '—');
    set('h-env-humid', env.humidity_pct  != null ? `${env.humidity_pct.toFixed(1)} %`   : '—');
    set('h-env-press', env.pressure_hpa  != null ? `${env.pressure_hpa.toFixed(0)} hPa` : '—');
  } else {
    ['h-iaq-num','h-env-temp','h-env-humid','h-env-press'].forEach(id => set(id, '—'));
    const lblEl = document.getElementById('h-iaq-label');
    if (lblEl) lblEl.textContent = 'no sensor';
  }

  if (thermal) homeDrawThermal(thermal);
}

function homeDrawThermal(frame) {
  const canvas = document.getElementById('h-thermal');
  if (!canvas) return;
  const ctx = canvas.getContext('2d');
  const W = 32, H = 24;
  const cw = canvas.width / W, ch = canvas.height / H;
  const minC = frame.min_c ?? 20, maxC = frame.max_c ?? 35;
  const range = Math.max(1, maxC - minC);
  const pixels = frame.pixels || [];
  for (let y = 0; y < H; y++) {
    for (let x = 0; x < W; x++) {
      const t = pixels[y * W + x] ?? frame.mean_c ?? ((minC + maxC) / 2);
      const n = Math.max(0, Math.min(1, (t - minC) / range));
      const r = Math.round(n * 255);
      const b = Math.round((1 - n) * 200);
      ctx.fillStyle = `rgb(${r},${Math.round(n * 80)},${b})`;
      ctx.fillRect(x * cw, y * ch, cw + 0.5, ch + 0.5);
    }
  }
}

function homeTimeAgo(ts) {
  if (!ts) return '—';
  const sec = Math.floor(Date.now() / 1000 - ts);
  if (sec < 60)    return 'just now';
  if (sec < 3600)  return `${Math.floor(sec / 60)}m ago`;
  if (sec < 86400) return `${Math.floor(sec / 3600)}h ago`;
  return `${Math.floor(sec / 86400)}d ago`;
}

// ─── Voice Input (STT) + Voice Output (TTS) ──────────────────────────────────

let micRecorder      = null;
let micChunks        = [];
let micServerActive  = false;
let speakerOn        = false;
let agentTurnText    = '';  // accumulates text during current agent turn for TTS

// Primary: server-side ALSA recording (works in kiosk without PipeWire/HTTPS)
// Fallback: browser MediaRecorder (works when remote with HTTPS)

async function micToggle() {
  const btn = document.getElementById('mic-btn');
  // --- stop server recording ---
  if (micServerActive) {
    micServerActive = false;
    btn.classList.remove('recording');
    btn.textContent = '⏳';
    btn.disabled = true;
    try {
      const resp = await fetch('/api/record/stop', { method: 'POST' });
      if (resp.ok) {
        const { text } = await resp.json();
        const inp = document.getElementById('prompt-input');
        if (inp && text && text.trim()) {
          inp.value = (inp.value ? inp.value + ' ' : '') + text.trim();
          inp.focus();
        }
      }
    } catch (err) { console.warn('[mic] record/stop error', err); }
    finally { btn.textContent = '🎤'; btn.disabled = false; }
    return;
  }
  // --- stop browser recording ---
  if (micRecorder && micRecorder.state === 'recording') {
    micRecorder.stop();
    return;
  }
  // --- start: try server-side first ---
  try {
    const resp = await fetch('/api/record/start', { method: 'POST' });
    if (resp.ok) {
      micServerActive = true;
      btn.classList.add('recording');
      btn.textContent = '⏹';
      return;
    }
  } catch (e) { /* fall through to browser */ }

  // --- fallback: browser MediaRecorder ---
  if (!navigator.mediaDevices) {
    alert('Server mic unavailable and browser mic requires HTTPS/localhost');
    return;
  }
  navigator.mediaDevices.getUserMedia({ audio: true }).then(stream => {
    micChunks = [];
    micRecorder = new MediaRecorder(stream);
    micRecorder.ondataavailable = e => { if (e.data.size > 0) micChunks.push(e.data); };
    micRecorder.onstop = async () => {
      stream.getTracks().forEach(t => t.stop());
      btn.classList.remove('recording');
      btn.textContent = '⏳';
      btn.disabled = true;
      try {
        const blob = new Blob(micChunks, { type: 'audio/webm' });
        const resp = await fetch('/api/transcribe', {
          method: 'POST',
          headers: { 'Content-Type': 'audio/webm' },
          body: await blob.arrayBuffer(),
        });
        if (resp.ok) {
          const { text } = await resp.json();
          const inp = document.getElementById('prompt-input');
          if (inp && text && text.trim()) {
            inp.value = (inp.value ? inp.value + ' ' : '') + text.trim();
            inp.focus();
          }
        }
      } catch (err) { console.warn('[mic] transcribe error', err); }
      finally { btn.textContent = '🎤'; btn.disabled = false; }
    };
    micRecorder.start();
    btn.classList.add('recording');
    btn.textContent = '⏹';
  }).catch(err => { alert('Mic access denied: ' + err.message); });
}

function speakerToggle() {
  speakerOn = !speakerOn;
  const btn = document.getElementById('speaker-btn');
  if (btn) {
    btn.classList.toggle('speaker-off', !speakerOn);
    btn.title = speakerOn ? 'Agent voice output: ON' : 'Agent voice output: OFF';
  }
}

function speakText(text) {
  if (!speakerOn || !text || !text.trim()) return;
  fetch('/api/speak', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ text }),
  }).catch(() => {});
}

// Called from app.js event stream — accumulate agent text during a turn
window._voiceOnAgentText = (text) => { agentTurnText += text; };
window._voiceOnAgentDone = () => {
  if (agentTurnText.trim()) speakText(agentTurnText.trim());
  agentTurnText = '';
};

// Wake word triggered: server played ding, now auto-record → transcribe → submit
const WAKE_RECORD_SECS = 7;
let wakeRecordTimer = null;

window.onWakeTriggered = async () => {
  const btn = document.getElementById('mic-btn');
  // Force speaker on for this turn so the response gets spoken
  if (!speakerOn) speakerToggle();
  // Flash the mic button to signal we're listening
  if (btn) { btn.classList.add('recording'); btn.textContent = '⏹'; }
  micServerActive = true;
  try {
    await fetch('/api/record/start', { method: 'POST' });
  } catch (e) { return; }
  // Auto-stop after WAKE_RECORD_SECS
  wakeRecordTimer = setTimeout(async () => {
    wakeRecordTimer = null;
    micServerActive = false;
    if (btn) { btn.classList.remove('recording'); btn.textContent = '⏳'; btn.disabled = true; }
    try {
      const resp = await fetch('/api/record/stop', { method: 'POST' });
      if (resp.ok) {
        const { text } = await resp.json();
        if (text && text.trim()) {
          const inp = document.getElementById('prompt-input');
          if (inp) {
            inp.value = text.trim();
            // Auto-submit — find sendPrompt from app.js scope
            if (typeof sendPrompt === 'function') sendPrompt();
            else inp.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
          }
        }
      }
    } catch (e) { console.warn('[wake] stop error', e); }
    finally { if (btn) { btn.textContent = '🎤'; btn.disabled = false; } }
  }, WAKE_RECORD_SECS * 1000);
};

// ─── Init ─────────────────────────────────────────────────────────────────────
document.addEventListener('DOMContentLoaded', () => {
  // Power modal
  document.getElementById('power-btn')?.addEventListener('click', () => {
    if (typeof showPowerModal === 'function') showPowerModal();
  });
  // Mic + speaker buttons
  document.getElementById('mic-btn')?.addEventListener('click', micToggle);
  document.getElementById('speaker-btn')?.addEventListener('click', speakerToggle);
  // Ctrl+Space → wake (same sequence as wake word, no mic needed)
  document.addEventListener('keydown', (e) => {
    if (e.ctrlKey && e.code === 'Space') {
      e.preventDefault();
      fetch('/api/wake', { method: 'POST' }).catch(() => {});
    }
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
