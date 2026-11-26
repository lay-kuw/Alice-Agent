// Alice Web GUI: Calendar, Mail and Tasks (same data and look as the iPhone app).
// Uses the helpers from app.js (el, api, t, toast, $, LANG, pastel).
// Content from Google is only inserted as text; HTML mail is shown in a
// sandboxed frame served with its own CSP (no scripts, no remote loads by default).
'use strict';

Object.assign(I18N.ja, {
  'v.agents': 'エージェント', 'ag.intro': '時間と繰り返しを決めて、Alice に自動でお願いできます。結果は指定したチャットに届きます。',
  'ag.new': '新しいエージェント', 'ag.edit': 'エージェントを編集', 'ag.name': '名前', 'ag.prompt': 'お願いすること',
  'ag.promptHint': '例: 新しいメールがあれば要約して教えて。なければ何もせず終了して', 'ag.start': '開始日時', 'ag.repeat': '繰り返し',
  'ag.once': '1 回だけ', 'ag.min': '分ごと', 'ag.hour': '時間ごと', 'ag.day': '日ごと', 'ag.week': '週ごと', 'ag.every': '間隔',
  'ag.to': '送り先のチャット', 'ag.newThread': '新しいチャット（最初の実行で作成）', 'ag.newChat': '新しいチャット', 'ag.on': '有効', 'ag.run': '今すぐ実行', 'ag.ran': '実行しました',
  'ag.next': '次回 {t}', 'ag.done': '実行済み', 'ag.off': '停止中', 'ag.last': '前回 {t}', 'ag.none': 'まだエージェントがいません 🌙',
  'ag.everyN': '{n}{u}', 'ag.u.min': '分ごと', 'ag.u.hour': '時間ごと', 'ag.u.day': '日ごと', 'ag.u.week': '週ごと',
  'ag.mode.every': '繰り返し', 'ag.mode.times': '時刻指定', 'ag.addTime': '＋ 時刻を追加', 'ag.timesEach': '毎日 {t}',
  'ag.hist': '履歴', 'ag.histNone': 'まだ実行されていません', 'ag.manual': '手動', 'ag.st.ok': '完了', 'ag.st.waiting': '承認待ち', 'ag.goneChat': '（チャットなし）',
  'ag.st.interrupted': '中断（再起動）', 'ag.st.running': '実行中…', 'ag.st.quiet': '報告なし', 'ag.st.stopped': '中止', 'ag.st.busy': '待機中', 'ag.st.error': 'エラー',
  'ag.from': '{t} から', 'ag.daily': '毎日 {t}', 'ag.weekly': '毎週 {d} {t}', 'ag.at': '{t} に 1 回', 'ag.minEvery': '繰り返しは {n} 分以上にしてください',
  'nt.on': '通知オン', 'nt.off': '通知オフ', 'nt.denied': 'ブラウザで通知がブロックされています', 'nt.insecure': '通知は HTTPS か localhost で開いたときだけ使えます',
  'v.chat': 'チャット', 'v.calendar': 'カレンダー', 'v.mail': 'メール', 'v.tasks': 'タスク',
  'g.setup': 'Alice サーバーで Google を接続してください', 'g.off': '「{t}」がオフになっています',
  'g.offNote': '設定 → スキル で「{t}」をオンにして、再接続してください。', 'g.unreach': 'Alice に接続できません', 'g.retry': 'もう一度確認',
  'g.refreshFail': '更新できませんでした', today: '今日', tomorrow: '明日', allDay: '終日', week: '週', month: '月',
  'cal.nothing': '予定はありません ☕️', 'cal.new': '新しい予定', 'cal.title': 'タイトル', 'cal.allDay': '終日',
  'cal.start': '開始', 'cal.end': '終了', 'cal.color': '色', 'cal.none': 'なし', 'cal.notes': 'メモ', 'add': '追加', cancel: 'キャンセル',
  'delete': '削除', edit: '編集', 'cal.open': 'Google カレンダーで開く',
  inbox: '受信トレイ', 'mail.zero': 'すべて読みました ✨', 'mail.pick': 'メールを選んでください', reply: '返信', compose: '新規メール',
  'mail.read': '既読にする', 'mail.unread': '未読にする', 'mail.images': '画像を表示', 'mail.imagesNote': '送信者に開封を知られないよう、外部の画像は読み込んでいません。',
  to: '宛先', cc: 'Cc', subject: '件名', send: '送信', 'mail.sent': '送信しました', more: 'もっと見る', back: '戻る',
  'tasks.all': 'すべて', 'tasks.done': '完了 ({n})', 'tasks.allDone': 'すべて完了！ 🌷', 'tasks.new': '新しいタスク', 'tasks.edit': 'タスクを編集',
  'tasks.list': 'リスト', 'tasks.due': '期日', 'tasks.nextWeek': '来週', 'tasks.notes': 'メモ', save: '保存', 'no.title': '（タイトルなし）',
});
Object.assign(I18N.en, {
  'v.agents': 'Agents', 'ag.intro': 'Pick a time and how often, and Alice does it on her own. Results arrive in the chat you choose.',
  'ag.new': 'New agent', 'ag.edit': 'Edit agent', 'ag.name': 'Name', 'ag.prompt': 'What to do',
  'ag.promptHint': 'e.g. If there is new mail, summarize it for me. If not, do nothing', 'ag.start': 'Starts', 'ag.repeat': 'Repeat',
  'ag.once': 'Once', 'ag.min': 'Minutes', 'ag.hour': 'Hours', 'ag.day': 'Days', 'ag.week': 'Weeks', 'ag.every': 'Every',
  'ag.to': 'Send to chat', 'ag.newThread': 'New chat (made on the first run)', 'ag.newChat': 'New chat', 'ag.on': 'Enabled', 'ag.run': 'Run now', 'ag.ran': 'Started',
  'ag.next': 'Next {t}', 'ag.done': 'Done', 'ag.off': 'Paused', 'ag.last': 'Last {t}', 'ag.none': 'No agents yet 🌙',
  'ag.everyN': 'Every {n} {u}', 'ag.u.min': 'min', 'ag.u.hour': 'h', 'ag.u.day': 'days', 'ag.u.week': 'weeks',
  'ag.mode.every': 'Repeat', 'ag.mode.times': 'Set times', 'ag.addTime': '＋ Add a time', 'ag.timesEach': 'Every day at {t}',
  'ag.hist': 'History', 'ag.histNone': 'No runs yet', 'ag.manual': 'manual', 'ag.st.ok': 'Done', 'ag.st.waiting': 'Waiting for approval', 'ag.goneChat': '(no chat)',
  'ag.st.interrupted': 'Interrupted (restart)', 'ag.st.running': 'Running…', 'ag.st.quiet': 'Nothing to report', 'ag.st.stopped': 'Stopped', 'ag.st.busy': 'Waiting', 'ag.st.error': 'Error',
  'ag.from': 'from {t}', 'ag.daily': 'Every day at {t}', 'ag.weekly': 'Every {d} at {t}', 'ag.at': 'Once at {t}', 'ag.minEvery': 'Repeat at least every {n} minutes',
  'nt.on': 'Notifications on', 'nt.off': 'Notifications off', 'nt.denied': 'Notifications are blocked in this browser', 'nt.insecure': 'Notifications need HTTPS or localhost',
  'v.chat': 'Chat', 'v.calendar': 'Calendar', 'v.mail': 'Mail', 'v.tasks': 'Tasks',
  'g.setup': 'Connect Google on your Alice server', 'g.off': '{t} is turned off',
  'g.offNote': 'Turn on “{t}” in Settings → Skills, then reconnect.', 'g.unreach': "Can't reach Alice", 'g.retry': 'Check again',
  'g.refreshFail': "Couldn't refresh", today: 'Today', tomorrow: 'Tomorrow', allDay: 'All day', week: 'Week', month: 'Month',
  'cal.nothing': 'Nothing planned ☕️', 'cal.new': 'New event', 'cal.title': 'Title', 'cal.allDay': 'All day',
  'cal.start': 'Starts', 'cal.end': 'Ends', 'cal.color': 'Color', 'cal.none': 'None', 'cal.notes': 'Notes', add: 'Add', cancel: 'Cancel',
  delete: 'Delete', edit: 'Edit', 'cal.open': 'Open in Google Calendar',
  inbox: 'Inbox', 'mail.zero': 'All caught up ✨', 'mail.pick': 'Pick a message', reply: 'Reply', compose: 'New message',
  'mail.read': 'Mark as read', 'mail.unread': 'Mark as unread', 'mail.images': 'Load images', 'mail.imagesNote': 'Remote images are not loaded, so the sender cannot tell you opened this.',
  to: 'To', cc: 'Cc', subject: 'Subject', send: 'Send', 'mail.sent': 'Sent', more: 'More', back: 'Back',
  'tasks.all': 'All', 'tasks.done': 'Completed ({n})', 'tasks.allDone': 'All done! 🌷', 'tasks.new': 'New task', 'tasks.edit': 'Edit task',
  'tasks.list': 'List', 'tasks.due': 'Due date', 'tasks.nextWeek': 'Next week', 'tasks.notes': 'Notes', save: 'Save', 'no.title': '(No title)',
});

