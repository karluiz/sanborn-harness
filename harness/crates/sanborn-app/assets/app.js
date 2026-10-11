// Display-only client. Rust owns reasoning, Laya, tools, permissions and writes.
// The session token lives only in memory; it is removed from the address bar.
'use strict';

const $ = (id) => document.getElementById(id);
const token = new URLSearchParams(location.hash.slice(1)).get('token');
history.replaceState(null, '', location.pathname);

let runId = null;
let events = [];
let polling = null;
let snapshot = null;
const checked = new Set();

function el(tag, attrs = {}, ...children) {
  const node = document.createElement(tag);
  for (const [key, value] of Object.entries(attrs)) {
    if (key === 'text') node.textContent = value;
    else if (key.startsWith('data-') || key.startsWith('aria-')) node.setAttribute(key, value);
    else node[key] = value;
  }
  node.append(...children.filter((child) => child != null));
  return node;
}

async function api(path, body) {
  const response = await fetch('/api' + path, {
    method: body === undefined ? 'GET' : 'POST',
    headers: { Authorization: 'Bearer ' + token, ...(body === undefined ? {} : { 'Content-Type': 'application/json' }) },
    body: body === undefined ? undefined : JSON.stringify(body),
    cache: 'no-store',
  });
  const data = await response.json().catch(() => ({ error: response.statusText }));
  if (!response.ok) throw new Error(data.error || 'Request failed (' + response.status + ')');
  return data;
}

function show(id, visible) { $(id).hidden = !visible; }

function formError(message) {
  $('form-error').textContent = message || '';
  show('form-error', Boolean(message));
}

// ---- Progress -------------------------------------------------------------

function describe(event) {
  switch (event.event) {
    case 'reasoning_started': return ['◇', 'Reasoning turn ' + event.turn + ' requested from the provider'];
    case 'plan_accepted': return ['▣', 'Rust validated a plan of ' + event.steps + (event.steps === 1 ? ' step' : ' steps')];
    case 'laya_decision': {
      const d = event.result || {};
      if (d.decision === 'continue_planned_step') return ['✓', 'Laya ' + event.stage + ': continue ' + event.step];
      return ['!', 'Laya ' + event.stage + ' for ' + event.step + ': ' + d.decision + ' (' + (d.reason || '') + ')', 'warn'];
    }
    case 'tool_finished':
      return event.is_error
        ? ['✕', event.tool + ' failed (' + event.duration_ms + ' ms)', 'bad']
        : ['→', event.tool + ' finished (' + event.duration_ms + ' ms)'];
    case 'summoned': return ['∿', 'Summoning ' + event.count + (event.count === 1 ? ' tadpole' : ' tadpoles')];
    case 'contract_started': return ['§', event.specialty + ' contract baseline started'];
    case 'croak': return event.status === 'returned'
      ? ['◀', 'Croak ' + event.id + ': returned (not certified)']
      : ['◀', 'Croak ' + event.id + ': blocked', 'bad'];
    case 'finished': return ['■', 'Model gave a final answer after ' + event.turns + ' turns'];
    default: return ['·', event.event];
  }
}

function progressItem(event) {
  let nested = event;
  const path = [];
  while (nested.event === 'tadpole') { path.push(nested.id); nested = nested.detail; }
  const [mark, text, tone] = describe(nested);
  const item = el('li', tone ? { 'data-tone': tone } : {}, el('span', { className: 'mark', text: mark }));
  const body = el('span');
  if (path.length) body.append(el('span', { className: 'nest', text: '[' + path.join(' › ') + '] ' }));
  body.append(text);
  item.append(body);
  return item;
}

function tadpoleStates() {
  const pods = new Map();
  const pod = (id) => pods.get(id) || pods.set(id, { id, state: 'swimming', specialty: null, tools: 0 }).get(id);
  for (const event of events) {
    if (event.event === 'tadpole') {
      const p = pod(event.id);
      if (event.detail.event === 'contract_started') p.specialty = event.detail.specialty;
      if (event.detail.event === 'tool_finished') p.tools += 1;
    }
    if (event.event === 'croak') pod(event.id).state = event.status;
  }
  for (const croak of (snapshot?.run?.croaks || [])) {
    const p = pod(croak.id);
    p.state = croak.status;
    p.specialty = croak.specialty || p.specialty;
    p.report = croak.report || croak.reason || '';
  }
  return [...pods.values()];
}

