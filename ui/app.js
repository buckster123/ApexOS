// ─── Config ──────────────────────────────────────────────────────────────────
const WS_URL     = `ws://${location.host}/ws`;
const SESSION_ID = Math.floor(Math.random() * 2 ** 32);
const RECONNECT_DELAYS = [500, 1000, 2000, 4000, 8000];

// ─── State ───────────────────────────────────────────────────────────────────
let ws           = null;
let reconnectIdx = 0;
let reconnTimer  = null;
let activeTurn   = null;   // { turnEl, agentBlock, cursor }
let bootDone     = false;

// ─── Boot sequence ────────────────────────────────────────────────────────────
const LOGO = [
  ' █████╗ ██████╗ ███████╗██╗  ██╗ ██████╗ ███████╗',
  '██╔══██╗██╔══██╗██╔════╝╚██╗██╔╝██╔═══██╗██╔════╝',
  '███████║██████╔╝█████╗   ╚███╔╝ ██║   ██║███████╗',
  '██╔══██║██╔═══╝ ██╔══╝   ██╔██╗ ██║   ██║╚════██║',
  '██║  ██║██║     ███████╗██╔╝ ██╗╚██████╔╝███████║',
  '╚═╝  ╚═╝╚═╝     ╚══════╝╚═╝  ╚═╝ ╚═════╝ ╚══════╝',
  '',
  '        agent-first OS daemon  ·  pi 5  ·  v0.1',
];

const sleep = ms => new Promise(r => setTimeout(r, ms));

async function runBoot() {
  const logo  = document.getElementById('boot-logo');
  const lines = document.getElementById('boot-lines');

  logo.textContent = LOGO.join('\n');
  await sleep(40);
  logo.classList.add('visible');
  await sleep(500);

  const staticLines = [
    ['PLATFORM',          'RASPBERRY PI 5  AARCH64',       'ok'],
    ['EVENT BUS',         'BROADCAST CHANNEL OPEN',         'ok'],
    ['POLICY ENGINE',     'MODE: SUGGEST',                   'ok'],
    ['PLUGIN SUPERVISOR', 'STARTING MCP PLUGINS...',        'dot'],
    ['GATEWAY',           `WS → ${WS_URL}`,                 'dot'],
  ];

  for (const [lbl, msg, type] of staticLines) {
    await sleep(70);
    addBootLine(lines, lbl, msg, type);
  }
}

function addBootLine(container, label, msg, type) {
  const div = document.createElement('div');
  div.className = 'boot-line';
  const badge = type === 'ok' ? '[ OK ]' : type === 'err' ? '[ERR ]' : '[    ]';
  const cls   = type === 'ok' ? 'bl-ok'  : type === 'err' ? 'bl-err'  : 'bl-dot';
  const padded = label.padEnd(20);
  div.innerHTML =
    `<span class="bl-msg">${padded}  ${esc(msg)}</span>` +
    `<span class="${cls}">${badge}</span>`;
  container.appendChild(div);
  return div;
}

function updateBootLine(container, labelPrefix, newType, newMsg) {
  for (const div of container.children) {
    const msgEl = div.querySelector('.bl-msg');
    if (!msgEl || !msgEl.textContent.trimStart().startsWith(labelPrefix)) continue;
    const badge = newType === 'ok' ? '[ OK ]' : newType === 'err' ? '[ERR ]' : '[    ]';
    const cls   = newType === 'ok' ? 'bl-ok'  : newType === 'err' ? 'bl-err'  : 'bl-dot';
    const badgeEl = div.querySelector('[class^="bl-"]');
    if (badgeEl) { badgeEl.className = cls; badgeEl.textContent = badge; }
    if (newMsg) {
      const label = msgEl.textContent.slice(0, 22);
      msgEl.textContent = label + '  ' + newMsg;
    }
    return;
  }
}

async function transitionToApp() {
  if (bootDone) return;
  bootDone = true;

  const lines = document.getElementById('boot-lines');
  await sleep(80);
  addBootLine(lines, 'ALL SYSTEMS', 'NOMINAL', 'ok');
  await sleep(700);

  document.getElementById('boot').classList.add('hidden');
  document.getElementById('app').classList.remove('hidden');

  if (ws?.readyState === WebSocket.OPEN) enableInput(true);
}