const G = {
  view: 'chat', status: null, statusErr: null,
  events: new Map(), loaded: new Set(), sel: null, anchor: null, calMode: 'month',
  inbox: [], next: null, inboxLoaded: false, mail: null, msg: null, images: false,
  lists: [], tasksLoaded: false, listId: '', showDone: false, ticking: new Set(),
};
try { G.calMode = localStorage.getItem('alice.cal.mode') || 'month'; G.listId = localStorage.getItem('alice.tasks.list') || ''; } catch {}

// ------------------------------------------------------------ small helpers

const SVG = {
  plus: 'M12 5v14M5 12h14', left: 'm14.5 6-6 6 6 6', right: 'm9.5 6 6 6-6 6', reply: 'M9.5 7 4.5 12l5 5M5 12h9a5.5 5.5 0 0 1 5.5 5.5V19',
  refresh: 'M19.5 12a7.5 7.5 0 1 1-2.2-5.3M19.5 4v4.5H15', compose: 'M4.5 19.5h4l10-10-4-4-10 10zM13 7l4 4',
  open: 'M4 9.5 12 4l8 5.5V19a1.5 1.5 0 0 1-1.5 1.5h-13A1.5 1.5 0 0 1 4 19zM4 9.5l8 5.5 8-5.5', mail: 'M3.5 6.5h17v11h-17zM4 7l8 6 8-6',
  more: 'M6 12h.01M12 12h.01M18 12h.01', image: 'M4 5.5h16v13H4zM4 15l4.5-4.5 4 4 2.5-2.5L20 17M15.5 9.5h.01', back: 'm14.5 6-6 6 6 6',
  check: 'm5.5 12.5 4 4 9-9', grid: 'M4 5h16v14H4zM4 10h16M4 15h16M9.3 5v14M14.6 5v14', rows: 'M4 6h16v12H4zM9.3 6v12M14.6 6v12',
};
function icon(name) {
  const ns = 'http://www.w3.org/2000/svg';
  const s = document.createElementNS(ns, 'svg');
  s.setAttribute('viewBox', '0 0 24 24');
  s.setAttribute('class', 'ic');
  const p = document.createElementNS(ns, 'path');
  p.setAttribute('d', SVG[name]);
  s.append(p);
  return s;
}
const iconBtn = (name, title, onclick, cls) => el('button', { class: 'ghost icon ' + (cls || ''), title, 'aria-label': title, onclick }, icon(name));
const css = (e, props) => { for (const [k, v] of Object.entries(props)) e.style.setProperty(k, v); return e; };

const COLORS = [
  ['1', 'Lavender', '#a3adee'], ['2', 'Sage', '#8cd6b3'], ['3', 'Grape', '#c799eb'], ['4', 'Flamingo', '#f7a1a8'],
  ['5', 'Banana', '#fad673'], ['6', 'Tangerine', '#ffb085'], ['7', 'Peacock', '#78c4f5'], ['8', 'Graphite', '#b3b3bd'],
  ['9', 'Blueberry', '#8599e8'], ['10', 'Basil', '#6bbf94'], ['11', 'Tomato', '#f07885'],
];
const eventColor = (e) => (COLORS.find((c) => c[0] === e.color_id) || [])[2] || pastel(e.id);

const locale = () => (LANG === 'ja' ? 'ja-JP' : undefined);
const day0 = (d) => new Date(d.getFullYear(), d.getMonth(), d.getDate());
const addDays = (d, n) => new Date(d.getFullYear(), d.getMonth(), d.getDate() + n);
const sameDay = (a, b) => a && b && a.getFullYear() === b.getFullYear() && a.getMonth() === b.getMonth() && a.getDate() === b.getDate();
const pad = (n) => String(n).padStart(2, '0');
const ymd = (d) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
const slash = (d) => `${d.getFullYear()}/${pad(d.getMonth() + 1)}/${pad(d.getDate())}`;
const parseDay = (s) => { const [y, m, d] = s.slice(0, 10).split('-').map(Number); return new Date(y, m - 1, d); };
const parseWhen = (s) => (s && s.length === 10 ? parseDay(s) : new Date(s));
/// RFC 3339 with the local offset (what the iPhone app sends).
function rfc3339(d) {
  const o = -d.getTimezoneOffset(), sign = o >= 0 ? '+' : '-', a = Math.abs(o);
  return `${ymd(d)}T${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}${sign}${pad(Math.floor(a / 60))}:${pad(a % 60)}`;
}
const hm = (d) => d.toLocaleTimeString(locale(), { hour: '2-digit', minute: '2-digit' });
const startOfWeek = (d) => addDays(day0(d), -d.getDay());
const startOfMonth = (d) => new Date(d.getFullYear(), d.getMonth(), 1);
const weekdayColor = (d) => (d.getDay() === 0 ? 'sun' : d.getDay() === 6 ? 'sat' : '');
function shortDate(d) {
  if (sameDay(d, new Date())) return hm(d);
  const days = (day0(new Date()) - day0(d)) / 864e5;
  if (days === 1) return LANG === 'ja' ? '昨日' : 'Yesterday';
  if (days < 7) return d.toLocaleDateString(locale(), { weekday: 'short' });
  return d.toLocaleDateString(locale(), { month: 'short', day: 'numeric' });
}
const senderName = (from) => { const m = /^\s*"?([^"<]*?)"?\s*</.exec(from || ''); return (m && m[1].trim()) || address(from); };
const address = (from) => { const m = /<([^>]+)>/.exec(from || ''); return (m ? m[1] : from || '').trim(); };
function avatar(name, key, size) {
  const a = el('span', { class: 'avatar' }, (name.trim()[0] || '?').toUpperCase());
  return css(a, { '--c': pastel(key), '--s': (size || 34) + 'px' });
}

function modal(title, body, actions) {
  const panel = $('modal');
  $('modal-title').textContent = title;
  fill($('modal-body'), body, actions ? el('div', { class: 'modal-actions' }, ...actions) : null);
  panel.classList.remove('hidden');
  return () => panel.classList.add('hidden');
}
/// replaceChildren, skipping empty parts (which it would print as "null").
const fill = (box, ...kids) => box.replaceChildren(...kids.flat().filter((k) => k != null && k !== false));
const closeModal = () => $('modal').classList.add('hidden');

