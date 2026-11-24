// Alice Web GUI. No framework, no build step.
// Security: user/model content is only inserted as text, or through md()
// which escapes everything before adding a small set of safe tags.
'use strict';

const $ = (id) => document.getElementById(id);
const S = {
  threads: [], current: null, thread: null, messages: [], attachments: [],
  generating: false, models: { models: [], default: '' }, pending: [], newModel: '',
  approvals: new Map(), toolLines: new Map(), es: null, config: null,
};

// ------------------------------------------------------------ helpers

function el(tag, attrs, ...kids) {
  const e = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs || {})) {
    if (v == null || v === false) continue;
    if (k === 'class') e.className = v;
    else if (k.startsWith('on')) e.addEventListener(k.slice(2), v);
    else e.setAttribute(k, v === true ? '' : v);
  }
  for (const k of kids.flat()) if (k != null && k !== false) e.append(k instanceof Node ? k : document.createTextNode(String(k)));
  return e;
}

function esc(s) {
  return String(s).replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
}

function toast(msg) {
  const t = $('toast');
  t.textContent = msg;
  t.classList.remove('hidden');
  clearTimeout(toast.timer);
  toast.timer = setTimeout(() => t.classList.add('hidden'), 3500);
}

async function api(method, path, body) {
  const opts = { method, headers: { 'X-Alice-CSRF': '1' }, credentials: 'same-origin' };
  if (body !== undefined) {
    if (body instanceof Blob) {
      opts.body = body;
    } else {
      opts.headers['Content-Type'] = 'application/json';
      opts.body = JSON.stringify(body);
    }
  }
  const r = await fetch('/api' + path, opts);
  if (r.status === 401 && path !== '/login') {
    showLogin();
    throw new Error('unauthorized');
  }
  const text = await r.text();
  let v = null;
  try { v = text ? JSON.parse(text) : null; } catch { v = text; }
  if (!r.ok) throw new Error((v && v.error) || r.statusText);
  return v;
}

