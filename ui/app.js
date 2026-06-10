// ─── Config ──────────────────────────────────────────────────────────────────
const WS_URL     = `ws://${location.host}/ws`;
const RECONNECT_DELAYS = [500, 1000, 2000, 4000, 8000];

// MODELS is populated dynamically from /api/models — no hardcoded list needed.

// ─── State ───────────────────────────────────────────────────────────────────
let SESSION_ID   = null;   // assigned by server via session_init
let ws           = null;
let reconnectIdx = 0;
let reconnTimer  = null;
let activeTurn   = null;   // { turnEl, agentBlock, cursor }
var bootDone     = false;  // var → window.bootDone so desktop-app.js can set it
let sessionTurns = [];     // { ts, user, agent } — saved on new session
let evoCount     = 0;      // evolutions applied since page load
let voluntaryClose = false; // true when newSession() intentionally closes WS

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
    // Send hello to resume prior session or get a fresh server-assigned ID
    const storedId = localStorage.getItem('apexos_session_id');
    const hello = storedId
      ? { type: 'hello', resume_session: Number(storedId) }
      : { type: 'hello' };
    ws.send(JSON.stringify(hello));
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
    if (bootDone && !voluntaryClose) {
      onTurnComplete();
      showSysMsg('connection lost — reconnecting...');
    }
    voluntaryClose = false;
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

// ─── Sub-agent session routing ────────────────────────────────────────────────
// Maps child session_id (number) → { outputEl, statusEl }
const subAgentOutputs = new Map();

window.addWatchedSession = (id, els) => {
  els.toolBlocks = new Map();
  els.textEl     = null;
  subAgentOutputs.set(id, els);
};
window.removeWatchedSession = (id) => subAgentOutputs.delete(id);