/// A small pop-up menu at the pointer (right click / ⋯).
function popMenu(x, y, items) {
  const m = $('gmenu');
  m.replaceChildren(...items.map(([label, fn, danger]) => el('button', { class: danger ? 'danger' : '', onclick: () => { hideMenu(); fn(); } }, label)));
  m.classList.remove('hidden');
  m.classList.remove('pop');
  void m.offsetWidth;
  m.classList.add('pop');
  m.style.left = Math.min(x, innerWidth - m.offsetWidth - 8) + 'px';
  m.style.top = Math.min(y, innerHeight - m.offsetHeight - 8) + 'px';
}
const hideMenu = () => $('gmenu').classList.add('hidden');
document.addEventListener('click', (e) => { if (!$('gmenu').contains(e.target)) hideMenu(); });
document.addEventListener('scroll', hideMenu, true);

async function gtry(fn, quiet) {
  try { await fn(); return true; } catch (e) {
    if (e.message !== 'unauthorized') { if (quiet) G.loadErr = e.message; else toast(e.message); }
    return false;
  }
}

// ------------------------------------------------------------ views

function setView(v) {
  // #mail/<id> opens that message.
  const [base, arg] = String(v).split('/');
  v = base;
  if (v === 'mail' && arg) G.openId = decodeURIComponent(arg);
  if (!['chat', 'calendar', 'mail', 'tasks', 'agents'].includes(v)) v = 'chat';
  G.view = v;
  const app = $('app');
  app.classList.remove('view-chat', 'view-calendar', 'view-mail', 'view-tasks', 'view-agents');
  app.classList.add('view-' + v);
  for (const b of $('rail').querySelectorAll('button[data-view]')) b.classList.toggle('on', b.dataset.view === v);
  for (const id of ['chat', 'calendar', 'mail', 'tasks', 'agents']) $('view-' + id).classList.toggle('hidden', id !== v);
  try { localStorage.setItem('alice.view', v); } catch {}
  if (!arg && location.hash !== '#' + v) history.replaceState(null, '', v === 'chat' ? location.pathname : '#' + v);
  if (v !== 'chat') { closeSide(); renderView(); loadView(); }
  if (v === 'mail' && G.openId && G.inboxLoaded) { const m = G.inbox.find((x) => x.id === G.openId); G.openId = null; if (m) openMail(m); }
}
for (const b of $('rail').querySelectorAll('button[data-view]')) b.addEventListener('click', () => setView(b.dataset.view));
addEventListener('hashchange', () => setView(location.hash.slice(1) || 'chat'));

const GROUP = { calendar: 'calendar', mail: 'gmail', tasks: 'tasks' };

function renderView() {
  const v = G.view;
  if (v === 'chat') return;
  const box = $('view-' + v);
  if (v === 'agents') return renderAgents(box);
  const gate = gateEl(GROUP[v], t('v.' + v));
  if (gate) return box.replaceChildren(gate);
  if (v === 'calendar') renderCalendar(box);
  if (v === 'mail') renderMail(box);
  if (v === 'tasks') renderTasks(box);
}

async function loadView(force) {
  if (G.view === 'agents') return loadAgents();
  if (!G.status || force) {
    const ok = await gtry(async () => { G.status = await api('GET', '/google'); G.statusErr = null; }, true);
    if (!ok && !G.status) G.statusErr = G.loadErr;
    renderView();
  }
  if (!G.status || G.status.status !== 'ready') return;
  if (G.view === 'calendar' && on('calendar')) await loadCalendar(force);
  if (G.view === 'mail' && on('gmail') && (!G.inboxLoaded || force)) await loadInbox();
  if (G.view === 'tasks' && on('tasks') && (!G.tasksLoaded || force)) await loadTasks();
}
const on = (g) => !!(G.status && G.status.groups && G.status.groups[g]);

/// Setup steps when Google is not connected or the group is off; null when ready.
function gateEl(group, title) {
  const card = (heading, note, steps) => el('div', { class: 'gate' },
    el('div', { class: 'gcard gate-card' },
      el('img', { src: '/icon.png', alt: '', class: 'gate-icon' }),
      el('h2', {}, heading), note ? el('p', { class: 'muted' }, note) : null,
      steps ? el('ol', { class: 'steps' }, ...steps.map((c) => el('li', {}, el('code', {}, c)))) : null,
      el('button', { class: 'primary', onclick: () => loadView(true) }, t('g.retry'))));
  if (!G.status) return G.statusErr ? card(t('g.unreach'), G.statusErr) : el('div', { class: 'gate' }, el('div', { class: 'spinner' }));
  if (G.status.status !== 'ready') return card(t('g.setup'), G.status.detail, ['alice skill setup google-workspace', 'alice skill login google-workspace']);
  if (!on(group)) return card(t('g.off', { t: title }), t('g.offNote', { t: title }));
  return null;
}

function header(title, ...actions) {
  return el('header', { class: 'gtop' }, el('h1', { class: 'gtitle' }, title), el('div', { class: 'gactions' }, ...actions));
}

// ------------------------------------------------------------ calendar

function visibleRange() {
  const a = G.anchor;
  if (G.calMode === 'week') { const s = startOfWeek(a); return [s, addDays(s, 7)]; }
  const s = startOfWeek(startOfMonth(a));
  return [s, addDays(s, 42)];
}

async function loadCalendar(force) {
  const [from, to] = visibleRange();
  const a = addDays(from, -7), b = addDays(to, 7);
  const key = ymd(a) + '|' + ymd(b);
  if (G.loaded.has(key) && !force) return;
  await gtry(async () => {
    const r = await api('GET', `/google/calendar/events?from=${encodeURIComponent(rfc3339(a))}&to=${encodeURIComponent(rfc3339(b))}`);
    for (const [id, e] of G.events) { const s = parseWhen(e.start); if (s >= a && s < b) G.events.delete(id); }
    for (const e of r.events) G.events.set(e.id, e);
    G.loaded.add(key);
  }, true);
  if (G.view === 'calendar') renderView();
}

/// Events on a day (an event spanning days is on each), all-day first.
function eventsOn(d) {
  const s = day0(d), e = addDays(s, 1);
  return [...G.events.values()].filter((ev) => {
    const a = parseWhen(ev.start); let b = parseWhen(ev.end);
    if (!(b > a)) b = ev.all_day ? addDays(a, 1) : a;
    return a < e && (b > s || (a >= s && a < e));
  }).sort((x, y) => (x.all_day === y.all_day ? parseWhen(x.start) - parseWhen(y.start) : x.all_day ? -1 : 1));
}

function moveCal(n) {
  const a = G.anchor;
  G.anchor = G.calMode === 'week' ? addDays(a, 7 * n) : new Date(a.getFullYear(), a.getMonth() + n, 1);
  if (G.calMode === 'week') G.sel = addDays(G.sel, 7 * n);
  else if (G.sel.getMonth() !== G.anchor.getMonth() || G.sel.getFullYear() !== G.anchor.getFullYear()) G.sel = G.anchor;
  renderView(); loadCalendar();
}