// ─── WebSocket ────────────────────────────────────────────────────────────────
function connect() {
  setStatus('warn', 'CONNECTING');
  ws = new WebSocket(WS_URL);

  ws.onopen = () => {
    reconnectIdx = 0;
    setStatus('ok', 'CONNECTED');
    updateBootLine(document.getElementById('boot-lines'), 'GATEWAY', 'ok');
    if (bootDone) {
      enableInput(true);
      showSysMsg('reconnected');
    }
  };

  ws.onmessage = e => {
    try { handleEvent(JSON.parse(e.data)); }
    catch { /* ignore malformed */ }
  };

  ws.onclose = ws.onerror = () => {
    setStatus('err', 'DISCONNECTED');
    enableInput(false);
    if (bootDone) {
      onTurnComplete();
      showSysMsg('connection lost — reconnecting...');
    }
    scheduleReconnect();
  };
}

function scheduleReconnect() {
  if (reconnTimer) return;
  const delay = RECONNECT_DELAYS[Math.min(reconnectIdx, RECONNECT_DELAYS.length - 1)];
  reconnectIdx++;
  reconnTimer = setTimeout(() => { reconnTimer = null; connect(); }, delay);
}

function sendWs(obj) {
  if (ws?.readyState === WebSocket.OPEN) ws.send(JSON.stringify(obj));
}

// ─── Event dispatch ───────────────────────────────────────────────────────────
function handleEvent(ev) {
  // Session-specific events: only handle events for our session.
  // Plugin events (no session field) always pass through.
  if (ev.session !== undefined && ev.session !== SESSION_ID) return;

  switch (ev.type) {
    case 'agent_text':       onAgentText(ev);       break;
    case 'turn_complete':    onTurnComplete();       break;
    case 'tool_requested':   onToolRequested(ev);   break;
    case 'tool_result':      onToolResult(ev);      break;
    case 'approval_pending': onApprovalPending(ev); break;
    case 'plugin_up':        onPluginUp(ev);        break;
    case 'plugin_down':      onPluginDown(ev);      break;
  }
}

// ─── Agent text ───────────────────────────────────────────────────────────────
function ensureActiveTurn() {
  if (activeTurn) return activeTurn;

  const turnEl     = document.createElement('div');
  turnEl.className = 'turn';
  const agentBlock = document.createElement('div');
  agentBlock.className = 'agent-block';
  const cursor = document.createElement('span');
  cursor.className = 'cursor';
  agentBlock.appendChild(cursor);
  turnEl.appendChild(agentBlock);
  document.getElementById('output').appendChild(turnEl);

  activeTurn = { turnEl, agentBlock, cursor };
  return activeTurn;
}

function onAgentText(ev) {
  const t = ensureActiveTurn();
  t.cursor.insertAdjacentText('beforebegin', ev.delta);
  scrollDown();
}

function onTurnComplete() {
  if (activeTurn) {
    activeTurn.cursor.remove();
    activeTurn = null;
  }
  if (bootDone && ws?.readyState === WebSocket.OPEN) enableInput(true);
  scrollDown();
}

// ─── Tool calls ───────────────────────────────────────────────────────────────
function onToolRequested(ev) {
  const t   = ensureActiveTurn();
  const div = makeToolCallEl(ev.call.id, ev.call.tool, ev.call.input);
  // Append to the turn (sibling of agentBlock) so it appears inline in flow.
  t.turnEl.appendChild(div);
  scrollDown();
}

function makeToolCallEl(callId, toolName, input) {
  const wrap = document.createElement('div');
  wrap.className  = 'tool-call';
  wrap.dataset.id = String(callId);

  const header = document.createElement('div');
  header.className = 'tool-header';

  const argsStr = JSON.stringify(input, null, 2);

  header.innerHTML =
    `<span class="tool-tag">TOOL</span>` +
    `<span class="tool-name">${esc(toolName)}</span>` +
    `<span class="tool-status" data-status="pending">◌</span>` +
    `<span class="tool-toggle">▸</span>`;

  const body = document.createElement('div');
  body.className = 'tool-body';

  const argsDiv = document.createElement('div');
  argsDiv.className  = 'tool-args';
  argsDiv.textContent = argsStr;
  body.appendChild(argsDiv);

  header.addEventListener('click', () => {
    const open = body.classList.toggle('open');
    header.querySelector('.tool-toggle').textContent = open ? '▾' : '▸';
  });

  wrap.appendChild(header);
  wrap.appendChild(body);
  return wrap;
}