// ─── Event dispatch ───────────────────────────────────────────────────────────
function handleEvent(ev) {
  // Drive the desktop face widget (if present) from every bus event, all sessions.
  if (typeof window._faceOnEvent === 'function') window._faceOnEvent(ev);

  // Agent-to-agent messages: route to destination session window (no `session` field).
  if (ev.type === 'agent_message') {
    const msgEl = document.createElement('div');
    msgEl.className = 'subagent-inbox-msg';
    msgEl.textContent = `\u{1F4E8} from Agent ${ev.from}: ${ev.body}`;
    if (subAgentOutputs.has(ev.to)) {
      const entry = subAgentOutputs.get(ev.to);
      entry.outputEl.appendChild(msgEl);
      entry.outputEl.scrollTop = entry.outputEl.scrollHeight;
    } else if (SESSION_ID !== null && ev.to === SESSION_ID) {
      const out = document.getElementById('output');
      if (out) { out.appendChild(msgEl); out.scrollTop = out.scrollHeight; }
    }
    return;
  }
  if (ev.type === 'agent_message_ack') return;

  // null means daemon-scoped; undefined means all sessions.
  // Also pass through events for watched child sessions.
  if (SESSION_ID !== null && ev.session != null &&
      ev.session !== SESSION_ID && !subAgentOutputs.has(ev.session)) return;

  // Sub-agent events route to child window, not main output
  if (ev.session != null && subAgentOutputs.has(ev.session)) {
    const entry = subAgentOutputs.get(ev.session);
    const { outputEl, statusEl } = entry;
    const childSession = ev.session;

    if (ev.type === 'agent_text' && ev.delta) {
      if (!entry.textEl) {
        entry.textEl = document.createElement('div');
        entry.textEl.className = 'subagent-text';
        outputEl.appendChild(entry.textEl);
      }
      entry.textEl.textContent += ev.delta;
      outputEl.scrollTop = outputEl.scrollHeight;
    }

    if (ev.type === 'tool_requested' && ev.call) {
      entry.textEl = null;  // next text after tool call gets a fresh block
      const toolEl = makeToolCallEl(ev.call.id, ev.call.tool, ev.call.input);
      outputEl.appendChild(toolEl);
      entry.toolBlocks.set(String(ev.call.id), toolEl);
      outputEl.scrollTop = outputEl.scrollHeight;
    }

    if (ev.type === 'tool_result') {
      const toolEl = entry.toolBlocks.get(String(ev.call));
      if (toolEl) {
        const statusSpan = toolEl.querySelector('.tool-status');
        if (statusSpan) {
          statusSpan.className  = `tool-status ${ev.output.ok ? 'ok' : 'err'}`;
          statusSpan.textContent = ev.output.ok ? '✓' : '✗';
        }
        const body = toolEl.querySelector('.tool-body');
        if (body) {
          const content = ev.output.content;
          const text = typeof content === 'string' ? content : JSON.stringify(content, null, 2);
          const hdr = toolEl.querySelector('.tool-header');
          const tog = toolEl.querySelector('.tool-toggle');
          if (hdr && tog) {
            const preview = document.createElement('span');
            preview.className = 'tool-result-preview';
            const firstLine = text.split('\n').find(l => l.trim()) || '';
            preview.textContent = firstLine.length > 60 ? firstLine.slice(0, 60) + '…' : firstLine;
            hdr.insertBefore(preview, tog);
          }
          const resultEl = document.createElement('div');
          resultEl.className = `tool-result${ev.output.ok ? '' : ' err'}`;
          resultEl.textContent = text.length > 400 ? text.slice(0, 400) + '\n…' : text;
          body.appendChild(resultEl);
          if (!ev.output.ok) {
            body.classList.add('open');
            if (tog) tog.textContent = '▾';
          }
        }
      }
      outputEl.scrollTop = outputEl.scrollHeight;
    }

    if (ev.type === 'approval_pending' && ev.call) {
      const block = document.createElement('div');
      block.className = 'approval-block';
      const argsStr = JSON.stringify(ev.call.input, null, 2);
      const sum = toolSummary(ev.call.tool, ev.call.input);
      block.innerHTML =
        `<div class="approval-label">⚠ APPROVAL: <strong>${esc(ev.call.tool)}</strong>` +
        (sum ? ` <span class="tool-summary">${esc(sum)}</span>` : '') +
        `</div>` +
        `<details class="approval-detail"><summary>args</summary><div class="approval-args">${esc(argsStr)}</div></details>`;
      const btns = document.createElement('div');
      btns.className = 'approval-btns';
      const approve = document.createElement('button');
      approve.className   = 'btn-approve';
      approve.textContent = 'APPROVE';
      approve.onclick = () => {
        sendWs({ type: 'user_approval', session: childSession, action: ev.call.id, granted: true });
        block.replaceWith(approvedBadge(ev.call.tool, true));
      };
      const deny = document.createElement('button');
      deny.className   = 'btn-deny';
      deny.textContent = 'DENY';
      deny.onclick = () => {
        sendWs({ type: 'user_approval', session: childSession, action: ev.call.id, granted: false });
        block.replaceWith(approvedBadge(ev.call.tool, false));
      };
      btns.append(approve, deny);
      block.appendChild(btns);
      outputEl.appendChild(block);
      outputEl.scrollTop = outputEl.scrollHeight;
    }

    if (ev.type === 'turn_complete') {
      if (statusEl) { statusEl.textContent = '✓ done'; statusEl.style.color = 'var(--accent)'; }
      entry.textEl = null;  // reset for next turn
    }

    if (ev.type === 'sub_agent_started') {
      if (typeof window.openSubAgentWin === 'function') window.openSubAgentWin(ev);
    }

    return;
  }

  switch (ev.type) {
    case 'session_init':        onSessionInit(ev);        break;
    case 'agent_text':          onAgentText(ev);          break;
    case 'turn_complete':       onTurnComplete();          break;
    case 'tool_requested':      onToolRequested(ev);      break;
    case 'tool_result':         onToolResult(ev);         break;
    case 'approval_pending':    onApprovalPending(ev);    break;
    case 'plugin_up':           onPluginUp(ev);           break;
    case 'plugin_down':         onPluginDown(ev);         break;
    case 'evolution_proposed':  onEvolutionProposed(ev);  break;
    case 'evolution_applied':   onEvolutionApplied(ev);   break;
    case 'sensor_reading':      onSensorReading(ev);      break;
    case 'wake_triggered':
      if (typeof window.onWakeTriggered === 'function') window.onWakeTriggered();
      break;
    case 'sub_agent_started':
      if (typeof window.openSubAgentWin === 'function') window.openSubAgentWin(ev);
      break;
    case 'council_started':
      if (typeof window.onCouncilStarted === 'function') window.onCouncilStarted(ev);
      break;
    case 'council_round_start':
      if (typeof window.onCouncilRoundStart === 'function') window.onCouncilRoundStart(ev);
      break;
    case 'council_agent_delta':
      if (typeof window.onCouncilAgentDelta === 'function') window.onCouncilAgentDelta(ev);
      break;
    case 'council_agent_done':
      if (typeof window.onCouncilAgentDone === 'function') window.onCouncilAgentDone(ev);
      break;
    case 'council_round_done':
      if (typeof window.onCouncilRoundDone === 'function') window.onCouncilRoundDone(ev);
      break;
    case 'council_complete':
      if (typeof window.onCouncilComplete === 'function') window.onCouncilComplete(ev);
      break;
    case 'error':               onAgentError(ev);          break;
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
  if (typeof window._voiceOnAgentText === 'function') window._voiceOnAgentText(ev.delta);
}

function onTurnComplete() {
  if (activeTurn) {
    // Snapshot agent text for session history
    const agentText = activeTurn.agentBlock.textContent.replace(/\s*$/, '');
    if (sessionTurns.length > 0) {
      sessionTurns[sessionTurns.length - 1].agent = agentText;
    }
    activeTurn.cursor.remove();
    activeTurn = null;
  }
  setCancelVisible(false);
  if (bootDone && ws?.readyState === WebSocket.OPEN) enableInput(true);
  scrollDown();
  if (typeof window._voiceOnAgentDone === 'function') window._voiceOnAgentDone();
}

// ─── Tool summary (one-liner shown in header without expanding) ───────────────
function toolSummary(name, input) {
  if (!input || typeof input !== 'object') return '';
  const i = input;
  const s = (v, n = 55) => v ? String(v).trim().slice(0, n) : '';
  const base = name.includes('__') ? name.split('__').pop() : name;
  switch (base) {
    case 'run_command':    return s(i.command, 70);
    case 'read_file':      return s(i.path || i.file_path);
    case 'write_file':     return s(i.path || i.file_path);
    case 'list_dir':       return s(i.path || i.directory || i.dir);
    case 'create_dir':     return s(i.path);
    case 'delete_path':    return s(i.path);
    case 'http_fetch':     return s(i.url, 70);
    case 'memory_search':
    case 'recall':         return i.query ? `"${s(i.query)}"` : '';
    case 'memory_store':
    case 'remember':       return s(i.content || i.memory);
    case 'store_intention':return s(i.title || i.content);
    case 'find_relevant_procedures': return i.query ? `"${s(i.query)}"` : '';
    case 'session_save':   return s(i.title || i.summary);
    case 'get_memory':
    case 'update_memory':  return s(i.memory_id);
    case 'agent_spawn':    return s(i.task || i.prompt);
    case 'send_to_agent':  return s(i.message);
    case 'convene_council':return s(i.topic);
    case 'bootstrap_node': return s(i.host || i.ip);
    case 'schedule_task':  return s(i.cron ? `${i.cron} — ${i.task || ''}` : i.task);
    case 'cancel_schedule':return s(i.id);
    case 'notify':         return s(i.message);
    case 'propose_evolution': return s(i.description || i.summary);
    case 'disk_usage':     return s(i.path);
    case 'audio_analyze':
    case 'audio_trim_silence':
    case 'audio_normalize':
    case 'audio_peak_limit':
    case 'audio_trim':     return s((i.path || i.input || '').split('/').pop());
    case 'audio_clean':    return s((i.input || '').split('/').pop());
    case 'gpio_read':      return i.pin != null ? `GPIO${i.pin}` : '';
    case 'gpio_write':     return i.pin != null ? `GPIO${i.pin} → ${i.value}` : '';
    case 'gpio_pulse':     return i.pin != null ? `GPIO${i.pin} ${i.duration_ms}ms` : '';
    case 'gpio_pwm':       return i.pin != null ? `GPIO${i.pin} duty=${i.duty_cycle}%` : '';
    case 'gpio_servo':     return i.pin != null ? `GPIO${i.pin} angle=${i.angle}°` : '';
    case 'display_face':   return s(i.state);
    case 'generate_song':  return [i.title, i.style].filter(Boolean).map(v => s(v, 28)).join(' · ');
    case 'check_status':   return s(i.job_id || i.id);
    case 'download_track': return s(i.track_id || i.job_id || i.id);
    case 'extend_track':   return s(i.track_id || i.id);
    case 'vast_launch':    return s(i.recipe);
    case 'vast_destroy':   return s(i.instance_id);
    case 'query_event_log':return i.hours ? `last ${i.hours}h` : '';
    default: {
      const v = i.query || i.message || i.command || i.path || i.name || i.title || i.url;
      return v ? s(String(v)) : '';
    }
  }
}

// ─── Tool calls ───────────────────────────────────────────────────────────────
function onToolRequested(ev) {
  const t   = ensureActiveTurn();
  const div = makeToolCallEl(ev.call.id, ev.call.tool, ev.call.input);
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
  const summary = toolSummary(toolName, input);

  header.innerHTML =
    `<span class="tool-tag">TOOL</span>` +
    `<span class="tool-name">${esc(toolName)}</span>` +
    (summary ? `<span class="tool-summary">${esc(summary)}</span>` : '') +
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

  const statusEl = callEl.querySelector('.tool-status');
  if (statusEl) {
    statusEl.className = `tool-status ${ev.output.ok ? 'ok' : 'err'}`;
    statusEl.textContent = ev.output.ok ? '✓' : '✗';
  }

  const body = callEl.querySelector('.tool-body');
  if (!body) return;

  const content = ev.output.content;
  const text = typeof content === 'string' ? content : JSON.stringify(content, null, 2);

  // Result preview: first non-empty line shown in header without expanding
  const header = callEl.querySelector('.tool-header');
  const toggle = callEl.querySelector('.tool-toggle');
  if (header && toggle) {
    const preview = document.createElement('span');
    preview.className = 'tool-result-preview';
    const firstLine = text.split('\n').find(l => l.trim()) || '';
    preview.textContent = firstLine.length > 60 ? firstLine.slice(0, 60) + '…' : firstLine;
    header.insertBefore(preview, toggle);
  }

  const resultEl = document.createElement('div');
  resultEl.className = `tool-result${ev.output.ok ? '' : ' err'}`;
  resultEl.textContent = text.length > 400 ? text.slice(0, 400) + '\n…' : text;
  body.appendChild(resultEl);

  // Auto-open only on error; success is summarised in the header
  if (!ev.output.ok) {
    body.classList.add('open');
    if (toggle) toggle.textContent = '▾';
  }

  scrollDown();
}

// ─── Approval ─────────────────────────────────────────────────────────────────
function onApprovalPending(ev) {
  const block = document.createElement('div');
  block.className = 'approval-block';

  const argsStr = JSON.stringify(ev.call.input, null, 2);
  const summary = toolSummary(ev.call.tool, ev.call.input);
  block.innerHTML =
    `<div class="approval-label">⚠  APPROVAL REQUIRED: <strong>${esc(ev.call.tool)}</strong>` +
    (summary ? ` <span class="tool-summary">${esc(summary)}</span>` : '') +
    `</div>` +
    `<details class="approval-detail"><summary>args</summary><div class="approval-args">${esc(argsStr)}</div></details>`;

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
window.pluginCounts = {};

function onPluginUp(ev) {
  pluginCounts[ev.plugin] = (ev.tools || []).length;
  refreshToolCount();

  const lines = document.getElementById('boot-lines');
  if (!bootDone) {
    updateBootLine(lines, 'PLUGIN SUPERVISOR', 'ok');
    addBootLine(lines, `  ${ev.plugin.toUpperCase()}`,
                `${pluginCounts[ev.plugin]} tools registered`, 'ok');
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

// ─── Evolution events ─────────────────────────────────────────────────────────
function onEvolutionProposed(ev) {
  if (!bootDone) return;
  const kind = ev.proposal?.kind || 'unknown';
  showSysMsg(`evolution proposed: ${kind.replace(/_/g, ' ')}`);
}

function onEvolutionApplied(ev) {
  const kind    = ev.proposal?.kind || 'unknown';
  const summary = ev.patch_summary  || '';

  const banner = document.createElement('div');
  banner.className = 'evo-banner';
  banner.innerHTML =
    `<span class="evo-tag">EVOLVED</span>` +
    `<span class="evo-kind">${esc(kind.replace(/_/g, ' '))}</span>` +
    `<span class="evo-summary">${esc(summary)}</span>`;
  document.getElementById('output').appendChild(banner);
  scrollDown();

  evoCount++;
  const badge = document.getElementById('hdr-evo');
  badge.textContent = `Σ${evoCount}`;
  badge.classList.remove('hidden');
}

function onAgentError(ev) {
  const d = document.createElement('div');
  d.className   = 'sys-msg';
  d.textContent = `error: ${ev.message}`;
  d.style.color = 'var(--error)';
  document.getElementById('output').appendChild(d);
  scrollDown();
}

// ─── Session resume ───────────────────────────────────────────────────────────
function onSessionInit(ev) {
  if (activeTurn) { activeTurn.cursor.remove(); activeTurn = null; }
  setCancelVisible(false);
  SESSION_ID = ev.session_id;
  localStorage.setItem('apexos_session_id', String(ev.session_id));
  if (ev.history && ev.history.length > 0) {
    renderHistory(ev.history);
  }
}

function renderHistory(messages) {
  const output = document.getElementById('output');
  output.innerHTML = '';
  sessionTurns = [];
  for (const msg of messages) {
    if (msg.role === 'user')           renderHistoryUser(msg);
    else if (msg.role === 'assistant') renderHistoryAssistant(msg);
  }
  scrollDown();
}

function renderHistoryUser(msg) {
  const text = (msg.content || [])
    .filter(b => b.type === 'text')
    .map(b => b.text)
    .join('');
  if (!text) return;
  const turnEl   = document.createElement('div');
  turnEl.className = 'turn';
  const userLine   = document.createElement('div');
  userLine.className   = 'user-line';
  userLine.dataset.time = '';
  userLine.textContent = text;
  turnEl.appendChild(userLine);
  document.getElementById('output').appendChild(turnEl);
}

function renderHistoryAssistant(msg) {
  const content = msg.content || [];
  const turnEl  = document.createElement('div');
  turnEl.className = 'turn';

  const textParts = content.filter(b => b.type === 'text' && b.text).map(b => b.text);
  if (textParts.length > 0) {
    const agentBlock = document.createElement('div');
    agentBlock.className = 'agent-block';
    agentBlock.textContent = textParts.join('');
    turnEl.appendChild(agentBlock);
  }

  for (const block of content) {
    if (block.type === 'tool_use') {
      const toolEl   = makeToolCallEl(block.id, block.name, block.input || {});
      const statusEl = toolEl.querySelector('.tool-status');
      if (statusEl) { statusEl.className = 'tool-status ok'; statusEl.textContent = '✓'; }
      turnEl.appendChild(toolEl);
    }
  }

  if (turnEl.children.length > 0) {
    document.getElementById('output').appendChild(turnEl);
  }
}

// ─── Session picker modal ─────────────────────────────────────────────────────
async function showSessionModal() {
  const content = document.getElementById('sessions-content');
  content.innerHTML = '<div class="session-empty">Loading...</div>';
  document.getElementById('sessions-modal').classList.remove('hidden');

  try {
    const res  = await fetch('/api/sessions');
    const data = await res.json();
    content.innerHTML = '';

    if (!Array.isArray(data) || data.length === 0) {
      content.innerHTML = '<div class="session-empty">No saved sessions yet.</div>';
      return;
    }

    for (const s of data) {
      const item    = document.createElement('div');
      item.className = 'session-item';
      if (s.session_id === SESSION_ID) item.classList.add('active');

      const preview = s.preview || '(empty)';
      const count   = s.message_count || 0;
      const rel     = relativeTime(s.last_active);

      item.innerHTML =
        `<div class="session-preview">${esc(preview)}` +
        (s.session_id === SESSION_ID ? ' <span class="session-current">current</span>' : '') +
        `</div>` +
        `<div class="session-meta">${count} messages · ${rel}</div>`;

      item.addEventListener('click', () => {
        hideSessionModal();
        sendWs({ type: 'hello', resume_session: s.session_id });
      });

      content.appendChild(item);
    }
  } catch {
    content.innerHTML =
      '<div class="session-empty" style="color:var(--error)">Failed to load sessions.</div>';
  }
}

function hideSessionModal() {
  document.getElementById('sessions-modal').classList.add('hidden');
}

function relativeTime(secs) {
  const diff = Math.floor(Date.now() / 1000 - secs);
  if (diff < 60)    return 'just now';
  if (diff < 3600)  return `${Math.floor(diff / 60)}m ago`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
  return `${Math.floor(diff / 86400)}d ago`;
}

// ─── Cancel ───────────────────────────────────────────────────────────────────
function cancelTurn() {
  if (!activeTurn) return;
  sendWs({ type: 'user_cancel', session: SESSION_ID });
  showSysMsg('cancelled');
}

function setCancelVisible(on) {
  const cancel = document.getElementById('cancel-btn');
  const send   = document.getElementById('send-btn');
  cancel.classList.toggle('hidden', !on);
  send.classList.toggle('hidden',  on);
}

// ─── New session ──────────────────────────────────────────────────────────────
function newSession() {
  // Drop the stored session ID so the server assigns a fresh one on reconnect
  localStorage.removeItem('apexos_session_id');
  SESSION_ID   = null;
  sessionTurns = [];
  if (activeTurn) { activeTurn.cursor.remove(); activeTurn = null; }
  setCancelVisible(false);

  document.getElementById('output').innerHTML = '';
  showSysMsg('new session...');
  // Reconnect — server will issue a fresh session_id via session_init
  voluntaryClose = true;
  if (ws) ws.close(); else scheduleReconnect();
}

// ─── History ──────────────────────────────────────────────────────────────────
function showHistory() {
  let history = [];
  try { history = JSON.parse(localStorage.getItem('apexos_history') || '[]'); } catch {}

  const content = document.getElementById('history-content');
  content.innerHTML = '';

  if (history.length === 0) {
    content.innerHTML = '<div class="history-empty">No saved sessions yet.</div>';
  } else {
    // Show most recent session first
    const last = history[history.length - 1];
    const date = new Date(last.ts);
    const heading = document.createElement('div');
    heading.className = 'history-date';
    heading.textContent = date.toLocaleString();
    content.appendChild(heading);

    for (const turn of last.turns) {
      if (turn.user) {
        const u = document.createElement('div');
        u.className = 'history-user';
        u.textContent = turn.user;
        content.appendChild(u);
      }
      if (turn.agent) {
        const a = document.createElement('div');
        a.className = 'history-agent';
        a.textContent = turn.agent.length > 400
          ? turn.agent.slice(0, 400) + '…'
          : turn.agent;
        content.appendChild(a);
      }
    }
  }

  document.getElementById('history-modal').classList.remove('hidden');
}

// ─── Evolution history modal ──────────────────────────────────────────────────
async function showEvoModal() {
  const content = document.getElementById('evo-content');
  content.innerHTML = '<div class="evo-empty">Loading...</div>';
  document.getElementById('evo-modal').classList.remove('hidden');

  try {
    const res  = await fetch('/api/evolution/history');
    const data = await res.json();
    content.innerHTML = '';

    if (!Array.isArray(data) || data.length === 0) {
      content.innerHTML = '<div class="evo-empty">No evolutions applied yet.</div>';
      return;
    }

    for (const ev of [...data].reverse()) {
      const kind    = ev.proposal?.kind || 'unknown';
      const summary = ev.patch_summary  || '';
      const item    = document.createElement('div');
      item.className = 'evo-item';
      item.innerHTML =
        `<div class="evo-item-header">` +
        `<span class="evo-tag">EVOLVED</span>` +
        `<span class="evo-item-kind">${esc(kind.replace(/_/g, ' '))}</span>` +
        `</div>` +
        `<div class="evo-item-summary">${esc(summary)}</div>`;
      content.appendChild(item);
    }
  } catch {
    content.innerHTML =
      '<div class="evo-empty" style="color:var(--error)">Failed to load history.</div>';
  }
}

function hideEvoModal() {
  document.getElementById('evo-modal').classList.add('hidden');
}

// ─── Power modal ──────────────────────────────────────────────────────────────
let powerCountdownTimer = null;

function showPowerModal() {
  document.getElementById('power-modal').classList.remove('hidden');
  document.getElementById('power-countdown').classList.add('hidden');
}

function hidePowerModal() {
  if (powerCountdownTimer) { clearInterval(powerCountdownTimer); powerCountdownTimer = null; }
  document.getElementById('power-modal').classList.add('hidden');
}

function triggerPower(action) {
  const countdown = document.getElementById('power-countdown');
  const btns      = document.getElementById('power-modal-btns');
  btns.classList.add('hidden');
  countdown.classList.remove('hidden');

  let secs = 3;
  const label = action === 'reboot' ? 'REBOOTING' : 'SHUTTING DOWN';
  countdown.textContent = `${label} in ${secs}...`;

  powerCountdownTimer = setInterval(() => {
    secs--;
    if (secs > 0) {
      countdown.textContent = `${label} in ${secs}...`;
    } else {
      clearInterval(powerCountdownTimer);
      powerCountdownTimer = null;
      countdown.textContent = `${label}...`;
      fetch('/api/power', {
        method:  'POST',
        headers: { 'content-type': 'application/json' },
        body:    JSON.stringify({ action }),
      }).catch(() => {});
    }
  }, 1000);
}

// ─── Backend selector (shared by CLI and Desktop skins) ───────────────────────
const _LOCAL_BACKENDS = ['ollama', 'vllm', 'oai'];
let _backendSelInit = false;

async function initBackendSelector() {
  const sel  = document.getElementById('backend-select');
  const urlI = document.getElementById('backend-url');
  if (!sel) return;

  // Fetch current config
  try {
    const d = await fetch('/api/backend').then(r => r.json());
    const b = (d.backend || 'anthropic').toLowerCase();
    sel.value = b;
    if (urlI && d.oai_base_url) urlI.value = d.oai_base_url;
    _applyBackendStyle(b, sel, urlI);
  } catch {}

  if (_backendSelInit) return;
  _backendSelInit = true;

  sel.addEventListener('change', () => _applyBackendStyle(sel.value, sel, urlI));

  async function applyBackend() {
    const backend = sel.value;
    const url     = urlI ? urlI.value.trim() : '';
    const body    = { backend };
    if (url) body.oai_base_url = url;
    try {
      await fetch('/api/backend', {
        method: 'POST', headers: { 'content-type': 'application/json' },
        body: JSON.stringify(body),
      });
      // Refresh model list for the new backend
      const cur = await fetch('/api/model').then(r => r.json()).then(d => d.model).catch(() => '');
      _modelSelInit = false; // allow re-attach is harmless; options will refresh
      initModelSelector(cur);
    } catch {}
  }

  sel.addEventListener('change', applyBackend);
  if (urlI) {
    urlI.addEventListener('keydown', e => { if (e.key === 'Enter') { e.preventDefault(); applyBackend(); } });
    urlI.addEventListener('blur', applyBackend);
  }
}

function _applyBackendStyle(b, sel, urlI) {
  const local = _LOCAL_BACKENDS.includes(b);
  if (sel) { if (local) sel.classList.add('local'); else sel.classList.remove('local'); }
  if (urlI) { if (local) urlI.classList.remove('hidden'); else urlI.classList.add('hidden'); }
  // Cloud filter: only meaningful for ollama; reset when switching away
  const cfBtn = document.getElementById('cloud-filter-btn');
  if (cfBtn) {
    if (b === 'ollama' && _allModels.length) cfBtn.classList.remove('hidden');
    else {
      cfBtn.classList.add('hidden');
      if (_cloudOnly) {
        _cloudOnly = false;
        cfBtn.classList.remove('active');
        const ms = document.getElementById('model-select');
        if (ms && _allModels.length) _renderModelOptions(ms, _allModels);
      }
    }
  }
}

// ─── Model selector ───────────────────────────────────────────────────────────
let _modelSelInit = false;
let _allModels    = [];
let _cloudOnly    = false;

function _renderModelOptions(sel, models) {
  const filtered = _cloudOnly ? models.filter(m => (m.id || '').endsWith(':cloud')) : models;
  const list     = filtered.length ? filtered : models; // show all if filter empties list
  if (list.length) {
    sel.innerHTML = list.map(m => {
      const id  = (m.id   || '').replace(/</g, '&lt;');
      const lbl = (m.name || m.id || '').toUpperCase().replace(/</g, '&lt;');
      return `<option value="${id}">${lbl}</option>`;
    }).join('');
  }
}

async function initModelSelector(currentModel) {
  const sel = document.getElementById('model-select');
  if (!sel) return;

  // Fetch available models from the active backend
  try {
    const res  = await fetch('/api/models');
    const data = await res.json();
    _allModels = data.models || [];
    _renderModelOptions(sel, _allModels);
  } catch { /* keep static fallback options */ }

  // Show/hide cloud filter based on current backend
  const backend = document.getElementById('backend-select')?.value || '';
  const cfBtn   = document.getElementById('cloud-filter-btn');
  if (cfBtn) {
    if (backend === 'ollama' && _allModels.length) cfBtn.classList.remove('hidden');
    else cfBtn.classList.add('hidden');
  }

  // Set current selection, inserting an option if the model isn't in the list
  if (currentModel) {
    const exists = Array.from(sel.options).some(o => o.value === currentModel);
    if (!exists) {
      const opt = document.createElement('option');
      opt.value = currentModel;
      opt.textContent = currentModel.toUpperCase();
      sel.insertBefore(opt, sel.firstChild);
    }
    sel.value = currentModel;
  }

  // Attach change listener once
  if (!_modelSelInit) {
    _modelSelInit = true;
    sel.addEventListener('change', async () => {
      try {
        await fetch('/api/model', {
          method:  'POST',
          headers: { 'content-type': 'application/json' },
          body:    JSON.stringify({ model: sel.value }),
        });
      } catch { /* offline */ }
    });
  }
}

function initCloudFilterBtn() {
  const btn = document.getElementById('cloud-filter-btn');
  if (!btn) return;
  btn.addEventListener('click', () => {
    _cloudOnly = !_cloudOnly;
    btn.classList.toggle('active', _cloudOnly);
    const sel = document.getElementById('model-select');
    if (sel) _renderModelOptions(sel, _allModels);
  });
}

// ─── Sending prompts ──────────────────────────────────────────────────────────
function sendPrompt() {
  const input = document.getElementById('prompt-input');
  const text  = input.value.trim();
  if (!text) return;

  // Shell passthrough: !cmd bypasses the agent and runs directly via /api/run
  if (text.startsWith('!')) {
    input.value = '';
    const cmd = text.slice(1).trimStart();
    if (cmd) runPassthrough(cmd);
    return;
  }

  if (ws?.readyState !== WebSocket.OPEN) return;

  input.value = '';
  enableInput(false);
  setCancelVisible(true);

  // Track for session history
  sessionTurns.push({ ts: Date.now(), user: text, agent: '' });

  const output     = document.getElementById('output');
  const turnEl     = document.createElement('div');
  turnEl.className = 'turn';

  const userLine = document.createElement('div');
  userLine.className   = 'user-line';
  userLine.dataset.time = timestamp();
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

// ─── Shell passthrough (!cmd) ─────────────────────────────────────────────────
async function runPassthrough(cmd) {
  const output  = document.getElementById('output');
  const turnEl  = document.createElement('div');
  turnEl.className = 'turn';

  const userLine = document.createElement('div');
  userLine.className   = 'user-line passthrough-line';
  userLine.dataset.time = timestamp();
  userLine.textContent = '$ ' + cmd;
  turnEl.appendChild(userLine);

  const result = document.createElement('div');
  result.className = 'agent-block';
  result.innerHTML = '<span class="passthrough-running">running…</span>';
  turnEl.appendChild(result);

  output.appendChild(turnEl);
  scrollDown();

  try {
    const r = await fetch('/api/run', {
      method:  'POST',
      headers: { 'content-type': 'application/json' },
      body:    JSON.stringify({ command: cmd }),
    });
    const d = await r.json();
    const out = [d.stdout, d.stderr].filter(Boolean).join('').trimEnd();
    if (!d.ok) {
      result.innerHTML = `<span class="passthrough-err">${esc(d.error || 'error')}</span>`;
    } else if (out) {
      result.innerHTML = `<pre class="passthrough-out">${esc(out)}</pre>`;
    } else {
      result.innerHTML = `<span class="passthrough-exit">exit ${d.exit_code}</span>`;
    }
  } catch (e) {
    result.innerHTML = `<span class="passthrough-err">${esc(String(e))}</span>`;
  }
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
  document.getElementById('ws-dot').className    = cls;
  document.getElementById('ws-label').textContent = label;
}

function setPolicySelect(mode) {
  const sel = document.getElementById('policy-select');
  if (!sel || !mode) return;
  sel.value = mode;
  sel.className = `policy-select policy-${(mode || '').toLowerCase().replace(/[^a-z]/g, '')}`;
}

function initPolicySelect() {
  const sel = document.getElementById('policy-select');
  if (!sel) return;
  sel.addEventListener('change', async () => {
    try {
      await fetch('/api/policy', {
        method:  'POST',
        headers: { 'content-type': 'application/json' },
        body:    JSON.stringify({ mode: sel.value }),
      });
    } catch { /* offline */ }
  });
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

function timestamp() {
  const d = new Date();
  return `${String(d.getHours()).padStart(2,'0')}:${String(d.getMinutes()).padStart(2,'0')}`;
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

    // Apply server-reported state
    if (data.model)       initModelSelector(data.model);
    if (data.policy_mode) setPolicySelect(data.policy_mode);
    initBackendSelector();
    initCloudFilterBtn();

    if (data.api_key_set) return;
  } catch {
    return;
  }

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

      submit.disabled    = true;
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
          submit.disabled    = false;
          submit.textContent = 'CONNECT';
        }
      } catch {
        showKeyError('Could not reach agentd — is it running?');
        submit.disabled    = false;
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

// ─── Collapse all tools ───────────────────────────────────────────────────────
function collapseAllTools() {
  const open = document.querySelectorAll('.tool-body.open');
  if (open.length > 0) {
    open.forEach(b => {
      b.classList.remove('open');
      const toggle = b.previousElementSibling?.querySelector('.tool-toggle');
      if (toggle) toggle.textContent = '▸';
    });
  } else {
    document.querySelectorAll('.tool-body').forEach(b => {
      b.classList.add('open');
      const toggle = b.previousElementSibling?.querySelector('.tool-toggle');
      if (toggle) toggle.textContent = '▾';
    });
  }
}

// ─── Sensor readings ──────────────────────────────────────────────────────────
const sensorState = { env: null, thermal: null, cpu_c: null, ts: null };

function onSensorReading(ev) {
  const r = ev.reading;
  if (!r) return;
  if (r.kind === 'air_quality')  { sensorState.env    = r; sensorState.ts = Date.now(); }
  if (r.kind === 'thermal_frame'){
    sensorState.thermal = r;
    if (typeof window.updateThermalWallpaper === 'function') window.updateThermalWallpaper(r);
  }
  if (r.kind === 'temperature' && r.sensor_id === 'cpu_thermal') sensorState.cpu_c = r.celsius;
  updateSensorWidget();
}

function iaqLabel(iaq) {
  if (iaq <  51) return 'Excellent';
  if (iaq < 101) return 'Good';
  if (iaq < 151) return 'Lightly polluted';
  if (iaq < 201) return 'Moderately polluted';
  if (iaq < 251) return 'Heavily polluted';
  return 'Severely polluted';
}

function iaqColor(iaq) {
  if (iaq <  51) return '#39ff14';
  if (iaq < 101) return '#a0ff70';
  if (iaq < 151) return '#f0b429';
  if (iaq < 201) return '#ff8c00';
  return '#ff4444';
}

function updateSensorWidget() {
  const env     = sensorState.env;
  const thermal = sensorState.thermal;

  const set = (id, val) => { const e = document.getElementById(id); if (e) e.textContent = val; };

  if (env) {
    set('s-temp',  env.temperature_c?.toFixed(1) ?? '—');
    set('s-humid', env.humidity_pct?.toFixed(1)  ?? '—');
    set('s-press', env.pressure_hpa?.toFixed(0)  ?? '—');
    const iaq = env.iaq ?? 0;
    set('s-iaq', iaq.toFixed(0));
    const bar = document.getElementById('s-iaq-bar');
    if (bar) {
      bar.style.width = Math.min(100, iaq / 5) + '%';
      bar.style.background = iaqColor(iaq);
    }
    set('s-iaq-label', iaqLabel(iaq));
    if (sensorState.ts) {
      const age = Math.round((Date.now() - sensorState.ts) / 1000);
      set('sensor-age', age < 90 ? `${age}s ago` : `${Math.round(age/60)}m ago`);
    }
  }
  if (thermal) {
    set('s-tmin',  thermal.min_c?.toFixed(1)  ?? '—');
    set('s-tmean', thermal.mean_c?.toFixed(1) ?? '—');
    set('s-tmax',  thermal.max_c?.toFixed(1)  ?? '—');
  }
  if (sensorState.cpu_c !== null) {
    set('s-cpu', sensorState.cpu_c.toFixed(1));
  }
}

// ─── Skin switcher ────────────────────────────────────────────────────────────
function switchSkin(target) {
  location.href = target === 'desktop' ? '/desktop.html' : '/';
}

// ─── Init ─────────────────────────────────────────────────────────────────────
document.addEventListener('DOMContentLoaded', () => {
  // Send / input bar
  document.getElementById('send-btn').addEventListener('click', sendPrompt);
  document.getElementById('prompt-input').addEventListener('keydown', e => {
    if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); sendPrompt(); }
  });

  // Cancel
  document.getElementById('cancel-btn').addEventListener('click', cancelTurn);

  // New session
  document.getElementById('new-session-btn').addEventListener('click', newSession);

  // Power modal
  document.getElementById('power-btn').addEventListener('click', showPowerModal);
  document.getElementById('power-dismiss-btn').addEventListener('click', hidePowerModal);
  document.getElementById('power-reboot-btn').addEventListener('click', () => triggerPower('reboot'));
  document.getElementById('power-shutdown-btn').addEventListener('click', () => triggerPower('shutdown'));
  document.getElementById('power-modal').addEventListener('click', e => {
    if (e.target === document.getElementById('power-modal')) hidePowerModal();
  });

  // History: double-click the logo or click hdr-center
  document.getElementById('hdr-logo').addEventListener('dblclick', showHistory);
  document.getElementById('history-close-btn').addEventListener('click', () => {
    document.getElementById('history-modal').classList.add('hidden');
  });
  document.getElementById('history-modal').addEventListener('click', e => {
    if (e.target === document.getElementById('history-modal')) {
      document.getElementById('history-modal').classList.add('hidden');
    }
  });

  // Evolution history: click evo counter badge
  document.getElementById('hdr-evo').addEventListener('click', showEvoModal);
  document.getElementById('evo-close-btn').addEventListener('click', hideEvoModal);
  document.getElementById('evo-modal').addEventListener('click', e => {
    if (e.target === document.getElementById('evo-modal')) hideEvoModal();
  });

  // Session picker: click #hdr-sessions badge
  document.getElementById('hdr-sessions').addEventListener('click', showSessionModal);
  document.getElementById('sessions-close-btn').addEventListener('click', hideSessionModal);
  document.getElementById('sessions-new-btn').addEventListener('click', () => {
    hideSessionModal(); newSession();
  });
  document.getElementById('sessions-modal').addEventListener('click', e => {
    if (e.target === document.getElementById('sessions-modal')) hideSessionModal();
  });

  // Collapse all tools on hdr-center click (also shows tool count)
  document.getElementById('hdr-center').addEventListener('click', collapseAllTools);

  // Global keyboard shortcuts
  document.addEventListener('keydown', e => {
    if (e.key === 'Escape') {
      if (!document.getElementById('power-modal').classList.contains('hidden')) {
        hidePowerModal(); return;
      }
      if (!document.getElementById('sessions-modal').classList.contains('hidden')) {
        hideSessionModal(); return;
      }
      if (!document.getElementById('history-modal').classList.contains('hidden')) {
        document.getElementById('history-modal').classList.add('hidden'); return;
      }
      if (!document.getElementById('evo-modal').classList.contains('hidden')) {
        hideEvoModal(); return;
      }
      if (activeTurn) { cancelTurn(); return; }
    }
    if (e.key === 'k' && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      newSession();
    }
    if (e.key === 'S' && (e.ctrlKey || e.metaKey) && e.shiftKey) {
      e.preventDefault();
      showSessionModal();
    }
    if (e.key === 'E' && (e.ctrlKey || e.metaKey) && e.shiftKey) {
      e.preventDefault();
      showEvoModal();
    }
  });

  initPolicySelect();
  runBoot()
    .then(checkAndMaybePromptKey)
    .then(() => {
      connect();
      setTimeout(transitionToApp, 4000);
    });
});