function renderTadpoles() {
  const list = $('tadpoles');
  const pods = tadpoleStates();
  list.replaceChildren(...pods.map((p) => el('li', {},
    el('span', { className: 'glyph', text: p.state === 'swimming' ? '∿' : '◀', 'aria-hidden': 'true' }),
    el('span', { className: 'who', text: p.id }, el('small', { text: (p.specialty || 'general') + ' · ' + p.tools + ' tools' })),
    el('span', { className: 'state', 'data-state': p.state, text: p.state === 'returned' ? 'returned · not certified' : p.state }),
    p.report ? el('p', { className: 'report', text: p.report }) : null,
  )));
  show('tadpoles-wrap', pods.length > 0);
}

// ---- Proposals --------------------------------------------------------------

function lineDiff(before, after) {
  // A trailing newline ends the last line rather than starting an empty one.
  const lines = (text) => (text.endsWith('\n') ? text.slice(0, -1) : text).split('\n');
  const a = lines(before);
  const b = lines(after);
  const n = a.length; const m = b.length;
  const lcs = Array.from({ length: n + 1 }, () => new Uint16Array(m + 1));
  for (let i = n - 1; i >= 0; i--) for (let j = m - 1; j >= 0; j--) {
    lcs[i][j] = a[i] === b[j] ? lcs[i + 1][j + 1] + 1 : Math.max(lcs[i + 1][j], lcs[i][j + 1]);
  }
  const out = [];
  let i = 0; let j = 0;
  while (i < n || j < m) {
    if (i < n && j < m && a[i] === b[j]) { out.push(['same', a[i]]); i++; j++; }
    else if (i < n && (j === m || lcs[i + 1][j] >= lcs[i][j + 1])) { out.push(['del', a[i]]); i++; }
    else { out.push(['add', b[j]]); j++; }
  }
  return out;
}

function applicationText(app) {
  switch (app.status) {
    case 'applied_verified': return ['Applied and verified: ' + app.detail, 'good'];
    case 'applied_unverified': return ['Applied, NOT verified: ' + app.detail, 'warn'];
    case 'applying': return ['Applying…', null];
    default: return ['Not applied: ' + app.detail, 'bad'];
  }
}