function onToolResult(ev) {
  const callEl = document.querySelector(`.tool-call[data-id="${ev.call}"]`);
  if (!callEl) return;

  // Update status indicator
  const statusEl = callEl.querySelector('.tool-status');
  if (statusEl) {
    statusEl.className = `tool-status ${ev.output.ok ? 'ok' : 'err'}`;
    statusEl.textContent = ev.output.ok ? '✓' : '✗';
  }

  // Append result to body and auto-expand
  const body = callEl.querySelector('.tool-body');
  if (!body) return;

  const resultEl = document.createElement('div');
  resultEl.className = `tool-result${ev.output.ok ? '' : ' err'}`;
  const content = ev.output.content;
  const text = typeof content === 'string' ? content : JSON.stringify(content, null, 2);
  // Truncate very long results; user can expand the raw log later
  resultEl.textContent = text.length > 400 ? text.slice(0, 400) + '\n…' : text;
  body.appendChild(resultEl);

  // Auto-open so result is visible
  body.classList.add('open');
  const toggle = callEl.querySelector('.tool-toggle');
  if (toggle) toggle.textContent = '▾';

  scrollDown();
}

// ─── Approval ─────────────────────────────────────────────────────────────────
function onApprovalPending(ev) {
  const block = document.createElement('div');
  block.className = 'approval-block';

  const argsStr = JSON.stringify(ev.call.input, null, 2);
  block.innerHTML =
    `<div class="approval-label">⚠  APPROVAL REQUIRED: <strong>${esc(ev.call.tool)}</strong></div>` +
    `<div class="approval-args">${esc(argsStr)}</div>`;

  const btns    = document.createElement('div');
  btns.className = 'approval-btns';

  const approve = document.createElement('button');
  approve.className   = 'btn-approve';
  approve.textContent = 'APPROVE';
  approve.onclick = () => {
    sendWs({ type: 'user_approval', session: SESSION_ID, action: ev.call.id, granted: true });
    block.replaceWith(approvedBadge(ev.call.tool, true));
  };

  const deny = document.createElement('button');
  deny.className   = 'btn-deny';
  deny.textContent = 'DENY';
  deny.onclick = () => {
    sendWs({ type: 'user_approval', session: SESSION_ID, action: ev.call.id, granted: false });
    block.replaceWith(approvedBadge(ev.call.tool, false));
  };

  btns.appendChild(approve);
  btns.appendChild(deny);
  block.appendChild(btns);

  document.getElementById('output').appendChild(block);
  scrollDown();
}

function approvedBadge(toolName, granted) {
  const d = document.createElement('div');
  d.className = 'sys-msg';
  d.textContent = `${granted ? 'approved' : 'denied'}: ${toolName}`;
  d.style.color = granted ? 'var(--accent)' : 'var(--error)';
  return d;
}

// ─── Plugin status ────────────────────────────────────────────────────────────
const pluginCounts = {};

function onPluginUp(ev) {
  pluginCounts[ev.plugin] = (ev.tools || []).length;
  refreshToolCount();

  const lines = document.getElementById('boot-lines');
  if (!bootDone) {
    updateBootLine(lines, 'PLUGIN SUPERVISOR', 'ok');
    addBootLine(lines, `  ${ev.plugin.toUpperCase()}`,
                `${pluginCounts[ev.plugin]} tools registered`, 'ok');
    // Give a beat for the user to read the boot log, then transition.
    setTimeout(transitionToApp, 1200);
  } else {
    showSysMsg(`plugin ${ev.plugin} online  (${pluginCounts[ev.plugin]} tools)`);
  }
}

function onPluginDown(ev) {
  delete pluginCounts[ev.plugin];
  refreshToolCount();
  if (bootDone) showSysMsg(`plugin ${ev.plugin} offline: ${ev.reason || 'unknown'}`);
}

function refreshToolCount() {
  const total = Object.values(pluginCounts).reduce((a, b) => a + b, 0);
  document.getElementById('hdr-center').textContent = total ? `${total} tools` : '';
}

