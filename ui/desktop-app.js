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

  // Terminal: defer init until WinBox has laid out the element
  if (id === 'terminal') setTimeout(initTerminal, 60);

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
