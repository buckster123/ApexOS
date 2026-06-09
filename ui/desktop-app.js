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
  council: {
    title: '⚗ Council',
    x: 160, y: 70, width: 600, height: 440,
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
  eventlog: {
    title: '📜 Event Log',
    x: 80, y: 60, width: 820, height: 540,
    background: 'var(--wb-bg)',
  },
  mesh: {
    title: '🕸 Mesh',
    x: 100, y: 70, width: 680, height: 480,
    background: 'var(--wb-bg)',
  },
  inference: {
    title: '⚡ Inference',
    x: 120, y: 60, width: 720, height: 560,
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
  if (id === 'eventlog')  setTimeout(eventlogInit, 30);
  if (id === 'mesh')      setTimeout(meshInit, 30);
  if (id === 'inference') setTimeout(inferenceInit, 30);

  const cfg = WIN_DEFAULTS[id] || { title: id, x: 100, y: 80, width: 600, height: 400 };
  wins[id] = new WinBox(cfg.title, {
    ...cfg,
    class: `wb-apexos wb-${id}`,
    mount: content,
    onclose() {
      content.style.display = 'none';
      if (id === 'home')      homeStop();
      if (id === 'eventlog')  eventlogStop();
      if (id === 'mesh')      meshStop();
      if (id === 'inference') inferenceStop();
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
  setTimeout(() => { try { termFit.fit(); } catch(e) {} }, 80);
  setTimeout(() => { termConnectWs(); }, 100);
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
    // Keys tab
    antKey: '', antKeySet: false, antKeySaving: false, antKeySaved: false,
    oaiKey: '', oaiKeySet: false, oaiKeySaving: false, oaiKeySaved: false,

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

      // Keys status
      try {
        const r = await fetch('/api/keys');
        const d = await r.json();
        this.antKeySet = !!d.anthropic_set;
        this.oaiKeySet = !!d.oai_set;
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

    async saveAntKey() {
      if (!this.antKey) return;
      this.antKeySaving = true; this.antKeySaved = false;
      try {
        const r = await fetch('/api/keys', {
          method: 'POST',
          headers: { 'content-type': 'application/json' },
          body: JSON.stringify({ anthropic: this.antKey }),
        });
        const d = await r.json();
        if (d.ok) { this.antKeySet = true; this.antKeySaved = true; this.antKey = ''; setTimeout(() => this.antKeySaved = false, 3000); }
      } catch {}
      this.antKeySaving = false;
    },

    async saveOaiKey() {
      if (!this.oaiKey) return;
      this.oaiKeySaving = true; this.oaiKeySaved = false;
      try {
        const r = await fetch('/api/keys', {
          method: 'POST',
          headers: { 'content-type': 'application/json' },
          body: JSON.stringify({ oai: this.oaiKey }),
        });
        const d = await r.json();
        if (d.ok) { this.oaiKeySet = true; this.oaiKeySaved = true; this.oaiKey = ''; setTimeout(() => this.oaiKeySaved = false, 3000); }
      } catch {}
      this.oaiKeySaving = false;
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

// Backend selector is initialized by app.js initBackendSelector() via checkAndMaybePromptKey.

// ─── Init ─────────────────────────────────────────────────────────────────────
document.addEventListener('DOMContentLoaded', () => {
  // initBackendSelector called via app.js checkAndMaybePromptKey
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

// ─── Council launcher (static window → POST /api/council) ─────────────────

window.conveneCouncil = async function() {
  const topicEl = document.getElementById('cl-topic');
  const topic = topicEl ? topicEl.value.trim() : '';
  if (!topic) { topicEl && topicEl.focus(); return; }

  const checked = [...document.querySelectorAll('input[name="cl-agent"]:checked')];
  const agents  = checked.map(c => c.value);
  if (!agents.length) { alert('Select at least one agent.'); return; }

  const maxRounds = parseInt(document.getElementById('cl-rounds')?.value || '3') || 3;
  const threshold = parseFloat(document.getElementById('cl-threshold')?.value || '0.7') || 0.7;

  const btn    = document.getElementById('cl-convene-btn');
  const status = document.getElementById('cl-status');
  if (btn) { btn.disabled = true; btn.textContent = '⟳ Convening...'; }
  if (status) status.textContent = '';

  try {
    const r = await fetch('/api/council', {
      method:  'POST',
      headers: {'Content-Type': 'application/json'},
      body:    JSON.stringify({ topic, agents, max_rounds: maxRounds, consensus_threshold: threshold }),
    });
    const d = await r.json();
    if (d.council_id) {
      if (status) status.textContent = 'Council ' + d.council_id + ' started — watch for new window';
    } else {
      if (status) status.textContent = d.error || 'Error starting council';
    }
  } catch(e) {
    if (status) status.textContent = 'Network error: ' + e.message;
  }

  if (btn) {
    setTimeout(() => {
      btn.disabled = false;
      btn.textContent = '⚗ CONVENE';
      if (status) status.textContent = '';
      if (topicEl) topicEl.value = '';
    }, 4000);
  }
};

// ─── Council Chamber (dynamic WinBox per council session) ─────────────────

window.openCouncilWin = function(ev) {
  const cid    = ev.council_id;
  const topic  = ev.topic || 'Council';
  const agents = ev.agents || [];
  const winId  = 'council-' + cid;
  if (wins[winId]) { wins[winId].focus(); return; }

  const container = document.createElement('div');
  container.id        = 'win-' + winId + '-content';
  container.className = 'win-content council-win';
  container.dataset.councilId = cid;

  // Header: topic + round counter + convergence bar
  const header = document.createElement('div');
  header.className = 'council-header';
  header.innerHTML =
    '<div class="council-meta">' +
      '<span class="council-topic">' + topic.slice(0, 80) + '</span>' +
      '<span class="council-round" id="cround-' + cid + '">Round 0</span>' +
    '</div>' +
    '<div class="council-conv-wrap">' +
      '<div class="council-conv-bar" id="cconv-' + cid + '" style="width:0%"></div>' +
    '</div>' +
    '<span class="council-conv-label" id="cconv-label-' + cid + '">convergence 0%</span>';

  // Agent columns
  const columns = document.createElement('div');
  columns.className = 'council-columns';
  columns.id = 'ccols-' + cid;

  const colCount = agents.length || 1;
  agents.forEach(function(agent) {
    const col = document.createElement('div');
    col.className = 'council-col';
    col.id = 'ccol-' + cid + '-' + agent.id;
    const color = agent.color || '#888888';
    const modelShort = (agent.model || '').split('/').pop().slice(0, 22);
    col.innerHTML =
      '<div class="col-hdr">' +
        '<span class="col-name" style="color:' + color + '">' + agent.id + '</span>' +
        '<span class="col-model">' + modelShort + '</span>' +
        '<span class="col-status" id="cstatus-' + cid + '-' + agent.id + '">⟳</span>' +
      '</div>' +
      '<div class="col-text" id="ctext-' + cid + '-' + agent.id + '"></div>';
    columns.appendChild(col);
  });

  // Footer: synthesis + butt-in input
  const footer = document.createElement('div');
  footer.className = 'council-footer';
  footer.innerHTML =
    '<div class="council-synthesis" id="csynth-' + cid + '" style="display:none"></div>' +
    '<div class="council-butt-row">' +
      '<input type="text" class="council-butt-input" id="cbutt-' + cid + '" placeholder="Butt in to the deliberation..." onkeydown="if(event.key===\'Enter\')councilButtIn(\'' + cid + '\')">' +
      '<button class="council-butt-btn" onclick="councilButtIn(\'' + cid + '\')">SEND</button>' +
    '</div>';

  container.append(header, columns, footer);
  document.body.appendChild(container);

  const titleShort = topic.slice(0, 36) + (topic.length > 36 ? '…' : '');
  const winW = Math.min(1400, Math.max(560, colCount * 280 + 40));

  wins[winId] = new WinBox('⚗ ' + titleShort, {
    x: 80, y: 56, width: winW, height: 580,
    background: 'var(--wb-bg)',
    class: 'wb-apexos wb-council',
    mount: container,
    onclose: function() {
      container.remove();
      delete wins[winId];
      removeTaskbarTab(winId);
      return false;
    },
  });
  addTaskbarTab(winId, '⚗ ' + titleShort.slice(0, 16), wins[winId]);
};

window.onCouncilStarted = function(ev) {
  if (typeof window.openCouncilWin === 'function') window.openCouncilWin(ev);
};

window.onCouncilRoundStart = function(ev) {
  const cid = ev.council_id;
  const roundEl = document.getElementById('cround-' + cid);
  if (roundEl) roundEl.textContent = 'Round ' + ev.round;
  // Add round separator to each agent column (skip on round 1 — columns are empty)
  if (ev.round > 1) {
    const cols = document.getElementById('ccols-' + cid);
    if (cols) cols.querySelectorAll('.col-text').forEach(function(t) {
      const sep = document.createElement('div');
      sep.className = 'col-round-sep';
      sep.textContent = '── Round ' + ev.round + ' ──';
      t.appendChild(sep);
    });
  }
  // Reset agent statuses to "thinking"
  const cols2 = document.getElementById('ccols-' + cid);
  if (cols2) cols2.querySelectorAll('.col-status').forEach(function(s) {
    s.textContent = '⟳';
    s.style.color = 'var(--accent2)';
  });
};

window.onCouncilAgentDelta = function(ev) {
  const el = document.getElementById('ctext-' + ev.council_id + '-' + ev.agent_id);
  if (!el) return;
  el.textContent += ev.delta;
  el.scrollTop = el.scrollHeight;
};

window.onCouncilAgentDone = function(ev) {
  const el = document.getElementById('cstatus-' + ev.council_id + '-' + ev.agent_id);
  if (el) { el.textContent = '✓'; el.style.color = 'var(--accent)'; }
};

window.onCouncilRoundDone = function(ev) {
  const cid = ev.council_id;
  const pct  = Math.round((ev.convergence || 0) * 100);
  const bar  = document.getElementById('cconv-' + cid);
  const lbl  = document.getElementById('cconv-label-' + cid);
  if (bar) {
    bar.style.width      = pct + '%';
    bar.style.background = pct >= 70 ? 'var(--accent)' : 'var(--accent2)';
  }
  if (lbl) lbl.textContent = 'convergence ' + pct + '%';
};

window.onCouncilComplete = function(ev) {
  const cid = ev.council_id;
  const synthEl = document.getElementById('csynth-' + cid);
  if (synthEl) {
    synthEl.style.display = '';
    synthEl.innerHTML =
      '<strong>SYNTHESIS</strong> <span class="csynth-meta">[' + ev.reason + ', ' + ev.rounds + ' round' + (ev.rounds !== 1 ? 's' : '') + ']</span><br>' +
      ev.synthesis;
  }
  const roundEl = document.getElementById('cround-' + cid);
  if (roundEl) { roundEl.textContent = 'Complete'; roundEl.style.color = 'var(--accent)'; }
  // Disable butt-in input
  const buttEl = document.getElementById('cbutt-' + cid);
  if (buttEl) { buttEl.disabled = true; buttEl.placeholder = 'Council complete'; }
};

window.councilButtIn = async function(cid) {
  const input = document.getElementById('cbutt-' + cid);
  if (!input) return;
  const msg = input.value.trim();
  if (!msg) return;
  input.value = '';
  try {
    const r = await fetch('/api/council/' + cid + '/butt-in', {
      method:  'POST',
      headers: {'Content-Type': 'application/json'},
      body:    JSON.stringify({message: msg}),
    });
    const d = await r.json();
    if (!d.ok) console.warn('[council] butt-in error:', d.error);
  } catch(e) { console.error('[council] butt-in failed:', e); }
};

// ─── Event Log Timeline ───────────────────────────────────────────────────────

var _elAutoTimer = null;

const EL_BADGE = {
  user_prompt:            { label: 'QUERY',  color: '#39ff14' },
  tool_requested:         { label: 'TOOL',   color: '#4fc3f7' },
  approval_pending:       { label: 'APPRVL', color: '#ffd700' },
  user_approval:          { label: 'APPRV',  color: '#ffd700' },
  evolution_proposed:     { label: 'EVO',    color: '#e8b4ff' },
  evolution_applied:      { label: 'EVO+',   color: '#9b59b6' },
  evolution_rolled_back:  { label: 'ROLLBK', color: '#e74c3c' },
  plugin_up:              { label: 'PLUG+',  color: '#2ecc71' },
  plugin_down:            { label: 'PLUG-',  color: '#e74c3c' },
  wake_triggered:         { label: 'WAKE',   color: '#ffb300' },
  spawn_agent:            { label: 'SPAWN',  color: '#4fc3f7' },
  sub_agent_started:      { label: 'AGENT',  color: '#4fc3f7' },
  agent_message:          { label: 'A2A',    color: '#3498db' },
  agent_message_ack:      { label: 'A2AACK', color: '#3498db' },
  council_started:        { label: 'CNCL',   color: '#ffd700' },
  council_complete:       { label: 'CNCL+',  color: '#ffd700' },
  sensor_reading:         { label: 'SENSOR', color: '#ff9800' },
  error:                  { label: 'ERR',    color: '#e74c3c' },
};

function elFormatEvent(ev) {
  const t = ev.type || 'unknown';
  const badge = EL_BADGE[t] || { label: t.toUpperCase().replace(/_/g,' ').slice(0,7), color: '#666' };

  let msg = '';
  switch (t) {
    case 'user_prompt':
      msg = ev.text ? String(ev.text).slice(0, 160) : '';
      break;
    case 'tool_requested':
      if (ev.call) {
        const args = ev.call.args ? JSON.stringify(ev.call.args).slice(0, 100) : '';
        msg = (ev.call.tool || '?') + ': ' + args;
      }
      break;
    case 'approval_pending':
      msg = 'approval needed: ' + (ev.call && ev.call.tool ? ev.call.tool : ev.call_id || '?');
      break;
    case 'user_approval':
      msg = (ev.approved ? '✓ approved' : '✗ denied') + (ev.call_id ? ' call ' + ev.call_id : '');
      break;
    case 'evolution_proposed':
      msg = 'proposed: ' + (ev.proposal ? (ev.proposal.kind || JSON.stringify(ev.proposal).slice(0,80)) : '?');
      break;
    case 'evolution_applied':
      msg = ev.patch_summary || (ev.proposal ? ev.proposal.kind : '?');
      break;
    case 'evolution_rolled_back':
      msg = 'reason: ' + (ev.reason || '?');
      break;
    case 'plugin_up':
      msg = String(ev.plugin || '?') + ' — ' + (ev.tools ? ev.tools.length + ' tools' : 'up');
      break;
    case 'plugin_down':
      msg = String(ev.plugin || '?') + (ev.reason ? ' — ' + ev.reason : '');
      break;
    case 'wake_triggered':
      msg = ev.transcript ? '"' + String(ev.transcript).slice(0,80) + '"' : 'wake word';
      break;
    case 'spawn_agent':
      msg = String(ev.prompt || '?').slice(0, 120);
      break;
    case 'sub_agent_started':
      msg = 'child session ' + (ev.child !== undefined ? ev.child : '?') + ': ' + String(ev.prompt || '').slice(0, 80);
      break;
    case 'agent_message':
      msg = 'session ' + ev.from + ' → ' + ev.to + ': ' + String(ev.body || '').slice(0, 80);
      break;
    case 'council_started':
      msg = 'agents: ' + (Array.isArray(ev.agents) ? ev.agents.map(function(a){ return a.id||a; }).join(', ') : '?');
      break;
    case 'council_complete':
      msg = ev.synthesis ? String(ev.synthesis).slice(0, 120) : 'complete';
      break;
    case 'sensor_reading':
      msg = elFormatSensor(ev);
      break;
    case 'error':
      msg = String(ev.message || '?').slice(0, 120);
      break;
    default:
      msg = JSON.stringify(ev).slice(0, 120);
  }

  const ts = ev.timestamp ? new Date(ev.timestamp * 1000).toLocaleTimeString() : '';
  return { badge: badge, msg: msg, ts: ts };
}

function elFormatSensor(ev) {
  const r = ev.reading || {};
  const k = r.kind || '';
  const node = ev.node_id || '';
  switch (k) {
    case 'air_quality':
      return node + ' IAQ ' + (r.iaq !== undefined ? r.iaq.toFixed(0) : '?') +
             '/acc' + (r.accuracy !== undefined ? r.accuracy : '?') +
             ' T:' + (r.temperature_c !== undefined ? r.temperature_c.toFixed(1) : '?') + '°C' +
             ' RH:' + (r.humidity_pct !== undefined ? r.humidity_pct.toFixed(0) : '?') + '%';
    case 'thermal_frame':
      return node + ' thermal min:' + (r.min_c !== undefined ? r.min_c.toFixed(1) : '?') +
             ' mean:' + (r.mean_c !== undefined ? r.mean_c.toFixed(1) : '?') +
             ' max:' + (r.max_c !== undefined ? r.max_c.toFixed(1) : '?') + '°C';
    case 'temperature':
      return node + ' ' + (r.sensor_id || 'cpu') + ': ' + (r.celsius !== undefined ? r.celsius.toFixed(1) : '?') + '°C';
    case 'motion':
      return node + ' ' + (r.sensor_id || '') + ': motion ' + (r.detected ? 'detected' : 'clear');
    default:
      return node + ' ' + k + ': ' + JSON.stringify(r).slice(0, 80);
  }
}

function eventlogInit() {
  eventlogRefresh();
}

function eventlogStop() {
  if (_elAutoTimer) { clearInterval(_elAutoTimer); _elAutoTimer = null; }
  const cb = document.getElementById('el-auto');
  if (cb) cb.checked = false;
}

function eventlogAutoToggle(on) {
  if (_elAutoTimer) { clearInterval(_elAutoTimer); _elAutoTimer = null; }
  if (on) _elAutoTimer = setInterval(eventlogRefresh, 30000);
}

async function eventlogRefresh() {
  const hoursEl = document.getElementById('el-hours');
  const typeEl  = document.getElementById('el-type');
  const countEl = document.getElementById('el-count');
  const listEl  = document.getElementById('el-events');
  const btn     = document.getElementById('el-refresh-btn');
  if (!listEl) return;

  const hours = hoursEl ? hoursEl.value : '24';
  const types = typeEl  ? typeEl.value  : '';

  if (btn) btn.disabled = true;
  if (countEl) countEl.textContent = '…';

  let events = [];
  try {
    let url = '/api/events/recent?hours=' + hours + '&max=500';
    if (types) url += '&types=' + encodeURIComponent(types);
    const r = await fetch(url);
    if (r.ok) events = await r.json();
  } catch(e) {
    if (countEl) countEl.textContent = 'error';
    if (btn) btn.disabled = false;
    return;
  }

  // Newest first
  events = events.slice().reverse();

  if (countEl) countEl.textContent = events.length + ' event' + (events.length !== 1 ? 's' : '');

  listEl.innerHTML = '';
  if (events.length === 0) {
    listEl.innerHTML = '<div class="el-empty">No events in the last ' + hours + 'h</div>';
    if (btn) btn.disabled = false;
    return;
  }

  const frag = document.createDocumentFragment();
  events.forEach(function(ev) {
    const fmt = elFormatEvent(ev);
    const row = document.createElement('div');
    row.className = 'el-row';

    const badge = document.createElement('span');
    badge.className = 'el-badge';
    badge.textContent = fmt.badge.label;
    badge.style.color = fmt.badge.color;
    badge.style.borderColor = fmt.badge.color + '44';

    const msg = document.createElement('span');
    msg.className = 'el-msg';
    msg.textContent = fmt.msg;

    row.appendChild(badge);
    row.appendChild(msg);

    if (fmt.ts) {
      const ts = document.createElement('span');
      ts.className = 'el-ts';
      ts.textContent = fmt.ts;
      row.appendChild(ts);
    }

    frag.appendChild(row);
  });
  listEl.appendChild(frag);

  if (btn) btn.disabled = false;
}

// ─── Mesh panel ───────────────────────────────────────────────────────────────

let meshAutoTimer = null;

function meshInit() {
  // Show self node ID in toolbar
  fetch('/api/mesh/peers')
    .then(r => r.json())
    .catch(() => null)
    .then(() => {
      // node_id comes from hostname; we can read it from /api/mesh/nodes self-filter absence
      // Just label it with the hostname shown in the URL
      const selfEl = document.getElementById('mesh-node-id');
      if (selfEl) selfEl.textContent = '● ' + location.hostname;
    });

  meshRefresh();
  if (document.getElementById('mesh-auto')?.checked) {
    meshAutoTimer = setInterval(meshRefresh, 30000);
  }
}

function meshStop() {
  clearInterval(meshAutoTimer);
  meshAutoTimer = null;
}

function meshAutoToggle(on) {
  clearInterval(meshAutoTimer);
  meshAutoTimer = on ? setInterval(meshRefresh, 30000) : null;
}

async function meshRefresh() {
  await Promise.all([meshLoadPeers(), meshLoadDiscovered()]);
}

async function meshLoadPeers() {
  const el = document.getElementById('mesh-peers');
  if (!el) return;
  try {
    const data = await fetch('/api/mesh/peers').then(r => r.json());
    const peers = data.peers || [];
    if (!peers.length) {
      el.innerHTML = '<div style="padding:14px 12px;font-family:var(--mono);font-size:11px;color:var(--text-dim)">No peers registered. Use ⊕ Bootstrap or register via agent.</div>';
      return;
    }
    el.innerHTML = '';
    peers.forEach(p => {
      const row = document.createElement('div');
      row.className = 'mesh-peer-row';
      const httpUrl = (p.ws_url || '').replace('ws://', 'http://').replace('wss://', 'https://');
      row.innerHTML = `
        <span class="mesh-peer-id">${p.node_id}</span>
        <span class="mesh-peer-url">${p.ws_url || ''}</span>
        <span class="mesh-peer-role">${p.role || 'full'}</span>
        <span class="mesh-peer-status-${p.status === 'online' ? 'online' : 'offline'}">${p.status || 'online'}</span>
        <button class="mesh-peer-btn" onclick="window.open('${httpUrl}','_blank')" title="Open UI">↗</button>
        <button class="mesh-peer-btn" onclick="meshSendMsg('${p.node_id}')" title="Send message">✉</button>
        <button class="mesh-peer-btn mesh-peer-btn-danger" onclick="meshRemovePeer('${p.node_id}')" title="Remove">✕</button>
      `;
      el.appendChild(row);
    });
  } catch(e) {
    el.innerHTML = `<div style="padding:10px 12px;font-family:var(--mono);font-size:11px;color:#f44336">Error loading peers: ${e.message}</div>`;
  }
}

async function meshLoadDiscovered() {
  const listEl  = document.getElementById('mesh-discovered');
  const titleEl = document.getElementById('mesh-disc-title');
  if (!listEl || !titleEl) return;
  try {
    const data = await fetch('/api/mesh/nodes').then(r => r.json());
    const nodes = (data.nodes || []).filter(n => !n.known);
    if (!nodes.length) {
      titleEl.style.display = 'none';
      listEl.innerHTML = '';
      return;
    }
    titleEl.style.display = '';
    listEl.innerHTML = '';
    nodes.forEach(n => {
      const row = document.createElement('div');
      row.className = 'mesh-peer-row';
      const wsUrl = n.ws_url || `ws://${n.ip}:8787`;
      const escapedWsUrl = wsUrl.replace(/'/g, "\\'");
      row.innerHTML = `
        <span class="mesh-peer-id">${n.node_id}</span>
        <span class="mesh-peer-url">${n.ip}:${n.port || 8787}</span>
        <button class="mesh-peer-btn" onclick="meshRegisterDiscovered('${n.node_id}','${escapedWsUrl}')">+ Register</button>
      `;
      listEl.appendChild(row);
    });
  } catch(e) {
    titleEl.style.display = 'none';
  }
}

async function meshScan() {
  const btn = document.querySelector('.mesh-btn');
  if (btn) { btn.textContent = '⟳ Scanning…'; btn.disabled = true; }
  await meshRefresh();
  if (btn) { btn.textContent = '⟳ Scan'; btn.disabled = false; }
}

async function meshRegisterDiscovered(nodeId, wsUrl) {
  try {
    await fetch('/api/mesh/peers', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ node_id: nodeId, ws_url: wsUrl, role: 'full', status: 'online' }),
    });
    meshRefresh();
  } catch(e) {
    alert('Register failed: ' + e.message);
  }
}

async function meshRemovePeer(nodeId) {
  if (!confirm(`Remove peer ${nodeId}?`)) return;
  try {
    await fetch(`/api/mesh/peers/${nodeId}`, { method: 'DELETE' });
    meshRefresh();
  } catch(e) {
    alert('Remove failed: ' + e.message);
  }
}

async function meshSendMsg(nodeId) {
  const msg = prompt(`Message to send to ${nodeId} (root session):`);
  if (!msg) return;
  // Use the agent to call send_to_agent with node: field
  const txt = `send_to_agent to node ${nodeId}: ${msg}`;
  // Inject into agent input as a shortcut
  const input = document.getElementById('prompt-input');
  if (input) {
    input.value = `Use send_to_agent(session_id: 0, node: "${nodeId}", message: "${msg.replace(/"/g,'\\\"')}")`;
    input.focus();
  }
}

function meshBootstrapOpen() {
  const modal = document.getElementById('mesh-bootstrap-modal');
  if (modal) modal.style.display = '';
}

function meshBootstrapClose() {
  const modal = document.getElementById('mesh-bootstrap-modal');
  if (modal) modal.style.display = 'none';
  const status = document.getElementById('mesh-bs-status');
  if (status) status.textContent = '';
}

async function meshBootstrapRun() {
  const ip   = document.getElementById('mesh-bs-ip')?.value.trim();
  const user = document.getElementById('mesh-bs-user')?.value.trim() || 'apexos';
  const pass = document.getElementById('mesh-bs-pass')?.value;
  const key  = document.getElementById('mesh-bs-key')?.value.trim();
  const status = document.getElementById('mesh-bs-status');

  if (!ip || !pass) { if (status) status.textContent = 'IP and SSH password are required.'; return; }

  if (status) status.textContent = '⏳ Bootstrapping — injecting into agent…';

  // Build a natural language message that triggers the bootstrap_node virtual tool
  const parts = [`bootstrap_node(target_ip: "${ip}", ssh_user: "${user}", ssh_password: "${pass}"`];
  if (key) parts.push(`, api_key: "${key}"`);
  parts.push(')');
  const agentMsg = 'Please run: ' + parts.join('');

  const inputEl = document.getElementById('prompt-input');
  if (inputEl) {
    inputEl.value = agentMsg;
    inputEl.focus();
    if (status) status.textContent = '✓ Message ready in agent input — press Enter to send.';
  } else {
    if (status) status.textContent = 'Open the Agent window first, then try again.';
  }
}

// ─── Inference (Vast.ai) panel ────────────────────────────────────────────────

let inferenceTimer     = null;
let inferenceRecipes   = [];
let inferenceSelected  = null;   // currently selected recipe name

function inferenceInit() {
  inferenceRefresh();
  inferenceLoadRecipes();
  inferenceTimer = setInterval(inferenceRefresh, 8000);
}

function inferenceStop() {
  clearInterval(inferenceTimer);
  inferenceTimer = null;
}

async function inferenceRefresh() {
  const r = await fetch('/api/vast/status').catch(() => null);
  if (!r || !r.ok) return;
  const data = await r.json();

  const statusBadge  = document.getElementById('inf-status-badge');
  const activePanel  = document.getElementById('inf-active-panel');
  const launchPanel  = document.getElementById('inf-launch-panel');

  if (statusBadge) {
    const s = data.status;
    statusBadge.textContent = s;
    statusBadge.className = 'inf-badge inf-badge-' + s;
    if (s === 'launching' && data.launch_phase) {
      statusBadge.textContent = 'launching: ' + data.launch_phase;
    }
  }

  if (data.instance && activePanel) {
    activePanel.style.display = '';
    const i = data.instance;
    document.getElementById('inf-instance-id')?.setAttribute('data-value', i.id);
    const el = (id, val) => { const e = document.getElementById(id); if (e) e.textContent = val; };
    el('inf-inst-id',      i.id);
    el('inf-inst-recipe',  i.recipe);
    el('inf-inst-port',    i.local_port);
    el('inf-inst-cost',    '$' + (i.cost_per_hr || 0).toFixed(3) + '/hr');
    el('inf-inst-launched', i.launched_at ? i.launched_at.slice(0,19).replace('T',' ') : '');
    // Compute running cost
    if (i.launched_at && i.cost_per_hr) {
      const elapsed = (Date.now() - new Date(i.launched_at).getTime()) / 3600000;
      el('inf-inst-spent', '$' + (elapsed * i.cost_per_hr).toFixed(3));
    }
  } else if (activePanel) {
    activePanel.style.display = 'none';
  }

  if (launchPanel) {
    launchPanel.style.display = data.status === 'idle' ? '' : 'none';
  }
}

async function inferenceLoadRecipes() {
  const r = await fetch('/api/vast/recipes').catch(() => null);
  if (!r || !r.ok) return;
  const data = await r.json();
  inferenceRecipes = data.recipes || [];
  inferenceRenderRecipes();
}

function inferenceRenderRecipes() {
  const list = document.getElementById('inf-recipe-list');
  if (!list) return;
  list.innerHTML = '';

  // Group by GPU tier
  const byGpu = {};
  inferenceRecipes.forEach(r => {
    if (!byGpu[r.gpu]) byGpu[r.gpu] = [];
    byGpu[r.gpu].push(r);
  });

  Object.entries(byGpu).forEach(([gpu, recipes]) => {
    const header = document.createElement('div');
    header.className = 'inf-gpu-header';
    header.textContent = gpu.toUpperCase();
    list.appendChild(header);

    recipes.forEach(recipe => {
      const row = document.createElement('div');
      row.className = 'inf-recipe-row' + (inferenceSelected === recipe.name ? ' selected' : '');
      row.innerHTML = `
        <span class="inf-recipe-label">${recipe.label}</span>
        <span class="inf-recipe-desc">${recipe.description}</span>
        <button class="inf-btn" onclick="inferenceSelectRecipe('${recipe.name}')">Select</button>
      `;
      list.appendChild(row);
    });
  });
}

function inferenceSelectRecipe(name) {
  inferenceSelected = name;
  const sel = document.getElementById('inf-launch-recipe');
  if (sel) sel.value = name;
  inferenceRenderRecipes();
}

async function inferenceLaunch() {
  const recipe = document.getElementById('inf-launch-recipe')?.value || inferenceSelected;
  const geo    = document.getElementById('inf-launch-geo')?.value || 'EU_NORDIC';
  if (!recipe) { alert('Select a recipe first'); return; }

  const btn = document.getElementById('inf-launch-btn');
  if (btn) { btn.disabled = true; btn.textContent = '⏳ Launching…'; }

  const status = document.getElementById('inf-launch-status');
  if (status) status.textContent = 'Sending launch request to agent…';

  // Inject into agent input so user sees it
  const msg = `vast_launch — recipe: ${recipe}, geo: ${geo}`;
  const inputEl = document.getElementById('prompt-input');
  if (inputEl) {
    inputEl.value = `Please run vast_launch with recipe="${recipe}" and geo="${geo}"`;
    if (status) status.textContent = '✓ Message ready in agent input — press Enter to send.';
  } else {
    if (status) status.textContent = 'Open Agent window first.';
  }
  if (btn) { btn.disabled = false; btn.textContent = '⚡ Launch'; }
}

async function inferenceDestroy() {
  if (!confirm('Destroy the active Vast.ai instance? Billing stops immediately.')) return;
  const btn = document.getElementById('inf-destroy-btn');
  if (btn) { btn.disabled = true; btn.textContent = '⏳ Destroying…'; }
  const inputEl = document.getElementById('prompt-input');
  if (inputEl) {
    inputEl.value = 'Please run vast_destroy to shut down the Vast.ai instance.';
  }
  if (btn) { btn.disabled = false; btn.textContent = '🗑 Destroy Instance'; }
}

async function inferenceLoadOffers() {
  const gpu = document.getElementById('inf-builder-gpu')?.value || '';
  const geo = document.getElementById('inf-builder-geo')?.value || 'EU_NORDIC';
  const offerEl = document.getElementById('inf-offers-list');
  if (offerEl) offerEl.textContent = 'Loading…';
  const r = await fetch(`/api/vast/offers?gpu=${encodeURIComponent(gpu)}&geo=${encodeURIComponent(geo)}`).catch(() => null);
  if (!r || !r.ok) { if (offerEl) offerEl.textContent = 'Error loading offers.'; return; }
  const offers = await r.json();
  if (!offerEl) return;
  if (!offers.length) { offerEl.textContent = 'No offers found.'; return; }
  offerEl.innerHTML = offers.slice(0,8).map(o =>
    `<div class="inf-offer-row">
      <span>${o.gpu_name}</span>
      <span>$${(o.dph_total||0).toFixed(3)}/hr</span>
      <span>${o.geolocation||''}</span>
      <span>rel=${(o.reliability||0).toFixed(3)}</span>
      <span>↓${Math.round(o.inet_down||0)} Mbps</span>
    </div>`
  ).join('');
}

async function inferenceHfSearch() {
  const q    = document.getElementById('inf-hf-search')?.value || '';
  const el   = document.getElementById('inf-hf-results');
  if (!q) return;
  if (el) el.textContent = 'Searching…';
  const r = await fetch(`/api/vast/hf-search?q=${encodeURIComponent(q)}`).catch(() => null);
  if (!r || !r.ok) { if (el) el.textContent = 'Error.'; return; }
  const models = await r.json();
  if (!el) return;
  el.innerHTML = models.slice(0,10).map(m =>
    `<div class="inf-hf-row" onclick="inferenceSetModel('${m.id}')">
      <span class="inf-hf-id">${m.id}</span>
      <span class="inf-hf-dl">↓${(m.downloads||0).toLocaleString()}</span>
    </div>`
  ).join('');
}

function inferenceSetModel(id) {
  const el = document.getElementById('inf-builder-model');
  if (el) el.value = id;
}