function renderCalendar(box) {
  const today = day0(new Date());
  G.sel = G.sel || today;
  G.anchor = G.anchor || (G.calMode === 'week' ? G.sel : startOfMonth(G.sel));
  const title = (G.calMode === 'week' ? addDays(startOfWeek(G.anchor), 3) : G.anchor).toLocaleDateString(locale(), { year: 'numeric', month: 'long' });
  const mode = el('div', { class: 'seg' },
    el('button', { class: G.calMode === 'week' ? 'on' : '', title: t('week'), onclick: () => setMode('week') }, icon('rows')),
    el('button', { class: G.calMode === 'month' ? 'on' : '', title: t('month'), onclick: () => setMode('month') }, icon('grid')));
  const top = header(title,
    iconBtn('left', '‹', () => moveCal(-1)), el('button', { class: 'ghost', onclick: () => { G.sel = today; G.anchor = G.calMode === 'week' ? today : startOfMonth(today); renderView(); loadCalendar(); } }, t('today')),
    iconBtn('right', '›', () => moveCal(1)), mode, el('button', { class: 'primary icon round', title: t('cal.new'), onclick: () => eventSheet() }, icon('plus')));
  const [from] = visibleRange();
  const weeks = G.calMode === 'week' ? 1 : 6;
  const heads = el('div', { class: 'cal-heads' }, ...[...Array(7)].map((_, i) => {
    const d = addDays(from, i);
    return el('span', { class: weekdayColor(d) }, d.toLocaleDateString(locale(), { weekday: 'short' }));
  }));
  const grid = el('div', { class: 'cal-grid ' + G.calMode });
  for (let i = 0; i < weeks * 7; i++) {
    const d = addDays(from, i);
    const evs = eventsOn(d);
    const cell = el('button', {
      class: ['cal-day', weekdayColor(d), sameDay(d, G.sel) ? 'sel' : '', sameDay(d, today) ? 'today' : '', G.calMode === 'month' && d.getMonth() !== G.anchor.getMonth() ? 'faded' : ''].join(' '),
      onclick: () => { G.sel = d; renderView(); },
    }, el('span', { class: 'num' }, d.getDate()),
    el('span', { class: 'dots' }, ...evs.slice(0, 3).map((e) => css(el('i'), { '--c': eventColor(e) }))),
    el('span', { class: 'pills' }, ...evs.slice(0, 3).map((e) => css(el('em', {}, e.summary || t('no.title')), { '--c': eventColor(e) })),
      evs.length > 3 ? el('small', {}, '+' + (evs.length - 3)) : null));
    grid.append(cell);
  }
  // Swipe sideways on touch screens to move.
  let sx = null;
  grid.addEventListener('touchstart', (e) => { sx = e.touches[0].clientX; }, { passive: true });
  grid.addEventListener('touchend', (e) => { if (sx == null) return; const dx = e.changedTouches[0].clientX - sx; sx = null; if (Math.abs(dx) > 60) moveCal(dx < 0 ? 1 : -1); });

  const evs = eventsOn(G.sel);
  const dayTitle = sameDay(G.sel, today) ? t('today') : G.sel.toLocaleDateString(locale(), { weekday: 'long' });
  const list = evs.length ? evs.map(eventRow) : [el('div', { class: 'gcard soft-empty' }, t('cal.nothing'))];
  fill(box, top, loadBanner(),
    el('div', { class: 'cal-wrap ' + G.calMode },
      el('div', { class: 'gcard cal-card' }, heads, grid),
      el('section', { class: 'day' },
        el('h2', { class: 'day-title' }, dayTitle, el('span', { class: 'muted' }, G.sel.toLocaleDateString(locale(), { month: 'short', day: 'numeric' }))),
        el('div', { class: 'day-list' }, ...list))));
}

function setMode(m) {
  G.calMode = m;
  try { localStorage.setItem('alice.cal.mode', m); } catch {}
  G.anchor = m === 'week' ? G.sel : startOfMonth(G.sel);
  renderView(); loadCalendar();
}

function eventRow(e) {
  const s = parseWhen(e.start), en = parseWhen(e.end);
  const row = el('button', { class: 'event', onclick: () => eventDetail(e), oncontextmenu: (ev) => { ev.preventDefault(); popMenu(ev.clientX, ev.clientY, [[t('delete'), () => deleteEvent(e), true]]); } },
    css(el('i', { class: 'bar' }), { '--c': eventColor(e) }),
    el('span', { class: 'when' }, e.all_day ? el('b', {}, t('allDay')) : [el('b', {}, hm(s)), el('small', {}, hm(en))]),
    el('span', { class: 'what' }, el('b', {}, e.summary || t('no.title')), e.location ? el('small', {}, e.location) : null));
  return row;
}

function colorPicker(current, onPick) {
  const wrap = el('div', { class: 'swatches' });
  const draw = (cur) => wrap.replaceChildren(
    el('button', { class: 'swatch none' + (!cur ? ' on' : ''), title: t('cal.none'), onclick: () => { draw(''); onPick(''); } }, '∅'),
    ...COLORS.map(([id, name, hex]) => css(el('button', { class: 'swatch' + (cur === id ? ' on' : ''), title: name, 'aria-label': name, onclick: () => { draw(id); onPick(id); } }), { '--c': hex })));
  draw(current || '');
  return wrap;
}

function eventDetail(e) {
  const s = parseWhen(e.start), en = parseWhen(e.end);
  const when = e.all_day ? `${s.toLocaleDateString(locale(), { dateStyle: 'full' })} · ${t('allDay')}`
    : `${s.toLocaleDateString(locale(), { dateStyle: 'full' })} ${hm(s)} – ${hm(en)}`;
  const body = el('div', { class: 'detail' },
    el('p', { class: 'muted' }, when),
    e.location ? el('p', {}, '📍 ', e.location) : null,
    e.description ? el('p', { class: 'pre' }, e.description) : null,
    el('div', { class: 'field' }, el('label', {}, t('cal.color')), colorPicker(e.color_id, (c) => setEventColor(e, c))),
    e.link ? el('a', { href: e.link, target: '_blank', rel: 'noopener noreferrer' }, t('cal.open')) : null);
  modal(e.summary || t('no.title'), body, [el('button', { class: 'danger', onclick: () => { closeModal(); deleteEvent(e); } }, t('delete'))]);
}

async function setEventColor(e, c) {
  const before = e.color_id;
  e.color_id = c || null;
  renderView();
  const ok = await gtry(async () => { const r = await api('PATCH', '/google/calendar/events/' + encodeURIComponent(e.id), { color_id: c }); G.events.set(r.event.id, r.event); });
  if (!ok) e.color_id = before;
  renderView();
}

async function deleteEvent(e) {
  G.events.delete(e.id);
  renderView();
  if (!await gtry(() => api('DELETE', '/google/calendar/events/' + encodeURIComponent(e.id)))) { G.events.set(e.id, e); renderView(); }
}

function eventSheet() {
  const d = G.sel || day0(new Date());
  const now = new Date();
  const startH = sameDay(d, now) ? Math.min(now.getHours() + 1, 23) : 9;
  const title = el('input', { type: 'text', placeholder: t('cal.title') });
  const allDay = toggle(false, t('cal.allDay'));
  const date = el('input', { type: 'date', value: ymd(d) });
  const start = el('input', { type: 'time', value: `${pad(startH)}:00` });
  const end = el('input', { type: 'time', value: `${pad(Math.min(startH + 1, 23))}:${startH + 1 > 23 ? '59' : '00'}` });
  const notes = el('textarea', { rows: 3, placeholder: t('cal.notes') });
  let color = '';
  const times = el('div', { class: 'row' }, field(t('cal.start'), start), field(t('cal.end'), end));
  allDay.input.addEventListener('change', (ev) => times.classList.toggle('hidden', ev.target.checked));
  const save = el('button', { class: 'primary', onclick: async () => {
    const summary = title.value.trim();
    if (!summary) return title.focus();
    const day = parseDay(date.value || ymd(d));
    const isAll = allDay.input.checked;
    const at = (v) => { const [h, m] = v.split(':').map(Number); return new Date(day.getFullYear(), day.getMonth(), day.getDate(), h, m); };
    let s = at(start.value || '09:00'), e = at(end.value || '10:00');
    if (e <= s) e = new Date(s.getTime() + 3600e3);
    const body = isAll
      ? { summary, start: ymd(day), end: ymd(addDays(day, 1)), time_zone: Intl.DateTimeFormat().resolvedOptions().timeZone }
      : { summary, start: rfc3339(s), end: rfc3339(e), time_zone: Intl.DateTimeFormat().resolvedOptions().timeZone };
    if (color) body.color_id = color;
    if (notes.value.trim()) body.description = notes.value.trim();
    save.disabled = true;
    const ok = await gtry(async () => { const r = await api('POST', '/google/calendar/events', body); G.events.set(r.event.id, r.event); });
    save.disabled = false;
    if (ok) { closeModal(); G.sel = day; renderView(); }
  } }, t('add'));
  modal(t('cal.new'), el('div', { class: 'form' }, field(t('cal.title'), title), field(t('cal.start'), date), allDay.node, times,
    el('div', { class: 'field' }, el('label', {}, t('cal.color')), colorPicker('', (c) => { color = c; })), field(t('cal.notes'), notes)),
  [el('button', { class: 'ghost', onclick: closeModal }, t('cancel')), save]);
  title.focus();
}