function renderProposals(run) {
  const host = $('proposals');
  const attempted = Object.entries(run.applications || {});
  const open = run.proposals || [];
  const cards = [];
  for (const proposal of open) {
    const tags = el('div', { className: 'tags' },
      el('span', { className: 'tag', text: 'proposal · not applied' }),
      el('span', { className: 'tag', 'data-tone': proposal.ready ? 'good' : 'bad', text: proposal.ready ? 'Laya contract passed' : 'contract incomplete' }),
      proposal.stale ? el('span', { className: 'tag', 'data-tone': 'warn', text: 'stale · file changed' }) : null,
    );
    const diff = el('pre', { className: 'diff', 'aria-label': 'Line changes for ' + proposal.path },
      ...lineDiff(proposal.expected_content, proposal.content).map(([kind, line]) => el('span', { className: kind, text: line })));
    const box = el('input', { type: 'checkbox', id: 'grant-' + proposal.proposal_id, checked: checked.has(proposal.proposal_id) });
    const button = el('button', { type: 'button', className: 'primary', text: 'Apply with write permission' });
    const blocked = proposal.stale ? 'Stale: the file no longer matches what the coder read. Run again for a fresh proposal.'
      : !proposal.ready ? 'This proposal did not complete its Laya-gated contract and cannot be applied.' : null;
    const busy = run.status === 'running' || snapshot.permission.applying;
    button.disabled = Boolean(blocked) || busy || !box.checked;
    box.disabled = Boolean(blocked) || busy;
    box.addEventListener('change', () => {
      if (box.checked) checked.add(proposal.proposal_id); else checked.delete(proposal.proposal_id);
      button.disabled = !box.checked;
    });
    const outcome = el('p', { className: 'outcome', text: blocked || 'One attempt per proposal. Rust replaces the file only if it still matches exactly, then rereads it to check.' });
    if (blocked) outcome.setAttribute('data-tone', 'warn');
    button.addEventListener('click', async () => {
      button.disabled = true; box.disabled = true;
      outcome.textContent = 'Applying through apply_tadpole_patch…';
      try {
        await api('/runs/' + run.id + '/proposals/' + encodeURIComponent(proposal.proposal_id) + '/apply', { grant_write: box.checked });
      } catch (error) {
        outcome.textContent = error.message; outcome.setAttribute('data-tone', 'bad');
      }
      checked.delete(proposal.proposal_id);
      await refresh();
    });
    cards.push(el('article', { className: 'proposal' },
      el('header', {}, el('code', { text: proposal.path + '  ·  ' + proposal.proposal_id }), tags),
      diff,
      el('div', { className: 'apply' },
        el('label', { htmlFor: box.id }, box, el('span', { text: 'Grant write permission for this one replacement of ' + proposal.path + ' in ' + run.workspace })),
        button, outcome)));
  }
  for (const [id, app] of attempted) {
    if (open.some((p) => p.proposal_id === id)) continue;
    const [text, tone] = applicationText(app);
    cards.push(el('article', { className: 'proposal' },
      el('header', {}, el('code', { text: id }), el('div', { className: 'tags' }, el('span', { className: 'tag', text: 'consumed' }))),
      el('div', { className: 'apply' }, el('p', { className: 'outcome', 'data-tone': tone || '', text }))));
  }
  host.replaceChildren(...cards);
  show('proposals-wrap', cards.length > 0);
}

// ---- Run state ---------------------------------------------------------------

function hint(message) {
  if (/sb init|AZURE_OPENAI|keyring|credential/i.test(message)) return 'Configure inference with `sb init` (or Azure environment variables) and restart the app. Credentials never pass through this page.';
  if (/budget exhausted/.test(message)) return 'The shared inference-call budget ran out. Narrow the task or restart with a higher --max-inference-calls.';
  if (/deadline/.test(message)) return 'The run hit its deadline. Narrow the task or restart with a larger --timeout.';
  if (/reasoning limit/.test(message)) return 'The model used all reasoning turns without a final answer. Try a narrower task.';
  if (/ran out of responses/.test(message)) return 'The synthetic script only covers the greeting fixture demo. Use examples/app/run.sh, or launch without --synthetic.';
  return 'Inspect the progress list. Nothing was written: runs are read-only.';
}

function renderLedger(run) {
  const apps = Object.values(run?.applications || {});
  const proposals = run?.proposals || [];
  const runtime = !run ? 'not started'
    : { running: 'running', completed: 'completed · not certified', failed: 'failed', cancelled: 'cancelled' }[run.status];
  $('l-runtime').textContent = runtime;
  $('l-proposal').textContent = proposals.length ? proposals.length + ' open · inspect below' : apps.length ? 'consumed' : 'none';
  const applied = apps.filter((a) => a.status.startsWith('applied'));
  $('l-application').textContent = snapshot.permission.applying ? 'applying…'
    : applied.length ? applied.length + ' written' : apps.length ? 'attempted · not written' : 'none';
  const verified = apps.filter((a) => a.status === 'applied_verified').length;
  $('l-verification').textContent = verified ? verified + ' exact reread · no tests run'
    : applied.length ? 'reread failed' : 'none · no tests run';
}