// ─── Sending prompts ──────────────────────────────────────────────────────────
function sendPrompt() {
  const input = document.getElementById('prompt-input');
  const text  = input.value.trim();
  if (!text || ws?.readyState !== WebSocket.OPEN) return;

  input.value = '';
  enableInput(false);

  // Build turn with user line + empty agent block
  const output     = document.getElementById('output');
  const turnEl     = document.createElement('div');
  turnEl.className = 'turn';

  const userLine = document.createElement('div');
  userLine.className   = 'user-line';
  userLine.textContent = text;
  turnEl.appendChild(userLine);

  const agentBlock = document.createElement('div');
  agentBlock.className = 'agent-block';
  const cursor = document.createElement('span');
  cursor.className = 'cursor';
  agentBlock.appendChild(cursor);
  turnEl.appendChild(agentBlock);

  output.appendChild(turnEl);
  activeTurn = { turnEl, agentBlock, cursor };

  sendWs({ type: 'user_prompt', session: SESSION_ID, text });
  scrollDown();
}

// ─── Utilities ────────────────────────────────────────────────────────────────
function enableInput(on) {
  const inp = document.getElementById('prompt-input');
  const btn = document.getElementById('send-btn');
  inp.disabled = !on;
  btn.disabled = !on;
  if (on) inp.focus();
}

function setStatus(cls, label) {
  document.getElementById('ws-dot').className   = cls;
  document.getElementById('ws-label').textContent = label;
}

function scrollDown() {
  const out = document.getElementById('output');
  out.scrollTop = out.scrollHeight;
}

function showSysMsg(text) {
  const d = document.createElement('div');
  d.className   = 'sys-msg';
  d.textContent = text;
  document.getElementById('output').appendChild(d);
  scrollDown();
}

function esc(str) {
  return String(str)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}

// ─── API key entry ────────────────────────────────────────────────────────────
async function checkAndMaybePromptKey() {
  try {
    const res  = await fetch('/api/status');
    const data = await res.json();
    if (data.api_key_set) return; // all good
  } catch {
    return; // can't reach status endpoint — offline, keep going
  }

  // Key missing — show the form and wait for it to be submitted
  const form   = document.getElementById('key-form');
  const input  = document.getElementById('key-input');
  const submit = document.getElementById('key-submit');
  const errEl  = document.getElementById('key-error');
  const cursor = document.getElementById('boot-cursor');

  cursor.style.display = 'none';
  form.classList.remove('hidden');
  input.focus();

  await new Promise(resolve => {
    async function trySubmit() {
      const key = input.value.trim();
      if (!key) { showKeyError('Key cannot be empty.'); return; }

      submit.disabled   = true;
      submit.textContent = 'SAVING...';
      errEl.classList.add('hidden');

      try {
        const res  = await fetch('/api/key', {
          method:  'POST',
          headers: { 'content-type': 'application/json' },
          body:    JSON.stringify({ key }),
        });
        const data = await res.json();
        if (data.ok) {
          form.classList.add('hidden');
          cursor.style.display = '';
          addBootLine(document.getElementById('boot-lines'),
                      'API KEY', 'ACCEPTED', 'ok');
          await sleep(300);
          resolve();
        } else {
          showKeyError(data.error || 'Failed to save key.');
          submit.disabled   = false;
          submit.textContent = 'CONNECT';
        }
      } catch {
        showKeyError('Could not reach agentd — is it running?');
        submit.disabled   = false;
        submit.textContent = 'CONNECT';
      }
    }

    function showKeyError(msg) {
      errEl.textContent = msg;
      errEl.classList.remove('hidden');
    }

    submit.addEventListener('click', trySubmit);
    input.addEventListener('keydown', e => {
      if (e.key === 'Enter') { e.preventDefault(); trySubmit(); }
    });
  });
}

// ─── Init ─────────────────────────────────────────────────────────────────────
document.addEventListener('DOMContentLoaded', () => {
  document.getElementById('send-btn').addEventListener('click', sendPrompt);
  document.getElementById('prompt-input').addEventListener('keydown', e => {
    if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); sendPrompt(); }
  });

  runBoot()
    .then(checkAndMaybePromptKey)
    .then(() => {
      connect();
      // Failsafe: enter app even if WS never gets a plugin_up
      setTimeout(transitionToApp, 4000);
    });
});