// ------------------------------------------------------------ mail

async function loadInbox() {
  G.mailLoading = true;
  if (G.view === 'mail') renderView();
  await gtry(async () => {
    const r = await api('GET', '/google/mail');
    G.inbox = r.messages; G.next = r.next_page; G.inboxLoaded = true; G.loadErr = null;
  }, true);
  G.mailLoading = false;
  updateUnread();
  if (G.openId) { const m = G.inbox.find((x) => x.id === G.openId); G.openId = null; if (m) openMail(m); }
  if (G.view === 'mail') renderView();
}

async function loadMore() {
  if (!G.next || G.moreLoading) return;
  G.moreLoading = true;
  await gtry(async () => {
    const r = await api('GET', '/google/mail?page=' + encodeURIComponent(G.next));
    const have = new Set(G.inbox.map((m) => m.id));
    G.inbox.push(...r.messages.filter((m) => !have.has(m.id)));
    G.next = r.next_page;
  });
  G.moreLoading = false;
  renderView();
}

function updateUnread() {
  const n = G.inbox.filter((m) => m.unread).length;
  const b = $('unread-badge');
  b.textContent = n > 99 ? '99+' : String(n);
  b.classList.toggle('hidden', n === 0);
}

async function setRead(m, read) {
  if (m.unread === !read) return;
  m.unread = !read;
  updateUnread(); renderView();
  if (!await gtry(() => api('POST', `/google/mail/${encodeURIComponent(m.id)}/read`, { read }))) { m.unread = read; updateUnread(); renderView(); }
}

async function openMail(m) {
  G.mail = m; G.msg = null; G.images = false;
  renderView();
  await gtry(async () => { const r = await api('GET', '/google/mail/' + encodeURIComponent(m.id)); if (G.mail === m) G.msg = r.message; });
  if (G.mail === m) renderView();
}

function renderMail(box) {
  const listScroll = box.querySelector('.mail-list')?.scrollTop || 0;
  const rows = G.inbox.map((m) => el('button', {
    class: 'mail-row' + (m.unread ? ' unread' : '') + (G.mail && G.mail.id === m.id ? ' sel' : ''),
    onclick: () => openMail(m),
    oncontextmenu: (ev) => { ev.preventDefault(); popMenu(ev.clientX, ev.clientY, [[m.unread ? t('mail.read') : t('mail.unread'), () => setRead(m, m.unread)]]); },
  }, avatar(senderName(m.from), address(m.from), 36),
  el('span', { class: 'mail-text' },
    el('span', { class: 'mail-line' }, el('b', {}, senderName(m.from)), el('small', {}, m.date_ms ? shortDate(new Date(m.date_ms)) : '')),
    el('span', { class: 'mail-subj' }, m.subject || '(no subject)'),
    el('span', { class: 'mail-snip' }, m.snippet || ''))));
  const list = el('div', { class: 'mail-list' }, ...rows,
    G.next ? el('button', { class: 'ghost wide', onclick: loadMore }, G.moreLoading ? '…' : t('more')) : null,
    !G.inbox.length ? el('div', { class: 'soft-empty' }, G.inboxLoaded ? t('mail.zero') : el('div', { class: 'spinner' })) : null);
  list.addEventListener('scroll', () => { if (list.scrollTop + list.clientHeight > list.scrollHeight - 200) loadMore(); });
  const top = header(t('inbox'),
    iconBtn('refresh', t('g.retry'), () => loadInbox(), G.mailLoading ? 'spin' : ''),
    el('button', { class: 'primary icon round', title: t('compose'), onclick: () => composeSheet() }, icon('compose')));
  fill(box, top, loadBanner(), el('div', { class: 'mail-split' + (G.mail ? ' reading' : '') }, list, mailDetail()));
  list.scrollTop = listScroll;
}