function render() {
  const s = snapshot;
  const run = s.run;
  $('reasoning-kind').textContent = s.reasoning.kind === 'synthetic' ? 'synthetic script' : 'Azure';
  $('reasoning-kind').setAttribute('data-kind', s.reasoning.kind);
  $('reasoning-label').textContent = s.reasoning.label;
  $('laya').textContent = s.laya_backend + ' · required';
  const l = s.limits;
  $('limits').textContent = l.max_inference_calls + ' inference calls shared with tadpoles, ' + l.max_tadpoles + ' tadpoles, ' + l.timeout_secs + ' s';
  $('workspace-current').textContent = s.workspace || 'none';
  const running = run?.status === 'running';
  const busy = running || s.permission.applying;
  $('perm-state').textContent = s.permission.applying ? 'Applying one granted write…' : 'Read-only.';
  document.querySelector('.permission').setAttribute('data-writing', String(s.permission.applying));
  $('run').disabled = busy || !s.workspace || !token;
  $('run').textContent = running ? 'Running…' : 'Run read-only';
  $('workspace-form').querySelector('button').disabled = busy;
  show('cancel', running);
  show('empty', !run);
  show('progress-wrap', Boolean(run));
  if (s.workspace && !$('workspace-path').value) $('workspace-path').value = s.workspace;
  const status = run ? run.status : 'idle';
  $('status').textContent = status;
  $('status').setAttribute('data-status', status);
  renderLedger(run);
  if (!run) return;
  const usage = run.usage;
  $('meters').textContent = [
    run.elapsed_ms < 1000 ? run.elapsed_ms + ' ms' : (run.elapsed_ms / 1000).toFixed(1) + ' s',
    'inference ' + (run.inference_calls ?? 0) + '/' + l.max_inference_calls,
    usage ? 'tokens ' + usage.input_tokens + ' in · ' + usage.output_tokens + ' out' : null,
    'tools ' + run.tool_calls,
    run.usage_complete === false ? 'usage incomplete' : null,
  ].filter(Boolean).join('  ·  ');
  const failed = run.status === 'failed' || run.status === 'cancelled';
  show('run-error', failed);
  if (failed) {
    $('run-error').setAttribute('data-tone', run.status);
    $('run-error-title').textContent = run.status === 'cancelled' ? 'Run cancelled' : 'Run failed';
    $('run-error-text').textContent = run.error || '';
    $('run-error-hint').textContent = run.status === 'cancelled' ? 'You can submit the task again.' : hint(run.error || '');
  }
  $('progress-count').textContent = '(' + events.length + (run.dropped_events ? ', ' + run.dropped_events + ' not shown' : '') + ')';
  renderTadpoles();
  show('result-wrap', Boolean(run.final_text));
  $('result').textContent = run.final_text || '';
  renderProposals(run);
}

async function refresh() {
  if (!token) return;
  try {
    const next = await api('/state?after=' + (snapshot?.run && snapshot.run.id === runId ? events.length : 0));
    if (next.run && next.run.id !== runId) {
      runId = next.run.id; events = []; $('progress').replaceChildren();
    }
    if (next.run) {
      events.push(...next.run.events);
      $('progress').append(...next.run.events.map(progressItem));
    }
    snapshot = next;
    show('session', false);
    render();
    const active = next.run?.status === 'running' || next.permission.applying;
    if (active && !polling) polling = setInterval(refresh, 500);
    if (!active && polling) { clearInterval(polling); polling = null; }
  } catch (error) {
    $('session').textContent = error.message;
    show('session', true);
  }
}

// ---- Intents -------------------------------------------------------------------

$('workspace-form').addEventListener('submit', async (event) => {
  event.preventDefault();
  formError('');
  try {
    await api('/workspace', { path: $('workspace-path').value.trim() });
    await refresh();
  } catch (error) { formError(error.message); }
});

$('run-form').addEventListener('submit', async (event) => {
  event.preventDefault();
  formError('');
  $('run').disabled = true;
  try {
    await api('/runs', { prompt: $('prompt').value });
  } catch (error) { formError(error.message); }
  await refresh();
});

$('cancel').addEventListener('click', async () => {
  $('cancel').disabled = true;
  try { await api('/runs/' + runId + '/cancel', {}); } catch (error) { formError(error.message); }
  $('cancel').disabled = false;
  await refresh();
});

// Rechecks proposal staleness when you come back from editing files elsewhere.
window.addEventListener('focus', refresh);

if (!token) {
  $('session').textContent = 'No session token. Open the exact link printed by sanborn-app in your terminal.';
  show('session', true);
  $('run').disabled = true;
} else {
  refresh();
}