function fmtTime(ms) {
  const d = new Date(ms);
  return d.toLocaleString(undefined, { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' });
}

function fmtBytes(n) {
  if (n < 1024) return n + ' B';
  if (n < 1048576) return (n / 1024).toFixed(1) + ' KB';
  return (n / 1048576).toFixed(1) + ' MB';
}

// ------------------------------------------------------------ markdown

function inline(s) {
  let out = '';
  for (const p of s.split(/(`[^`\n]+`)/g)) {
    if (p.length > 1 && p.startsWith('`') && p.endsWith('`')) {
      out += '<code>' + esc(p.slice(1, -1)) + '</code>';
      continue;
    }
    let t = esc(p);
    t = t.replace(/\[([^\]]+)\]\((https?:\/\/[^\s)]+)\)/g, '<a href="$2" target="_blank" rel="noopener noreferrer">$1</a>');
    t = t.replace(/(^|[\s(])(https?:\/\/[^\s<)]+)/g, '$1<a href="$2" target="_blank" rel="noopener noreferrer">$2</a>');
    t = t.replace(/\*\*([^*\n]+)\*\*/g, '<strong>$1</strong>');
    t = t.replace(/(^|[^*\w])\*([^*\n]+)\*(?!\w)/g, '$1<em>$2</em>');
    t = t.replace(/~~([^~\n]+)~~/g, '<del>$1</del>');
    out += t;
  }
  return out;
}

function md(src) {
  const lines = String(src).replace(/\r\n/g, '\n').split('\n');
  const out = [];
  let i = 0;
  const isBlockStart = (l) => /^\s*(```|#{1,6}\s|>|[-*+]\s|\d+[.)]\s|\|)/.test(l) || /^\s*([-*_])\s*\1\s*\1[\s\1]*$/.test(l);
  while (i < lines.length) {
    const line = lines[i];
    const fence = line.match(/^\s*```\s*([\w+#.-]*)/);
    if (fence) {
      const lang = fence[1];
      const code = [];
      i++;
      while (i < lines.length && !/^\s*```\s*$/.test(lines[i])) code.push(lines[i++]);
      i++;
      out.push('<div class="codeblock">' + (lang ? '<span class="lang">' + esc(lang) + '</span>' : '') +
        '<button class="copy ghost" type="button">Copy</button><pre><code>' + esc(code.join('\n')) + '</code></pre></div>');
      continue;
    }
    if (!line.trim()) { i++; continue; }
    const h = line.match(/^(#{1,6})\s+(.*)$/);
    if (h) { out.push(`<h${h[1].length}>${inline(h[2])}</h${h[1].length}>`); i++; continue; }
    if (/^\s*([-*_])\s*\1\s*\1[\s\1]*$/.test(line)) { out.push('<hr>'); i++; continue; }
    if (/^\s*>/.test(line)) {
      const q = [];
      while (i < lines.length && /^\s*>/.test(lines[i])) q.push(lines[i++].replace(/^\s*>\s?/, ''));
      out.push('<blockquote>' + md(q.join('\n')) + '</blockquote>');
      continue;
    }
    if (/^\s*\|/.test(line) && i + 1 < lines.length && /^\s*\|?\s*:?-+/.test(lines[i + 1])) {
      const cells = (l) => l.trim().replace(/^\||\|$/g, '').split('|').map((c) => inline(c.trim()));
      let t = '<table><thead><tr>' + cells(line).map((c) => `<th>${c}</th>`).join('') + '</tr></thead><tbody>';
      i += 2;
      while (i < lines.length && /^\s*\|/.test(lines[i])) t += '<tr>' + cells(lines[i++]).map((c) => `<td>${c}</td>`).join('') + '</tr>';
      out.push(t + '</tbody></table>');
      continue;
    }
    const li = line.match(/^\s*([-*+]|\d+[.)])\s+/);
    if (li) {
      const ordered = /\d/.test(li[1]);
      const items = [];
      while (i < lines.length) {
        const m = lines[i].match(/^\s*([-*+]|\d+[.)])\s+(.*)$/);
        if (m) { items.push(m[2]); i++; }
        else if (lines[i].trim() && /^\s{2,}/.test(lines[i]) && items.length) { items[items.length - 1] += '\n' + lines[i].trim(); i++; }
        else break;
      }
      const tag = ordered ? 'ol' : 'ul';
      out.push(`<${tag}>` + items.map((it) => '<li>' + inline(it).replace(/\n/g, '<br>') + '</li>').join('') + `</${tag}>`);
      continue;
    }
    const para = [];
    while (i < lines.length && lines[i].trim() && !isBlockStart(lines[i])) para.push(lines[i++]);
    if (!para.length) para.push(lines[i++]);
    out.push('<p>' + para.map(inline).join('<br>') + '</p>');
  }
  return out.join('');
}

document.addEventListener('click', (e) => {
  const b = e.target.closest('.codeblock .copy');
  if (!b) return;
  const code = b.parentElement.querySelector('code').textContent;
  navigator.clipboard?.writeText(code).then(() => { b.textContent = 'Copied'; setTimeout(() => (b.textContent = 'Copy'), 1200); });
});

// ------------------------------------------------------------ login

function showLogin() {
  $('app').classList.add('hidden');
  $('settings').classList.add('hidden');
  $('login').classList.remove('hidden');
  S.es?.close();
  $('login-token').focus();
}

$('login-form').addEventListener('submit', async (e) => {
  e.preventDefault();
  $('login-error').textContent = '';
  try {
    await api('POST', '/login', { token: $('login-token').value.trim() });
    $('login-token').value = '';
    $('login').classList.add('hidden');
    start();
  } catch (err) {
    $('login-error').textContent = err.message === 'unauthorized' ? 'Invalid token' : err.message;
  }
});

// ------------------------------------------------------------ threads

async function loadThreads() {
  const v = await api('GET', '/threads');
  S.threads = v.threads;
  renderThreads();
}

function renderThreads() {
  const nav = $('threads');
  nav.replaceChildren(...S.threads.map((t) =>
    el('button', {
      class: 'thread' + (t.id === S.current ? ' active' : ''),
      title: t.title || 'New chat',
      onclick: () => { openThread(t.id); closeSide(); },
    }, t.generating ? el('span', { class: 'dot' }, '● ') : null, t.title || 'New chat')));
}

function closeSide() { $('app').classList.remove('side-open'); }

function newChat() {
  S.current = null; S.thread = null; S.messages = []; S.attachments = []; S.pending = [];
  S.generating = false;
  renderAll();
  $('input').focus();
}

async function openThread(id) {
  S.current = id;
  S.pending = [];
  try {
    const [t, m] = await Promise.all([api('GET', '/threads/' + id), api('GET', `/threads/${id}/messages`)]);
    if (S.current !== id) return;
    S.thread = t.thread;
    S.messages = m.messages;
    S.attachments = m.attachments;
    S.generating = m.generating;
    S.toolLines.clear();
    try { localStorage.setItem('alice.thread', id); } catch {}
  } catch (e) {
    toast(e.message);
    newChat();
    return;
  }
  renderAll();
  await refreshApprovals();
}

function renderAll() {
  renderThreads();
  renderHeader();
  renderMessages();
  renderComposer();
}

function renderHeader() {
  $('thread-title').textContent = S.thread?.title || 'New chat';
  const sel = $('model-select');
  const cur = S.thread ? (S.thread.model || '') : S.newModel;
  const def = S.models.default;
  sel.replaceChildren(
    el('option', { value: '' }, def ? `Default (${def})` : 'No model'),
    ...S.models.models.map((m) => el('option', { value: m }, m)));
  sel.value = cur;
  $('thread-menu').classList.toggle('hidden', !S.current);
}

$('model-select').addEventListener('change', async (e) => {
  const v = e.target.value;
  if (!S.current) { S.newModel = v; return; }
  try {
    const r = await api('PATCH', '/threads/' + S.current, { model: v });
    S.thread = r.thread;
  } catch (err) { toast(err.message); renderHeader(); }
});

$('thread-title').addEventListener('click', renameThread);

async function renameThread() {
  if (!S.current) return;
  const t = prompt('Thread title', S.thread?.title || '');
  if (t == null) return;
  try { await api('PATCH', '/threads/' + S.current, { title: t }); } catch (e) { toast(e.message); }
}

async function deleteThread() {
  if (!S.current) return;
  if (!confirm('Delete this thread? Memories learned from it are deleted too.')) return;
  try { await api('DELETE', '/threads/' + S.current); } catch (e) { toast(e.message); }
}

$('thread-menu').addEventListener('click', (e) => { e.stopPropagation(); $('menu').classList.toggle('hidden'); });
document.addEventListener('click', () => $('menu').classList.add('hidden'));
$('menu').addEventListener('click', (e) => {
  const act = e.target.dataset.act;
  if (act === 'rename') renameThread();
  if (act === 'delete') deleteThread();
});
$('new-chat').addEventListener('click', () => { newChat(); closeSide(); });
$('toggle-side').addEventListener('click', () => $('app').classList.toggle('side-open'));
$('scrim').addEventListener('click', closeSide);

// ------------------------------------------------------------ messages

function toolResultFor(callId) {
  return S.messages.find((m) => m.role === 'tool' && m.tool_call_id === callId);
}

function attachmentsFor(msgId) {
  return S.attachments.filter((a) => a.message_id === msgId);
}

function attEl(a) {
  if (a.mime.startsWith('image/') && a.mime !== 'image/svg+xml') {
    return el('a', { class: 'att', href: '/api/attachments/' + a.id, target: '_blank', rel: 'noopener' },
      el('img', { src: '/api/attachments/' + a.id, alt: a.name }));
  }
  return el('a', { class: 'att', href: '/api/attachments/' + a.id }, '📄 ', a.name, ' · ', fmtBytes(a.size));
}

function msgEl(m) {
  if (m.role === 'user') {
    return el('div', { class: 'msg user', id: 'msg-' + m.id },
      el('div', { class: 'bubble' }, m.content, el('div', {}, attachmentsFor(m.id).map(attEl))));
  }
  if (m.role !== 'assistant') return null;
  const box = el('div', { class: 'msg assistant' + (m.status === 'error' ? ' error' : ''), id: 'msg-' + m.id });
  const content = el('div', { class: 'content' + (m.status === 'streaming' ? ' cursor' : '') });
  content.innerHTML = md(m.content || '');
  box.append(content);
  for (const c of m.tool_calls || []) {
    const r = toolResultFor(c.id);
    const ok = r ? r.meta?.ok !== false : null;
    const line = el('div', { class: 'tool' + (ok === false ? ' fail' : ''), id: 'tool-' + c.id },
      (ok === null ? '⋯ ' : ok ? '✓ ' : '✗ ') + c.name + ' ' + argSummary(c.arguments));
    if (ok === false && r) line.title = r.content.slice(0, 500);
    box.append(line);
  }
  if (m.status === 'error') box.append(el('div', { class: 'meta' }, 'Error: ' + (m.meta?.error || 'generation failed')));
  if (m.status === 'cancelled') box.append(el('div', { class: 'meta' }, '(stopped)'));
  if (m.status === 'complete' && !(m.tool_calls || []).length && m.meta) {
    const secs = ((m.meta.provider_ms || 0) / 1000).toFixed(1);
    box.append(el('div', { class: 'meta' }, `${m.model || ''} · ${secs}s`));
  }
  return box;
}

function argSummary(a) {
  if (!a || typeof a !== 'object') return '';
  return Object.entries(a).map(([k, v]) => `${k}=${String(typeof v === 'string' ? v : JSON.stringify(v)).slice(0, 80)}`).join(' ').slice(0, 200);
}

function renderMessages() {
  const box = $('messages');
  const nodes = S.messages.map(msgEl).filter(Boolean);
  for (const a of S.approvals.values()) if (a.thread_id === S.current) nodes.push(approvalEl(a));
  const empty = $('empty');
  empty.classList.toggle('hidden', nodes.length > 0);
  box.replaceChildren(empty, ...nodes);
  box.scrollTop = box.scrollHeight;
}

let rafPending = new Set();
function rerenderMessage(id) {
  rafPending.add(id);
  if (rafPending.size > 1) return;
  requestAnimationFrame(() => {
    const box = $('messages');
    const nearBottom = box.scrollHeight - box.scrollTop - box.clientHeight < 80;
    for (const mid of rafPending) {
      const m = S.messages.find((x) => x.id === mid);
      const old = $('msg-' + mid);
      const fresh = m && msgEl(m);
      if (old && fresh) old.replaceWith(fresh);
      else if (fresh) box.append(fresh);
    }
    rafPending = new Set();
    if (nearBottom) box.scrollTop = box.scrollHeight;
  });
}

function approvalEl(a) {
  const decide = async (approve) => {
    try { await api('POST', '/approvals/' + a.approval_id, { approve }); } catch (e) { toast(e.message); }
  };
  return el('div', { class: 'approval', id: 'approval-' + a.approval_id },
    el('div', {}, el('strong', {}, 'Allow ', a.tool, '?'),
      a.client && a.client !== 'web' ? el('span', { class: 'muted small' }, ` (requested from ${a.client})`) : null),
    el('pre', {}, JSON.stringify(a.arguments, null, 2)),
    el('div', { class: 'row' },
      el('button', { class: 'primary', onclick: () => decide(true) }, 'Allow'),
      el('button', { onclick: () => decide(false) }, 'Deny')));
}

async function refreshApprovals() {
  // Approval details arrive by SSE; after a reload we only know IDs, so
  // pending approvals from before the reload are shown without arguments.
  try {
    const v = await api('GET', '/approvals');
    for (const a of v.approvals) if (!S.approvals.has(a.approval_id)) S.approvals.set(a.approval_id, { ...a, tool: 'a tool', arguments: {} });
    renderMessages();
  } catch {}
}

// ------------------------------------------------------------ composer

function renderComposer() {
  $('send').classList.toggle('hidden', S.generating);
  $('stop').classList.toggle('hidden', !S.generating);
  $('attach-list').replaceChildren(...S.pending.map((p, i) =>
    el('span', { class: 'att' }, p.name, el('button', { type: 'button', title: 'Remove', onclick: () => { S.pending.splice(i, 1); renderComposer(); } }, '✕'))));
}

const input = $('input');
input.addEventListener('input', () => {
  input.style.height = 'auto';
  input.style.height = Math.min(input.scrollHeight, window.innerHeight * 0.4) + 'px';
});
input.addEventListener('keydown', (e) => {
  if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) {
    e.preventDefault();
    $('composer').requestSubmit();
  }
});

async function ensureThread() {
  if (S.current) return S.current;
  const v = await api('POST', '/threads', { model: S.newModel || null });
  S.current = v.thread.id;
  S.thread = v.thread;
  S.messages = [];
  S.attachments = [];
  return S.current;
}

$('file-input').addEventListener('change', async (e) => {
  const files = [...e.target.files];
  e.target.value = '';
  try {
    const id = await ensureThread();
    for (const f of files) {
      const v = await api('POST', `/threads/${id}/attachments?name=${encodeURIComponent(f.name)}&mime=${encodeURIComponent(f.type || '')}`, f);
      S.pending.push(v);
      S.attachments.push({ ...v, message_id: null });
    }
    renderAll();
  } catch (err) { toast(err.message); }
});

$('composer').addEventListener('submit', async (e) => {
  e.preventDefault();
  const text = input.value.trim();
  if ((!text && !S.pending.length) || S.generating) return;
  try {
    const id = await ensureThread();
    const atts = S.pending.map((p) => p.id);
    input.value = '';
    input.dispatchEvent(new Event('input'));
    const r = await api('POST', `/threads/${id}/messages`, { content: text, client: 'web', attachments: atts });
    S.pending = [];
    // Events may already have delivered these; merge by id.
    for (const m of [r.user_message, r.assistant_message]) upsertMessage(m);
    for (const a of atts) { const x = S.attachments.find((y) => y.id === a); if (x) x.message_id = r.user_message.id; }
    S.generating = true;
    renderAll();
  } catch (err) {
    toast(err.message);
    if (!input.value) input.value = text;
  }
});

$('stop').addEventListener('click', () => { if (S.current) api('POST', `/threads/${S.current}/stop`).catch((e) => toast(e.message)); });

function upsertMessage(m) {
  const i = S.messages.findIndex((x) => x.id === m.id);
  if (i >= 0) S.messages[i] = m; else S.messages.push(m);
}

// ------------------------------------------------------------ events

function connectEvents() {
  S.es?.close();
  const es = new EventSource('/api/events');
  S.es = es;
  let dropped = false;
  es.onerror = () => { dropped = true; };
  es.onmessage = (e) => {
    let v;
    try { v = JSON.parse(e.data); } catch { return; }
    if (v.type === 'hello' && dropped) { dropped = false; resync(); return; }
    handleEvent(v);
  };
}

async function resync() {
  try {
    await loadThreads();
    if (S.current) await openThread(S.current);
  } catch {}
}

function handleEvent(v) {
  const here = v.thread_id && v.thread_id === S.current;
  switch (v.type) {
    case 'resync': resync(); break;
    case 'thread.created': case 'thread.updated': {
      const i = S.threads.findIndex((t) => t.id === v.thread.id);
      if (i >= 0) S.threads[i] = { ...S.threads[i], ...v.thread }; else S.threads.unshift(v.thread);
      S.threads.sort((a, b) => b.updated_at - a.updated_at);
      if (here) { S.thread = v.thread; renderHeader(); }
      renderThreads();
      break;
    }
    case 'thread.deleted':
      S.threads = S.threads.filter((t) => t.id !== v.thread_id);
      for (const [k, a] of S.approvals) if (a.thread_id === v.thread_id) S.approvals.delete(k);
      if (here) { newChat(); toast('This thread was deleted'); } else renderThreads();
      break;
    case 'threads.cleared':
      S.threads = [];
      S.approvals.clear();
      newChat();
      break;
    case 'message.created': {
      const t = S.threads.find((x) => x.id === v.thread_id);
      if (t) { t.updated_at = v.ts; if (v.message.status === 'streaming') t.generating = true; S.threads.sort((a, b) => b.updated_at - a.updated_at); renderThreads(); }
      else loadThreads().catch(() => {});
      if (!here) break;
      upsertMessage(v.message);
      if (v.message.role === 'user') { api('GET', `/threads/${S.current}/messages`).then((m) => { S.attachments = m.attachments; renderMessages(); }).catch(() => {}); }
      if (v.message.status === 'streaming') { S.generating = true; renderComposer(); }
      rerenderMessage(v.message.id);
      break;
    }
    case 'message.delta': {
      if (!here) break;
      const m = S.messages.find((x) => x.id === v.message_id);
      if (m) { m.content += v.delta; rerenderMessage(m.id); }
      break;
    }
    case 'message.completed': {
      if (here) {
        upsertMessage(v.message);
        const done = v.message.role === 'assistant' && (v.message.status !== 'complete' || !(v.message.tool_calls || []).length);
        if (done) { S.generating = false; renderComposer(); }
        rerenderMessage(v.message.id);
      }
      const t = S.threads.find((x) => x.id === v.thread_id);
      if (t && v.message.role === 'assistant' && (v.message.status !== 'complete' || !(v.message.tool_calls || []).length)) { t.generating = false; renderThreads(); }
      break;
    }
    case 'tool.started': case 'tool.completed':
      if (here) {
        api('GET', `/threads/${S.current}/messages`).then((m) => {
          if (S.current !== v.thread_id) return;
          S.messages = m.messages;
          rerenderMessage(v.message_id);
        }).catch(() => {});
      }
      break;
    case 'tool.approval_required':
      S.approvals.set(v.approval_id, v);
      if (here) renderMessages();
      else toast(`Approval needed: ${v.tool} (in another thread)`);
      break;
    case 'tool.approval_resolved':
      S.approvals.delete(v.approval_id);
      $('approval-' + v.approval_id)?.remove();
      break;
    case 'config.updated':
      loadModels().then(renderHeader).catch(() => {});
      break;
  }
}

// ------------------------------------------------------------ settings

async function loadModels() { S.models = await api('GET', '/models'); }

$('open-settings').addEventListener('click', () => { $('settings').classList.remove('hidden'); showTab('general'); closeSide(); });
$('close-settings').addEventListener('click', () => $('settings').classList.add('hidden'));
$('settings').addEventListener('click', (e) => { if (e.target.id === 'settings') $('settings').classList.add('hidden'); });
document.addEventListener('keydown', (e) => { if (e.key === 'Escape') $('settings').classList.add('hidden'); });
$('settings-tabs').addEventListener('click', (e) => { if (e.target.dataset.tab) showTab(e.target.dataset.tab); });

function field(label, input) { return el('div', { class: 'field' }, el('label', {}, label), input); }

async function showTab(name) {
  for (const b of $('settings-tabs').children) b.classList.toggle('active', b.dataset.tab === name);
  const box = $('settings-content');
  box.replaceChildren(el('p', { class: 'muted' }, 'Loading…'));
  try {
    const node = await TABS[name]();
    box.replaceChildren(...[node].flat());
  } catch (e) {
    box.replaceChildren(el('p', { class: 'error' }, e.message));
  }
}

function applyTheme(mode) {
  if (mode === 'light' || mode === 'dark') document.documentElement.dataset.theme = mode;
  else delete document.documentElement.dataset.theme;
}

function webSearchForm(c) {
  const prov = el('select', {}, el('option', { value: '' }, 'disabled'), el('option', { value: 'brave' }, 'Brave Search API'), el('option', { value: 'searxng' }, 'SearXNG (self-hosted)'));
  prov.value = c.tools.web.search_provider;
  const sx = el('input', { type: 'url', placeholder: 'https://searx.example.com', value: c.tools.web.searxng_url });
  const key = el('input', { type: 'password', placeholder: c.secrets_set.includes('brave.api_key') ? 'Brave API key is set — enter to replace' : 'Brave API key' });
  const save = async () => {
    try {
      await api('PATCH', '/config', { 'tools.web.search_provider': prov.value, 'tools.web.searxng_url': sx.value });
      if (key.value) await api('PUT', '/secrets/brave.api_key', { value: key.value });
      toast('Saved'); showTab('tools');
    } catch (e) { toast(e.message); }
  };
  return el('div', {}, field('Search provider', prov), field('SearXNG URL', sx), field('Brave API key', key),
    el('button', { class: 'primary', onclick: save }, 'Save web search'),
    el('p', { class: 'small muted' }, 'Fetched pages are treated as untrusted data. web_fetch asks before running by default and refuses private/local network addresses.'));
}

const TABS = {
  async general() {
    const c = await api('GET', '/config');
    S.config = c;
    const inp = (v, type = 'text') => el('input', { type, value: v ?? '' });
    const name = inp(c.agent.name), lang = inp(c.agent.language), bind = inp(c.server.bind), port = inp(c.server.port, 'number'), pub = inp(c.server.public_url, 'url');
    const sys = el('textarea', { rows: 6 }); sys.value = c.agent.system_prompt;
    const app = el('select', {}, ...['system', 'light', 'dark'].map((m) => el('option', { value: m }, m))); app.value = c.agent.appearance;
    const save = async () => {
      try {
        await api('PATCH', '/config', {
          'agent.name': name.value, 'agent.language': lang.value, 'agent.system_prompt': sys.value,
          'agent.appearance': app.value, 'server.bind': bind.value, 'server.port': Number(port.value), 'server.public_url': pub.value,
        });
        applyTheme(app.value);
        const restart = bind.value !== c.server.bind || Number(port.value) !== c.server.port;
        toast(restart ? 'Saved. Restart the server to apply address/port changes (`alice restart`).' : 'Saved');
      } catch (e) { toast(e.message); }
    };
    return [
      el('h3', {}, 'Agent'),
      field('Agent name', name), field('System prompt (kept when conversations are deleted)', sys), field('Default language', lang),
      el('h3', {}, 'Server'),
      field('Bind address (127.0.0.1 = this machine only)', bind), field('Port', port), field('Public URL (Tailscale / reverse proxy)', pub),
      el('h3', {}, 'Appearance'), field('Theme', app),
      el('div', { class: 'row' }, el('button', { class: 'primary', onclick: save }, 'Save'),
        el('button', { onclick: async () => { await api('POST', '/logout'); showLogin(); } }, 'Sign out')),
    ];
  },

  async models() {
    const v = await api('GET', '/models');
    S.models = v;
    const list = el('div', { class: 'list' });
    for (const p of v.providers) {
      const cap = p.capabilities || {};
      const keyInput = el('input', { type: 'password', placeholder: p.has_key ? 'API key is set — enter to replace' : 'API key' });
      const needsKey = ['openai', 'anthropic', 'gemini', 'openai-compatible'].includes(p.kind);
      list.append(el('div', { class: 'item' },
        el('div', { class: 'grow' },
          el('strong', {}, p.id), ' ', el('span', { class: 'muted small' }, `${p.kind} · ${p.base_url}`),
          el('div', { class: 'small muted' }, `tools: ${cap.tool_calling} · images: ${cap.image_input} · streaming: ${cap.streaming} · auth: ${cap.authentication}`),
          ...p.models.map((m) => {
            const id = `${p.id}/${m}`;
            const res = el('span', { class: 'small muted' });
            return el('div', { class: 'row' },
              el('span', {}, (id === v.default ? '★ ' : '') + m),
              id === v.default ? null : el('button', { class: 'ghost small', onclick: async () => { await api('PUT', '/models/default', { model: id }); showTab('models'); } }, 'Make default'),
              el('button', { class: 'ghost small', onclick: async () => {
                res.textContent = 'testing…';
                const r = await api('POST', '/models/test', { model: id }).catch((e) => ({ ok: false, error: e.message }));
                res.textContent = r.ok ? `ok · first token ${r.ttft_ms ?? '-'} ms · ${r.total_ms} ms` : `failed: ${r.error}`;
              } }, 'Test'), res);
          }),
          ['codex-cli', 'gemini-cli'].includes(p.kind)
            ? el('div', { class: 'small muted' }, `Subscription login runs in the official CLI on the server machine: alice model login ${p.id}`)
            : null,
          needsKey ? el('div', { class: 'row' }, keyInput, el('button', { onclick: async () => {
            if (!keyInput.value) return;
            await api('PUT', `/secrets/provider.${p.id}.api_key`, { value: keyInput.value });
            toast('API key saved'); showTab('models');
          } }, 'Save key')) : null),
        el('button', { class: 'danger', onclick: async () => {
          if (!confirm(`Remove provider ${p.id} and its API key?`)) return;
          await api('DELETE', '/providers/' + p.id); showTab('models');
        } }, 'Remove')));
    }
    if (!v.providers.length) list.append(el('div', { class: 'item muted' }, 'No providers yet.'));

    const kind = el('select', {}, ...['openai', 'anthropic', 'gemini', 'openai-compatible', 'ollama', 'codex-cli', 'gemini-cli'].map((k) => el('option', { value: k }, k)));
    const id = el('input', { type: 'text', placeholder: 'id (e.g. openai)' });
    const base = el('input', { type: 'text', placeholder: 'Base URL (optional; required for openai-compatible)' });
    const models = el('input', { type: 'text', placeholder: 'model names, comma separated' });
    const key = el('input', { type: 'password', placeholder: 'API key (if needed)' });
    kind.addEventListener('change', () => {
      if (!id.value) id.placeholder = kind.value;
      if (kind.value === 'anthropic' && !models.value) models.value = 'claude-opus-5-5';
      if (['codex-cli', 'gemini-cli'].includes(kind.value) && !models.value) models.value = 'default';
    });
    const add = async () => {
      try {
        await api('POST', '/providers', {
          id: id.value || kind.value, kind: kind.value, base_url: base.value,
          models: models.value.split(',').map((s) => s.trim()).filter(Boolean), api_key: key.value || null,
        });
        toast('Provider added'); showTab('models');
      } catch (e) { toast(e.message); }
    };
    return [
      el('h3', {}, 'Providers'), list,
      el('h3', {}, 'Add provider'),
      el('div', { class: 'field' }, kind, id, base, models, key, el('div', {}, el('button', { class: 'primary', onclick: add }, 'Add'))),
      el('p', { class: 'small muted' }, 'ChatGPT and Gemini subscriptions are used only through the vendors\' official CLIs (codex-cli / gemini-cli). They are text-only: Alice tools are not available with them.'),
    ];
  },

  async channels() {
    const c = await api('GET', '/config');
    const url = c.server.public_url || location.origin;
    return [
      el('h3', {}, 'iPhone app'),
      field('Server URL', el('input', { type: 'text', value: url, readonly: true })),
      el('p', { class: 'small muted' }, 'Sign in with the API token. For security the token is not shown here; run `alice token` on the server machine. For access outside your network use Tailscale or an HTTPS reverse proxy.'),
      el('h3', {}, 'Telegram / Discord'),
      el('p', { class: 'muted' }, 'Not available yet (roadmap Phase 5).'),
    ];
  },

  async tools() {
    const v = await api('GET', '/tools');
    const c = await api('GET', '/config');
    const list = el('div', { class: 'list' });
    for (const t of v.tools) {
      const sel = el('select', {}, ...['disabled', 'ask', 'allow'].map((p) => el('option', { value: p }, p === 'ask' ? 'ask before running' : p === 'allow' ? 'always allow' : 'disabled')));
      sel.value = t.permission;
      sel.addEventListener('change', async () => {
        try { await api('PATCH', '/tools', { [t.name]: sel.value }); toast(`${t.name}: ${sel.value}`); } catch (e) { toast(e.message); }
      });
      list.append(el('div', { class: 'item' },
        el('div', { class: 'grow' }, el('strong', {}, t.name), ' ', el('span', { class: 'muted small' }, t.group),
          el('div', { class: 'small muted' }, t.description + (t.available ? '' : ' (not configured)'))), sel));
    }
    const roots = el('textarea', { rows: 3, placeholder: 'one directory per line (default: ~/.alice/workspace)' });
    roots.value = (c.tools.fs.roots || []).join('\n');
    return [
      el('h3', {}, 'Permissions'), list,
      el('p', { class: 'small muted' }, 'Permissions can only be changed here or with `alice tool`. The model cannot change them.'),
      el('h3', {}, 'File system'),
      field('Accessible directories', roots),
      el('p', { class: 'small muted' }, 'Effective: ' + v.fs_roots.join(', ')),
      el('button', { class: 'primary', onclick: async () => {
        try {
          await api('PATCH', '/config', { 'tools.fs.roots': roots.value.split('\n').map((s) => s.trim()).filter(Boolean) });
          toast('Saved'); showTab('tools');
        } catch (e) { toast(e.message); }
      } }, 'Save directories'),
      el('h3', {}, 'Web search'),
      webSearchForm(c),
      el('h3', {}, 'Google account'),
      el('p', { class: 'muted' }, 'Not available yet (roadmap Phase 4).'),
    ];
  },

  async memory() {
    const st = (await api('GET', '/memories/status')).status;
    const q = el('input', { type: 'text', placeholder: 'Search memories' });
    const list = el('div', { class: 'list' });
    const detail = el('div');
    const load = async () => {
      const v = await api('GET', '/memories?limit=500' + (q.value ? '&q=' + encodeURIComponent(q.value) : ''));
      list.replaceChildren(...(v.memories.length ? v.memories.map((m) =>
        el('div', { class: 'item' },
          el('div', { class: 'grow' }, m.content, el('div', { class: 'small muted' }, `${m.key} · ${m.source_count} source(s) · ${fmtTime(m.updated_at)}`)),
          el('button', { class: 'ghost small', onclick: () => showSources(m.id) }, 'Sources')))
        : [el('div', { class: 'item muted' }, 'No memories.')]));
    };
    const showSources = async (id) => {
      const v = await api('GET', '/memories/' + id);
      detail.replaceChildren(el('h3', {}, 'Sources'), el('div', { class: 'list' }, ...v.sources.map((s) =>
        el('div', { class: 'item' }, el('div', { class: 'grow' }, s.content,
          el('div', { class: 'small muted' }, `${s.thread_title || s.thread_id} · ${fmtTime(s.created_at)}`)),
          el('button', { class: 'ghost small', onclick: () => { $('settings').classList.add('hidden'); openThread(s.thread_id); } }, 'Open thread')))));
    };
    q.addEventListener('input', () => { clearTimeout(q.t); q.t = setTimeout(load, 200); });
    await load();
    return [
      el('div', { class: 'stat' },
        el('div', {}, el('b', {}, st.memories), 'memories'), el('div', {}, el('b', {}, st.sources), 'sources'),
        el('div', {}, el('b', {}, st.threads), 'threads'), el('div', {}, el('b', {}, fmtBytes(st.db_bytes)), 'database'),
        el('div', {}, el('b', {}, fmtBytes(st.attachments_bytes)), 'attachments')),
      q, list, detail,
      el('h3', {}, 'Delete'),
      el('div', { class: 'row' },
        el('button', { onclick: async () => {
          if (!confirm('Delete all memories? Conversations are kept.')) return;
          await api('DELETE', '/memories', { confirm: 'clear-memories' }); showTab('memory');
        } }, 'Delete all memories'),
        el('button', { class: 'danger', onclick: async () => {
          const a = prompt('This deletes ALL threads and everything Alice learned from them. Settings, models and API keys are kept.\nType delete-all to confirm.');
          if (a !== 'delete-all') return;
          await api('DELETE', '/threads', { confirm: 'delete-all' }); showTab('memory');
        } }, 'Delete all conversations and memories')),
      el('p', { class: 'small muted' }, 'Deletion covers Alice\'s database, search index and attachments. Data already sent to LLM providers or posted on Telegram/Discord is outside Alice\'s control.'),
    ];
  },

  async system() {
    const s = await api('GET', '/status');
    const logs = await api('GET', '/logs?lines=200');
    const up = s.uptime_secs;
    return [
      el('div', { class: 'stat' },
        el('div', {}, el('b', {}, s.version), 'version'),
        el('div', {}, el('b', {}, `${Math.floor(up / 3600)}h ${Math.floor(up / 60) % 60}m`), 'uptime'),
        el('div', {}, el('b', {}, (s.rss_kb / 1024).toFixed(1) + ' MB'), 'memory'),
        el('div', {}, el('b', {}, s.cpu_percent + '%'), 'cpu'),
        el('div', {}, el('b', {}, fmtBytes(s.db_bytes)), 'database'),
        el('div', {}, el('b', {}, s.counts.threads), 'threads'),
        el('div', {}, el('b', {}, s.counts.messages), 'messages')),
      el('p', { class: 'small muted' }, `${s.os}/${s.arch} · ${s.bind} · data ${s.home}`),
      el('h3', {}, 'Logs'), el('pre', { class: 'log' }, logs.lines || '(empty)'),
      el('button', { onclick: () => showTab('system') }, 'Refresh'),
    ];
  },
};

// ------------------------------------------------------------ start

async function start() {
  try {
    await loadModels();
  } catch (e) {
    if (e.message !== 'unauthorized') toast(e.message);
    return;
  }
  $('app').classList.remove('hidden');
  try {
    const c = await api('GET', '/config');
    applyTheme(c.agent.appearance);
    S.profile = c.profile;
    if (c.profile && c.profile !== 'default') {
      document.title = `Alice · ${c.profile}`;
      $('new-chat').before(el('div', { class: 'small muted profile-tag' }, `Profile: ${c.profile}`));
    }
  } catch {}
  connectEvents();
  await loadThreads();
  let last = null;
  try { last = localStorage.getItem('alice.thread'); } catch {}
  if (last && S.threads.some((t) => t.id === last)) await openThread(last); else newChat();
}

start();