function mailDetail() {
  const m = G.mail;
  if (!m) return el('section', { class: 'mail-detail empty-pane' }, el('img', { src: '/icon.png', alt: '', class: 'empty-icon' }), el('p', { class: 'muted' }, t('mail.pick')));
  const msg = G.msg;
  const remote = msg && msg.html && /(src|background)\s*=\s*["']?https?:|url\(\s*["']?http/i.test(msg.html);
  const frame = el('iframe', { class: 'mail-frame', title: m.subject || 'mail', sandbox: 'allow-popups allow-popups-to-escape-sandbox allow-same-origin', referrerpolicy: 'no-referrer',
    src: `/api/google/mail/${encodeURIComponent(m.id)}/html${G.images ? '?images=1' : ''}` });
  frame.addEventListener('load', () => fitFrame(frame));
  return el('section', { class: 'mail-detail' },
    el('div', { class: 'mail-tools' },
      iconBtn('back', t('back'), () => { G.mail = null; renderView(); }, 'only-narrow'),
      el('span', { class: 'grow' }),
      iconBtn(m.unread ? 'open' : 'mail', m.unread ? t('mail.read') : t('mail.unread'), () => setRead(m, m.unread)),
      el('button', { class: 'primary icon round', title: t('reply'), onclick: () => composeSheet(m, msg) }, icon('reply'))),
    el('h2', { class: 'mail-title' }, m.subject || '(no subject)'),
    el('div', { class: 'gcard sender' }, avatar(senderName(m.from), address(m.from), 44),
      el('span', { class: 'grow' }, el('b', {}, senderName(m.from)), el('small', {}, address(m.from)), msg && msg.to ? el('small', {}, `${t('to')}: ${msg.to}`) : null),
      el('small', { class: 'muted' }, m.date_ms ? new Date(m.date_ms).toLocaleString(locale(), { dateStyle: 'medium', timeStyle: 'short' }) : '')),
    remote && !G.images ? el('div', { class: 'images-note' }, el('span', { class: 'muted small' }, t('mail.imagesNote')),
      el('button', { class: 'chipbtn', onclick: () => { G.images = true; renderView(); } }, icon('image'), t('mail.images'))) : null,
    frame);
}

/// Fit a fixed-width mail to the pane and grow the frame to its content.
/// The frame runs no script of its own; the app adjusts it from outside.
function fitFrame(f) {
  try {
    const doc = f.contentDocument;
    if (!doc) return;
    const w = doc.documentElement.scrollWidth, avail = f.clientWidth;
    if (w > avail + 1) doc.body.style.zoom = String(avail / w);
    f.style.height = Math.ceil(doc.documentElement.scrollHeight) + 'px';
  } catch {}
}

function composeSheet(replyTo, msg) {
  const to = el('input', { type: 'text', value: replyTo ? address((msg && msg.reply_to) || replyTo.from) : '' });
  const cc = el('input', { type: 'text' });
  const subj0 = replyTo ? (/^re:/i.test(replyTo.subject) ? replyTo.subject : 'Re: ' + replyTo.subject) : '';
  const subject = el('input', { type: 'text', value: subj0 });
  let quote = '';
  if (replyTo && msg) {
    const when = msg.date_ms ? new Date(msg.date_ms).toLocaleString(locale(), { dateStyle: 'medium', timeStyle: 'short' }) : msg.date;
    quote = `\n\n${when}, ${senderName(replyTo.from)}:\n` + (msg.body || '').split('\n').slice(0, 40).map((l) => '> ' + l).join('\n');
  }
  const body = el('textarea', { rows: 12, class: 'compose-body' });
  body.value = quote;
  const send = el('button', { class: 'primary', onclick: async () => {
    if (!to.value.trim()) return to.focus();
    send.disabled = true;
    const ok = await gtry(() => api('POST', '/google/mail/send', { to: to.value.trim(), cc: cc.value.trim() || undefined, subject: subject.value, body: body.value, reply_to_id: replyTo ? replyTo.id : undefined }));
    send.disabled = false;
    if (ok) { closeModal(); toast(t('mail.sent')); }
  } }, t('send'));
  modal(replyTo ? t('reply') : t('compose'), el('div', { class: 'form' }, field(t('to'), to), field(t('cc'), cc), field(t('subject'), subject), body),
    [el('button', { class: 'ghost', onclick: closeModal }, t('cancel')), send]);
  // Replies start at the top, above the quote, where you type.
  if (replyTo) { body.focus(); body.setSelectionRange(0, 0); body.scrollTop = 0; } else to.focus();
}

// ------------------------------------------------------------ tasks

async function loadTasks() {
  await gtry(async () => { G.lists = (await api('GET', '/google/tasks?completed=true')).lists; G.tasksLoaded = true; G.loadErr = null; }, true);
  if (G.view === 'tasks') renderView();
}

const shownLists = () => { const l = G.lists.find((x) => x.id === G.listId); return l ? [l] : G.lists; };

function renderTasks(box) {
  const lists = shownLists();
  const open = (l) => l.tasks.filter((x) => !x.completed || G.ticking.has(x.id));
  const chips = el('div', { class: 'chips-row' },
    chip(t('tasks.all'), '', G.lists.reduce((n, l) => n + l.tasks.filter((x) => !x.completed).length, 0)),
    ...G.lists.map((l) => chip(l.title, l.id, l.tasks.filter((x) => !x.completed).length)));
  const body = el('div', { class: 'task-list' });
  const total = lists.reduce((n, l) => n + open(l).length, 0);
  if (!G.tasksLoaded) body.append(el('div', { class: 'soft-empty' }, el('div', { class: 'spinner' })));
  else if (!total) body.append(el('div', { class: 'soft-empty' }, t('tasks.allDone')));
  for (const l of lists) {
    const items = open(l);
    if (lists.length > 1 && items.length) body.append(el('h3', { class: 'list-head' }, l.title));
    for (const x of items) body.append(taskRow(x, l.id));
  }
  const done = lists.flatMap((l) => l.tasks.filter((x) => x.completed && !G.ticking.has(x.id)).map((x) => [x, l.id]));
  if (done.length) {
    body.append(el('button', { class: 'ghost done-toggle' + (G.showDone ? ' open' : ''), onclick: () => { G.showDone = !G.showDone; renderView(); } }, t('tasks.done', { n: done.length }), ' ›'));
    if (G.showDone) for (const [x, lid] of done) body.append(taskRow(x, lid));
  }
  const top = header(t('v.tasks'), el('button', { class: 'primary icon round', title: t('tasks.new'), disabled: !G.lists.length, onclick: () => taskSheet() }, icon('plus')));
  fill(box, top, loadBanner(), G.lists.length ? chips : null, body);
}

function chip(title, id, count) {
  return el('button', { class: 'tchip' + ((G.lists.some((l) => l.id === G.listId) ? G.listId : '') === id ? ' on' : ''), onclick: () => {
    G.listId = id;
    try { localStorage.setItem('alice.tasks.list', id); } catch {}
    renderView();
  } }, title, count ? el('small', {}, count) : null);
}

function dueChip(d) {
  const today = day0(new Date());
  const cls = d < today ? 'late' : sameDay(d, today) ? 'today' : sameDay(d, addDays(today, 1)) ? 'tomorrow' : '';
  return el('span', { class: 'due ' + cls }, slash(d));
}

function taskRow(x, listId) {
  const checked = x.completed || G.ticking.has(x.id);
  const menu = (ev) => {
    ev.preventDefault(); ev.stopPropagation();
    const r = ev.currentTarget.getBoundingClientRect ? ev.currentTarget.getBoundingClientRect() : null;
    popMenu(ev.clientX || (r && r.left), ev.clientY || (r && r.bottom), [[t('edit'), () => taskSheet(x, listId)], [t('delete'), () => deleteTask(x, listId), true]]);
  };
  const box = el('button', { class: 'tick' + (checked ? ' on' : ''), 'aria-label': 'done', onclick: (ev) => toggleTask(x, listId, ev.currentTarget) }, checked ? icon('check') : null);
  const row = el('div', { class: 'task' + (checked ? ' done' : '') + (x.parent ? ' sub' : ''), oncontextmenu: menu },
    box,
    el('span', { class: 'task-text' }, el('span', { class: 'task-title' }, x.title || t('no.title')),
      x.notes ? el('small', {}, x.notes) : null, x.due && !checked ? dueChip(parseDay(x.due)) : null),
    el('button', { class: 'ghost icon more-btn', title: t('more'), onclick: menu }, icon('more')));
  // Long press on touch screens opens the same menu.
  let timer = null;
  row.addEventListener('touchstart', (ev) => { const p = ev.touches[0]; timer = setTimeout(() => { row.classList.add('lift'); popMenu(p.clientX, p.clientY, [[t('edit'), () => taskSheet(x, listId)], [t('delete'), () => deleteTask(x, listId), true]]); setTimeout(() => row.classList.remove('lift'), 300); }, 450); }, { passive: true });
  for (const k of ['touchend', 'touchmove', 'touchcancel']) row.addEventListener(k, () => clearTimeout(timer), { passive: true });
  return row;
}

/// Hearts and sparkles pop out of the checkbox, then the task moves to Completed.
function confetti(target) {
  const r = target.getBoundingClientRect();
  const layer = css(el('span', { class: 'confetti' }), { left: r.left + r.width / 2 + 'px', top: r.top + r.height / 2 + 'px' });
  for (let i = 0; i < 8; i++) {
    const a = (i / 8) * Math.PI * 2 - Math.PI / 2;
    layer.append(css(el('i', {}, i % 2 ? '✦' : '♥'), { '--x': Math.cos(a) * 26 + 'px', '--y': Math.sin(a) * 26 + 'px', '--c': PASTELS[i % PASTELS.length] }));
  }
  document.body.append(layer);
  setTimeout(() => layer.remove(), 800);
}

async function toggleTask(x, listId, btn) {
  if (x.id.startsWith('new-')) return;
  const done = !x.completed;
  if (done) {
    G.ticking.add(x.id);
    renderView();
    confetti(btn);
    await new Promise((r) => setTimeout(r, 600));
    G.ticking.delete(x.id);
  }
  x.completed = done;
  renderView();
  if (!await gtry(() => api('PATCH', `/google/tasks/${encodeURIComponent(listId)}/${encodeURIComponent(x.id)}`, { completed: done }))) { x.completed = !done; renderView(); }
}

async function deleteTask(x, listId) {
  const l = G.lists.find((y) => y.id === listId);
  if (!l) return;
  const before = l.tasks;
  l.tasks = l.tasks.filter((y) => y.id !== x.id);
  renderView();
  if (!await gtry(() => api('DELETE', `/google/tasks/${encodeURIComponent(listId)}/${encodeURIComponent(x.id)}`))) { l.tasks = before; renderView(); }
}

function taskSheet(x, listId) {
  const editing = !!x;
  const title = el('input', { type: 'text', value: x ? x.title : '', placeholder: t('cal.title') });
  const notes = el('textarea', { rows: 3 });
  notes.value = (x && x.notes) || '';
  const lists = shownLists();
  const list = el('select', {}, ...G.lists.map((l) => el('option', { value: l.id }, l.title)));
  list.value = listId || (lists[0] || G.lists[0] || {}).id || '';
  const hasDue = toggle(!!(x && x.due), t('tasks.due'));
  const due = el('input', { type: 'date', value: x && x.due ? x.due.slice(0, 10) : ymd(new Date()) });
  const quick = el('div', { class: 'row' }, ...[[t('today'), 0], [t('tomorrow'), 1], [t('tasks.nextWeek'), 7]].map(([label, n]) =>
    el('button', { class: 'chipbtn', type: 'button', onclick: () => { due.value = ymd(addDays(new Date(), n)); } }, label)));
  const dueBox = el('div', { class: 'form' + ((x && x.due) ? '' : ' hidden') }, quick, due);
  hasDue.input.addEventListener('change', (ev) => dueBox.classList.toggle('hidden', !ev.target.checked));
  const save = el('button', { class: 'primary', onclick: async () => {
    const tt = title.value.trim();
    if (!tt) return title.focus();
    const d = hasDue.input.checked ? due.value : null;
    closeModal();
    if (editing) {
      const before = { ...x };
      Object.assign(x, { title: tt, notes: notes.value || null, due: d });
      renderView();
      const ok = await gtry(async () => { const r = await api('PATCH', `/google/tasks/${encodeURIComponent(listId)}/${encodeURIComponent(x.id)}`, { title: tt, notes: notes.value, due: d }); Object.assign(x, r.task); });
      if (!ok) Object.assign(x, before);
    } else {
      const l = G.lists.find((y) => y.id === list.value);
      if (!l) return;
      const temp = { id: 'new-' + Date.now(), title: tt, due: d, completed: false };
      l.tasks.unshift(temp);
      renderView();
      const ok = await gtry(async () => { const r = await api('POST', `/google/tasks/${encodeURIComponent(l.id)}`, { title: tt, due: d || undefined }); Object.assign(temp, r.task); });
      if (!ok) l.tasks = l.tasks.filter((y) => y !== temp);
    }
    renderView();
  } }, editing ? t('save') : t('add'));
  modal(editing ? t('tasks.edit') : t('tasks.new'), el('div', { class: 'form' }, field(t('cal.title'), title),
    editing ? field(t('tasks.notes'), notes) : field(t('tasks.list'), list), hasDue.node, dueBox),
  [el('button', { class: 'ghost', onclick: closeModal }, t('cancel')), save]);
  title.focus();
}

// ------------------------------------------------------------ agents (scheduled instructions)

G.agents = null;
G.minEvery = 5;

async function loadAgents() {
  await gtry(async () => { const r = await api('GET', '/agents'); G.agents = r.agents; G.minEvery = r.min_every_min || 5; G.loadErr = null; }, true);
  if (G.view === 'agents') renderView();
}
function agentsUpdated() {
  if (G.view !== 'agents') { G.agents = null; return; }
  loadAgents();
  for (const id of G.hist.keys()) loadHistory(id);
}

const fmtWhen = (ms) => new Date(ms).toLocaleString(locale(), { month: 'numeric', day: 'numeric', weekday: 'short', hour: '2-digit', minute: '2-digit' });
const UNITS = [['week', 10080], ['day', 1440], ['hour', 60], ['min', 1]];
const splitEvery = (m) => { for (const [u, n] of UNITS) if (m % n === 0) return [u, m / n]; return ['min', m]; };

function scheduleText(a) {
  if (a.times && a.times.length) return t('ag.timesEach', { t: a.times.join('・') });
  if (!a.every_min) return t('ag.at', { t: fmtWhen(a.start_at) });
  const [u, n] = splitEvery(a.every_min);
  const s = new Date(a.start_at);
  if (u === 'day' && n === 1) return t('ag.daily', { t: hm(s) });
  if (u === 'week' && n === 1) return t('ag.weekly', { d: s.toLocaleDateString(locale(), { weekday: 'long' }), t: hm(s) });
  return t('ag.everyN', { n, u: t('ag.u.' + u) });
}

function renderAgents(box) {
  const list = G.agents;
  const cards = !list ? [el('div', { class: 'soft-empty' }, el('div', { class: 'spinner' }))]
    : !list.length ? [el('div', { class: 'gcard soft-empty' }, t('ag.none'))]
    : list.map(agentCard);
  const top = header(t('v.agents'), notifyButton(),
    el('button', { class: 'primary icon round', title: t('ag.new'), onclick: () => agentSheet() }, icon('plus')));
  fill(box, top, loadBanner(), el('p', { class: 'muted ag-intro' }, t('ag.intro')), el('div', { class: 'ag-list' }, ...cards));
}

function agentCard(a) {
  const sw = toggle(a.enabled, '');
  sw.input.addEventListener('change', () => saveAgent(a.id, { enabled: sw.input.checked }));
  const state = !a.enabled ? t('ag.off') : a.next_run ? t('ag.next', { t: fmtWhen(a.next_run) }) : t('ag.done');
  const to = a.thread_title || (a.thread_id ? '…' : t('ag.newChat'));
  const card = el('div', { class: 'gcard ag-card' + (a.enabled ? '' : ' off') },
    el('div', { class: 'ag-head' }, css(el('span', { class: 'ag-icon' }, '✦'), { '--c': pastel(a.id) }),
      el('span', { class: 'grow' }, el('b', {}, a.name), el('small', {}, scheduleText(a))), sw.node),
    el('p', { class: 'ag-prompt' }, a.prompt),
    el('div', { class: 'ag-foot' },
      el('span', { class: 'chipbtn ag-state' }, state),
      a.thread_id ? el('button', { class: 'ghost small', onclick: () => { setView('chat'); openThread(a.thread_id); } }, '→ ', to) : el('span', { class: 'muted small' }, '→ ', to),
      el('span', { class: 'grow' }),
      a.last_run ? el('small', { class: 'muted', title: a.last_status || '' }, t('ag.last', { t: fmtWhen(a.last_run) }), statusText(a.last_status)) : null,
      el('button', { class: 'ghost small' + (G.hist.has(a.id) ? ' on' : ''), onclick: () => toggleHistory(a) }, '🕘 ', t('ag.hist')),
      el('button', { class: 'ghost small', onclick: () => runAgent(a) }, '▶ ', t('ag.run')),
      el('button', { class: 'ghost icon more-btn on', title: t('more'), onclick: (ev) => { ev.stopPropagation(); popMenu(ev.clientX, ev.clientY, [[t('edit'), () => agentSheet(a)], [t('delete'), () => deleteAgent(a), true]]); } }, icon('more'))));
  if (G.hist.has(a.id)) card.append(historyEl(a));
  card.addEventListener('dblclick', () => agentSheet(a));
  return card;
}

// Run history (Web only): the last ten runs with result, target chat and why one failed.
G.hist = new Map(); // agent id -> runs (null while loading)

async function loadHistory(id) {
  await gtry(async () => { G.hist.set(id, (await api('GET', `/agents/${id}/runs`)).runs); });
  if (G.view === 'agents') renderView();
}

function toggleHistory(a) {
  if (G.hist.has(a.id)) G.hist.delete(a.id); else { G.hist.set(a.id, null); loadHistory(a.id); }
  renderView();
}

function historyEl(a) {
  const runs = G.hist.get(a.id);
  if (!runs) return el('div', { class: 'ag-hist' }, el('div', { class: 'spinner' }));
  if (!runs.length) return el('div', { class: 'ag-hist muted small' }, t('ag.histNone'));
  return el('div', { class: 'ag-hist' }, ...runs.map((r) => el('div', { class: 'ag-run' },
    el('span', { class: 'ag-run-time' }, fmtWhen(r.started_at)),
    el('span', { class: 'ag-badge ' + r.status }, t('ag.st.' + r.status)),
    r.manual ? el('span', { class: 'muted small' }, t('ag.manual')) : null,
    r.thread_id && r.thread_title
      ? el('button', { class: 'ghost small', onclick: () => { setView('chat'); openThread(r.thread_id); } }, '→ ', r.thread_title)
      : el('span', { class: 'muted small' }, '→ ', r.thread_id ? '…' : t('ag.goneChat')),
    r.error ? el('span', { class: 'ag-err', title: r.error }, r.error) : null)));
}

function statusText(s) {
  if (!s || s === 'ok') return '';
  const k = s.startsWith('error') ? 'error' : s;
  return ' · ' + (I18N[LANG]['ag.st.' + k] || I18N.en['ag.st.' + k] ? t('ag.st.' + k) : s) + (k === 'error' ? ' ⚠︎' : '');
}

async function saveAgent(id, fields) {
  const ok = await gtry(() => api(id ? 'PATCH' : 'POST', id ? '/agents/' + id : '/agents', fields));
  await loadAgents();
  return ok;
}

async function runAgent(a) {
  if (await gtry(() => api('POST', `/agents/${a.id}/run`, {}))) toast(t('ag.ran'));
  loadAgents();
}

async function deleteAgent(a) {
  G.agents = G.agents.filter((x) => x.id !== a.id);
  renderView();
  await gtry(() => api('DELETE', '/agents/' + a.id));
  loadAgents();
}

const localInput = (ms) => { const d = new Date(ms); return `${ymd(d)}T${pad(d.getHours())}:${pad(d.getMinutes())}`; };

function agentSheet(a) {
  const name = el('input', { type: 'text', value: a ? a.name : '' });
  const prompt = el('textarea', { rows: 5, placeholder: t('ag.promptHint') });
  prompt.value = a ? a.prompt : '';
  // Repeat every N units, or every day at set times.
  let mode = a && a.times && a.times.length ? 'times' : 'every';
  const [u0, n0] = a && a.every_min ? splitEvery(a.every_min) : ['hour', 1];
  const unit = el('select', {}, ...[['min', t('ag.min')], ['hour', t('ag.hour')], ['day', t('ag.day')], ['week', t('ag.week')]].map(([v, l]) => el('option', { value: v }, l)));
  unit.value = u0;
  const n = el('input', { type: 'number', min: 1, value: n0 });
  const everyBox = el('div', { class: 'row' }, field(t('ag.every'), n), field(' ', unit));
  const times = el('div', { class: 'times' });
  const addTime = (v) => {
    const inp = el('input', { type: 'time', value: v });
    const row = el('div', { class: 'time-row' }, inp, el('button', { class: 'ghost icon', type: 'button', title: t('delete'), onclick: () => row.remove() }, '×'));
    times.append(row);
  };
  for (const v of (a && a.times && a.times.length ? a.times : ['17:00'])) addTime(v);
  const timesBox = el('div', { class: 'form' }, times, el('button', { class: 'chipbtn', type: 'button', onclick: () => addTime('12:00') }, t('ag.addTime')));
  const seg = el('div', { class: 'seg' });
  const drawMode = () => {
    seg.replaceChildren(...['every', 'times'].map((m) => el('button', { type: 'button', class: mode === m ? 'on' : '', onclick: () => { mode = m; drawMode(); } }, t('ag.mode.' + m))));
    everyBox.classList.toggle('hidden', mode !== 'every');
    timesBox.classList.toggle('hidden', mode !== 'times');
  };
  drawMode();
  const to = el('select', {}, el('option', { value: '' }, t('ag.newThread')), ...S.threads.map((th) => el('option', { value: th.id }, th.title || t('newTitle'))));
  to.value = (a && a.thread_id) || '';
  const on = toggle(a ? a.enabled : true, t('ag.on'));
  const save = el('button', { class: 'primary', onclick: async () => {
    if (!name.value.trim()) return name.focus();
    if (!prompt.value.trim()) return prompt.focus();
    const fields = { name: name.value.trim(), prompt: prompt.value.trim(), thread_id: to.value || null, enabled: on.input.checked };
    if (mode === 'times') {
      fields.times = [...times.querySelectorAll('input')].map((i) => i.value).filter(Boolean);
      if (!fields.times.length) return addTime('17:00');
    } else {
      const mult = { min: 1, hour: 60, day: 1440, week: 10080 }[unit.value] || 60;
      fields.every_min = Math.max(1, Math.round(Number(n.value) || 1)) * mult;
      fields.times = [];
      if (fields.every_min < G.minEvery) return toast(t('ag.minEvery', { n: G.minEvery }));
    }
    save.disabled = true;
    const ok = await saveAgent(a && a.id, fields);
    save.disabled = false;
    if (ok) closeModal();
  } }, a ? t('save') : t('add'));
  modal(a ? t('ag.edit') : t('ag.new'), el('div', { class: 'form' }, field(t('ag.name'), name), field(t('ag.prompt'), prompt),
    seg, everyBox, timesBox, field(t('ag.to'), to), on.node),
  [a ? el('button', { class: 'ghost', onclick: () => { closeModal(); runAgent(a); } }, '▶ ', t('ag.run')) : null, el('span', { class: 'grow' }),
    el('button', { class: 'ghost', onclick: closeModal }, t('cancel')), save]);
  name.focus();
}

// ------------------------------------------------------------ chat notifications

const notifyOK = () => 'Notification' in window && window.isSecureContext;
const notifyOn = () => { try { return localStorage.getItem('alice.notify') === 'on'; } catch { return false; } };

function notifyButton() {
  if (!notifyOK()) return el('span', { class: 'muted small', title: t('nt.insecure') }, '🔕');
  const enabled = notifyOn() && Notification.permission === 'granted';
  return el('button', { class: 'ghost small', title: Notification.permission === 'denied' ? t('nt.denied') : '', onclick: async () => {
    let on = !enabled;
    if (on && Notification.permission !== 'granted') on = (await Notification.requestPermission()) === 'granted';
    if (!on && Notification.permission === 'denied') toast(t('nt.denied'));
    try { localStorage.setItem('alice.notify', on ? 'on' : 'off'); } catch {}
    renderView();
  } }, enabled ? '🔔 ' + t('nt.on') : '🔕 ' + t('nt.off'));
}

/// A reply finished in a chat you are not looking at (or the tab is hidden).
function notifyReply(threadId, m) {
  if (!notifyOK() || !notifyOn() || Notification.permission !== 'granted' || m.role !== 'assistant' || quiet(m)) return;
  if (!document.hidden && G.view === 'chat' && S.current === threadId) return;
  const th = S.threads.find((x) => x.id === threadId);
  const text = (m.content || '').replace(/[#*_`>|-]+/g, ' ').replace(/\s+/g, ' ').trim().slice(0, 160);
  if (!text) return;
  const n = new Notification(th && th.title ? th.title : 'Alice', { body: text, icon: '/icon.png', tag: threadId });
  n.onclick = () => { window.focus(); setView('chat'); openThread(threadId); n.close(); };
}

// ------------------------------------------------------------ shared

function loadBanner() {
  if (!G.loadErr) return null;
  return el('button', { class: 'load-banner', title: G.loadErr, onclick: () => { G.loadErr = null; loadView(true); } }, '⟳ ', t('g.refreshFail'));
}

// Refresh quietly when the tab comes back.
document.addEventListener('visibilitychange', () => { if (!document.hidden && G.view !== 'chat') loadView(true); });

// Start where the user was (or where the link points).
(function initViews() {
  let v = location.hash.slice(1);
  if (!v) { try { v = localStorage.getItem('alice.view') || 'chat'; } catch { v = 'chat'; } }
  const go = () => setView(v);
  if ($('app').classList.contains('hidden')) {
    // Wait until sign-in shows the app.
    const mo = new MutationObserver(() => { if (!$('app').classList.contains('hidden')) { mo.disconnect(); go(); if (G.view !== 'mail') gtry(async () => { G.status = await api('GET', '/google'); if (on('gmail')) loadInbox(); }, true); } });
    mo.observe($('app'), { attributes: true, attributeFilter: ['class'] });
  } else go();
})();
