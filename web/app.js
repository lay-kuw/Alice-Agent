// Alice Web GUI. No framework, no build step.
// Security: user/model content is only inserted as text, or through md()
// which escapes everything before adding a small set of safe tags.
'use strict';

const $ = (id) => document.getElementById(id);
const S = {
  threads: [], current: null, thread: null, messages: [], attachments: [],
  generating: false, models: { models: [], default: '' }, pending: [], newModel: '',
  approvals: new Map(), es: null, config: null, profile: 'default',
};

// ------------------------------------------------------------ i18n

const I18N = {
  ja: {
    'login.hint': 'サーバーで `alice token` を実行して表示される API トークンを入力してください。',
    'login.submit': 'サインイン', 'login.invalid': 'トークンが違います',
    newChat: '＋ 新しいチャット', settings: '⚙︎ 設定', rename: '名前を変更', deleteThread: 'スレッドを削除',
    empty: '何をお手伝いしましょう？', attach: 'ファイルを添付', placeholder: 'Alice にメッセージ…', newTitle: '新しいチャット',
    'confirm.delete': 'このスレッドを削除しますか？ このスレッドから覚えたことも消えます。', 'prompt.title': 'スレッドの名前',
    deleted: 'このスレッドは削除されました', stopped: '（停止しました）', copy: 'コピー', copied: 'コピーしました',
    allow: '許可する', deny: '拒否', 'approval.q': 'を実行してもいいですか？', 'approval.from': '{c} からの要求',
    'approval.other': '承認待ち: {t}（別のスレッド）', error: 'エラー',
    'settings.title': '設定', 'tab.general': '一般', 'tab.soul': '人格 (SOUL)', 'tab.models': 'モデル', 'tab.channels': 'チャンネル',
    'tab.tools': 'ツール', 'tab.memory': '記憶', 'tab.system': 'システム', loading: '読み込み中…', save: '保存', saved: '保存しました',
    'g.agent': 'エージェント', 'g.name': '名前', 'g.lang': '返答の言語（例: ja, en）', 'g.theme': 'テーマ', 'theme.system': '自動', 'theme.light': 'ライト', 'theme.dark': 'ダーク',
    'g.server': 'サーバー（上級者向け）', 'g.bind': '待ち受けアドレス（127.0.0.1 = このマシンのみ）', 'g.port': 'ポート', 'g.public': '公開 URL（Tailscale / リバースプロキシ）',
    'g.restart': '保存しました。アドレス・ポートの変更は再起動後に反映されます（alice restart）。', signout: 'サインアウト',
    'soul.desc': 'Alice の性格・話し方・仕事の進め方を書くファイルです。保存すると次の返答から反映されます。会話を全部消しても残ります。',
    'soul.reset': 'テンプレートに戻す', 'soul.resetq': 'SOUL.md をテンプレートに戻しますか？',
    'm.providers': 'プロバイダー', 'm.ok': '利用OK', 'm.ng': '要設定', 'm.unknown': '未確認', 'm.checking': '確認中…',
    'm.default': '既定', 'm.makeDefault': '既定にする', 'm.test': 'テスト', 'm.testing': 'テスト中…', 'm.remove': '削除',
    'm.removeq': 'プロバイダー {p} と API キーを削除しますか？', 'm.key': 'API キー', 'm.keySet': '設定済み — 変更する場合のみ入力', 'm.saveKey': 'キーを保存',
    'm.none': 'まだプロバイダーがありません。', 'm.add': 'プロバイダーを追加', 'm.kind': '種類', 'm.id': 'ID（例: openai）', 'm.base': 'ベース URL（任意。openai-compatible は必須）',
    'm.models': 'モデル名（カンマ区切り）', 'm.addBtn': '追加', 'm.added': '追加しました',
    'm.sub': 'ChatGPT / Gemini のサブスクは公式 CLI（codex-cli / gemini-cli）経由のみ。テキスト専用で、Alice のツールは使えません。',
    'm.cliLogin': 'ログインはサーバーで実行: alice model login {p}', 'm.caps': 'ツール: {t}・画像: {i}・ストリーミング: {s}',
    'c.iphone': 'iPhone アプリ', 'c.iphone.sub': '同じサーバーに接続して、スレッドと記憶を共有します。',
    'c.ios1': 'アプリを開いて「サーバーを追加」', 'c.ios2': 'この URL を入力', 'c.ios3': 'API トークンを入力',
    'c.ios3.hint': 'セキュリティのため、ここには表示しません。サーバーで `alice token` を実行してください。',
    'c.remote': '外出先から使うときは Tailscale（tailscale serve）か HTTPS のリバースプロキシ経由にしてください。',
    'c.st.connected': '接続中', 'c.st.connecting': '接続しています…', 'c.st.error': 'エラー', 'c.st.disabled': 'オフ', 'c.st.notset': '未設定',
    'c.enable': '有効にする', 'c.token': 'Bot トークン', 'c.tokenSet': '設定済み — 変更する場合のみ入力', 'c.ids': 'あなたのユーザー ID',
    'c.idsPh': 'ID を入力して Enter', 'c.idsHint': 'ここに登録した人だけが Alice と話せます。それ以外の人のメッセージは無視されます。',
    'c.save': '保存して接続', 'c.remove': 'トークンを削除して無効化', 'c.removeq': '{n} のトークンを削除して無効にしますか？',
    'c.needId': '少なくとも 1 つユーザー ID を登録してください', 'c.needToken': 'Bot トークンを入力してください',
    'c.tg.sub': 'Telegram のボットと 1 対 1 で会話します。',
    'c.tg1': 'Bot を作る', 'c.tg1.b': 'Telegram で @BotFather を開き /newbot を送信。名前を決めるとトークンが表示されます。',
    'c.tg2': 'トークンを貼り付け', 'c.tg3': 'ユーザー ID を登録', 'c.tg3.b': '@userinfobot に話しかけると数字の ID が分かります。',
    'c.dc.sub': 'DM、またはサーバーで @メンションして会話します。',
    'c.dc1': 'Bot を作って招待', 'c.dc1.b': 'Discord Developer Portal → New Application → Bot で Reset Token。OAuth2 → URL Generator で「bot」と「Send Messages / Read Message History」を選び、表示された URL でサーバーに招待します。',
    'c.dc2': 'トークンを貼り付け', 'c.dc3': 'ユーザー ID を登録', 'c.dc3.b': 'Discord の設定 → 詳細設定 → 開発者モードを ON。自分のアイコンを右クリック →「ユーザー ID をコピー」。',
    'c.del': 'スレッドを削除すると、Alice が投稿したメッセージもできる範囲で削除します（Telegram は 48 時間以内のみ）。',
    't.perm': '権限', 't.disabled': '無効', 't.ask': '実行前に確認', 't.allow': '常に許可', 't.notcfg': '（未設定）',
    't.permNote': '権限はここか `alice tool` でのみ変更できます。AI 自身は変更できません。',
    't.fs': 'ファイル', 't.dirs': 'アクセスを許可するフォルダ（1 行に 1 つ）', 't.dirsPh': '未指定なら ~/.alice/workspace', 't.effective': '現在: ', 't.saveDirs': 'フォルダを保存',
    't.web': 'Web 検索', 't.provider': '検索プロバイダー', 't.off': '無効', 't.braveKey': 'Brave API キー', 't.saveWeb': 'Web 検索を保存',
    't.webNote': '取得したページは信頼できないデータとして扱います。web_fetch は既定で実行前に確認し、ローカルネットワークにはアクセスしません。',
    't.google': 'Google アカウント', 't.gNote': 'Google Cloud で作成した「デスクトップ アプリ」の OAuth クライアントを使います。チェックしたサービスの権限だけを要求します。',
    't.cid': 'クライアント ID', 't.csecret': 'クライアント シークレット', 't.connected': '接続済み', 't.notConnected': '未接続',
    't.connect': '接続', 't.reconnect': '再接続', 't.disconnect': '切断', 't.gToast': '新しいタブで許可してください（ブラウザがサーバーと同じマシンにある場合）。別マシンなら `alice google login`。',
    'mem.search': '記憶を検索', 'mem.none': '記憶はまだありません。', 'mem.sources': '出典', 'mem.open': 'スレッドを開く',
    'mem.count': '{n} 件の出典', 'mem.clear': '記憶をすべて削除', 'mem.clearq': '記憶をすべて削除しますか？ 会話は残ります。',
    'mem.all': '会話と記憶をすべて削除', 'mem.allq': 'すべてのスレッドと、そこから覚えたことを削除します。設定・モデル・API キーは残ります。\n確認のため delete-all と入力してください。',
    'mem.note': '削除の対象は Alice のデータベース・検索インデックス・添付ファイルです。LLM プロバイダーや Telegram/Discord に送信済みのデータは対象外です。',
    'st.memories': '記憶', 'st.sources': '出典', 'st.threads': 'スレッド', 'st.db': 'データベース', 'st.att': '添付', 'st.version': 'バージョン',
    'st.uptime': '稼働時間', 'st.mem': 'メモリ', 'st.cpu': 'CPU', 'st.messages': 'メッセージ', 'sys.logs': 'ログ', refresh: '更新',
    profile: 'プロファイル: {p}', more: 'その他の操作',
    'g.uiLang': '表示言語', 'g.soulHint': '返答の言語や口調は「人格 (SOUL)」で設定します。',
    't.ws': '作業フォルダ', 't.wsHint': 'ファイルツールが相対パスで読み書きする場所。空欄なら ~/.alice/workspace。', 't.extra': 'ほかにアクセスを許可するフォルダ（1 行に 1 つ・任意）',
    'cmd.new': '新しいチャット', 'cmd.model': 'このスレッドのモデルを変更', 'cmd.rename': 'このスレッドの名前を変更', 'cmd.delete': 'このスレッドを削除',
    'cmd.attach': 'ファイルを添付', 'cmd.usage': 'サブスクの残量と使用トークン', 'cmd.soul': '人格 (SOUL) を編集', 'cmd.settings': '設定を開く', 'cmd.help': 'コマンド一覧',
    'cmd.unknown': '不明なコマンド: {c}', 'cmd.noModel': 'モデルが見つかりません: {m}', 'cmd.noThread': 'まだスレッドがありません',
    'u.title': '使用状況', 'u.left': '残り {p}%', 'u.reset': '{t} にリセット', 'u.none': 'サブスク（codex-cli）のプロバイダーがありません。',
    'u.credits': 'リセット権: {n} 回', 'u.tokens': 'Alice が使ったトークン', 'u.day': '24 時間', 'u.month': '30 日', 'u.model': 'モデル', 'u.in': '入力', 'u.out': '出力', 'u.replies': '回答',
    'u.win.5h': '5 時間枠', 'u.win.weekly': '週間枠', 'u.checking': '確認中…',
  },
  en: {
    'login.hint': 'Enter the API token shown by `alice token` on the server.', 'login.submit': 'Sign in', 'login.invalid': 'Invalid token',
    newChat: '＋ New chat', settings: '⚙︎ Settings', rename: 'Rename', deleteThread: 'Delete thread',
    empty: 'How can I help?', attach: 'Attach files', placeholder: 'Message Alice…', newTitle: 'New chat',
    'confirm.delete': 'Delete this thread? Memories learned from it are deleted too.', 'prompt.title': 'Thread title',
    deleted: 'This thread was deleted', stopped: '(stopped)', copy: 'Copy', copied: 'Copied',
    allow: 'Allow', deny: 'Deny', 'approval.q': '— allow?', 'approval.from': 'requested from {c}', 'approval.other': 'Approval needed: {t} (another thread)', error: 'Error',
    'settings.title': 'Settings', 'tab.general': 'General', 'tab.soul': 'Personality', 'tab.models': 'Models', 'tab.channels': 'Channels',
    'tab.tools': 'Tools', 'tab.memory': 'Memory', 'tab.system': 'System', loading: 'Loading…', save: 'Save', saved: 'Saved',
    'g.agent': 'Agent', 'g.name': 'Name', 'g.lang': 'Reply language (e.g. ja, en)', 'g.theme': 'Theme', 'theme.system': 'Auto', 'theme.light': 'Light', 'theme.dark': 'Dark',
    'g.server': 'Server (advanced)', 'g.bind': 'Bind address (127.0.0.1 = this machine only)', 'g.port': 'Port', 'g.public': 'Public URL (Tailscale / reverse proxy)',
    'g.restart': 'Saved. Address/port changes apply after `alice restart`.', signout: 'Sign out',
    'soul.desc': "Alice's personality, tone and way of working. Changes apply from the next reply and survive deleting all conversations.",
    'soul.reset': 'Reset to template', 'soul.resetq': 'Replace SOUL.md with the template?',
    'm.providers': 'Providers', 'm.ok': 'Ready', 'm.ng': 'Needs setup', 'm.unknown': 'Unchecked', 'm.checking': 'Checking…',
    'm.default': 'default', 'm.makeDefault': 'Make default', 'm.test': 'Test', 'm.testing': 'Testing…', 'm.remove': 'Remove',
    'm.removeq': 'Remove provider {p} and its API key?', 'm.key': 'API key', 'm.keySet': 'Set — enter only to replace', 'm.saveKey': 'Save key',
    'm.none': 'No providers yet.', 'm.add': 'Add provider', 'm.kind': 'Type', 'm.id': 'ID (e.g. openai)', 'm.base': 'Base URL (optional; required for openai-compatible)',
    'm.models': 'Model names, comma separated', 'm.addBtn': 'Add', 'm.added': 'Provider added',
    'm.sub': "ChatGPT / Gemini subscriptions work only through the vendors' official CLIs (codex-cli / gemini-cli). Text only; Alice tools are not available.",
    'm.cliLogin': 'Log in on the server: alice model login {p}', 'm.caps': 'tools: {t} · images: {i} · streaming: {s}',
    'c.iphone': 'iPhone app', 'c.iphone.sub': 'Connects to this server and shares threads and memory.',
    'c.ios1': 'Open the app and tap “Add server”', 'c.ios2': 'Enter this URL', 'c.ios3': 'Enter the API token',
    'c.ios3.hint': 'For security it is not shown here. Run `alice token` on the server.',
    'c.remote': 'For use outside your network, go through Tailscale (tailscale serve) or an HTTPS reverse proxy.',
    'c.st.connected': 'Connected', 'c.st.connecting': 'Connecting…', 'c.st.error': 'Error', 'c.st.disabled': 'Off', 'c.st.notset': 'Not set up',
    'c.enable': 'Enabled', 'c.token': 'Bot token', 'c.tokenSet': 'Set — enter only to replace', 'c.ids': 'Your user IDs',
    'c.idsPh': 'Type an ID and press Enter', 'c.idsHint': 'Only these users can talk to Alice; everyone else is ignored.',
    'c.save': 'Save & connect', 'c.remove': 'Remove token and disable', 'c.removeq': 'Remove the {n} token and disable it?',
    'c.needId': 'Add at least one user ID', 'c.needToken': 'Enter the bot token',
    'c.tg.sub': 'Chat with Alice one-to-one through a Telegram bot.',
    'c.tg1': 'Create a bot', 'c.tg1.b': 'Open @BotFather in Telegram and send /newbot. After naming it you get a token.',
    'c.tg2': 'Paste the token', 'c.tg3': 'Add your user ID', 'c.tg3.b': 'Message @userinfobot to see your numeric ID.',
    'c.dc.sub': 'Chat in DMs, or @mention the bot in a server.',
    'c.dc1': 'Create and invite a bot', 'c.dc1.b': 'Discord Developer Portal → New Application → Bot → Reset Token. Then OAuth2 → URL Generator: choose “bot” and “Send Messages / Read Message History”, and open the URL to invite it.',
    'c.dc2': 'Paste the token', 'c.dc3': 'Add your user ID', 'c.dc3.b': 'Settings → Advanced → Developer Mode on. Right-click your avatar → Copy User ID.',
    'c.del': 'When a thread is deleted, Alice also tries to delete its messages there (Telegram allows this within 48 hours).',
    't.perm': 'Permissions', 't.disabled': 'Disabled', 't.ask': 'Ask first', 't.allow': 'Always allow', 't.notcfg': '(not configured)',
    't.permNote': 'Permissions can only be changed here or with `alice tool`. The model cannot change them.',
    't.fs': 'Files', 't.dirs': 'Allowed folders (one per line)', 't.dirsPh': 'Default: ~/.alice/workspace', 't.effective': 'Effective: ', 't.saveDirs': 'Save folders',
    't.web': 'Web search', 't.provider': 'Search provider', 't.off': 'Disabled', 't.braveKey': 'Brave API key', 't.saveWeb': 'Save web search',
    't.webNote': 'Fetched pages are untrusted data. web_fetch asks first by default and never reaches your local network.',
    't.google': 'Google account', 't.gNote': 'Uses your own “Desktop app” OAuth client from Google Cloud. Only the checked services are requested.',
    't.cid': 'Client ID', 't.csecret': 'Client secret', 't.connected': 'Connected', 't.notConnected': 'Not connected',
    't.connect': 'Connect', 't.reconnect': 'Reconnect', 't.disconnect': 'Disconnect', 't.gToast': 'Approve in the new tab (works when the browser runs on the server machine). Otherwise use `alice google login`.',
    'mem.search': 'Search memories', 'mem.none': 'No memories yet.', 'mem.sources': 'Sources', 'mem.open': 'Open thread',
    'mem.count': '{n} source(s)', 'mem.clear': 'Delete all memories', 'mem.clearq': 'Delete all memories? Conversations are kept.',
    'mem.all': 'Delete all conversations and memories', 'mem.allq': 'This deletes ALL threads and everything Alice learned from them. Settings, models and API keys are kept.\nType delete-all to confirm.',
    'mem.note': "Deletion covers Alice's database, search index and attachments. Data already sent to LLM providers or Telegram/Discord is outside Alice's control.",
    'st.memories': 'memories', 'st.sources': 'sources', 'st.threads': 'threads', 'st.db': 'database', 'st.att': 'attachments', 'st.version': 'version',
    'st.uptime': 'uptime', 'st.mem': 'memory', 'st.cpu': 'cpu', 'st.messages': 'messages', 'sys.logs': 'Logs', refresh: 'Refresh',
    profile: 'Profile: {p}', more: 'More actions',
    'g.uiLang': 'Display language', 'g.soulHint': 'Reply language and tone are set in Personality (SOUL).',
    't.ws': 'Working folder', 't.wsHint': 'Where file tools read and write relative paths. Empty = ~/.alice/workspace.', 't.extra': 'Other allowed folders (one per line, optional)',
    'cmd.new': 'New chat', 'cmd.model': "Change this thread's model", 'cmd.rename': 'Rename this thread', 'cmd.delete': 'Delete this thread',
    'cmd.attach': 'Attach files', 'cmd.usage': 'Subscription limits and tokens used', 'cmd.soul': 'Edit personality (SOUL)', 'cmd.settings': 'Open settings', 'cmd.help': 'List commands',
    'cmd.unknown': 'Unknown command: {c}', 'cmd.noModel': 'No such model: {m}', 'cmd.noThread': 'No thread yet',
    'u.title': 'Usage', 'u.left': '{p}% left', 'u.reset': 'resets {t}', 'u.none': 'No subscription (codex-cli) providers.',
    'u.credits': 'Reset credits: {n}', 'u.tokens': 'Tokens used by Alice', 'u.day': '24 hours', 'u.month': '30 days', 'u.model': 'Model', 'u.in': 'In', 'u.out': 'Out', 'u.replies': 'Replies',
    'u.win.5h': '5-hour window', 'u.win.weekly': 'Weekly window', 'u.checking': 'Checking…',
  },
};
let LANG = (navigator.language || 'en').toLowerCase().startsWith('ja') ? 'ja' : 'en';
function t(key, vars) {
  let s = (I18N[LANG] && I18N[LANG][key]) || I18N.en[key] || key;
  for (const [k, v] of Object.entries(vars || {})) s = s.replace(`{${k}}`, v);
  return s;
}
function applyI18n() {
  document.documentElement.lang = LANG;
  for (const e of document.querySelectorAll('[data-i18n]')) e.textContent = t(e.dataset.i18n);
  for (const e of document.querySelectorAll('[data-i18n-title]')) e.title = t(e.dataset.i18nTitle);
  $('input').placeholder = t('placeholder');
  $('login-token').placeholder = 'API token';
}

// ------------------------------------------------------------ helpers

function el(tag, attrs, ...kids) {
  const e = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs || {})) {
    if (v == null || v === false) continue;
    if (k === 'class') e.className = v;
    else if (k.startsWith('on')) e.addEventListener(k.slice(2), v);
    else if (k === 'value') e.value = v;
    else e.setAttribute(k, v === true ? '' : v);
  }
  for (const k of kids.flat()) if (k != null && k !== false) e.append(k instanceof Node ? k : document.createTextNode(String(k)));
  return e;
}

function esc(s) {
  return String(s).replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
}

function toast(msg) {
  const x = $('toast');
  x.textContent = msg;
  x.classList.remove('hidden');
  clearTimeout(toast.timer);
  toast.timer = setTimeout(() => x.classList.add('hidden'), 3800);
}

async function api(method, path, body) {
  const opts = { method, headers: { 'X-Alice-CSRF': '1' }, credentials: 'same-origin' };
  if (body !== undefined) {
    if (body instanceof Blob) opts.body = body;
    else { opts.headers['Content-Type'] = 'application/json'; opts.body = JSON.stringify(body); }
  }
  const r = await fetch('/api' + path, opts);
  if (r.status === 401 && path !== '/login') { showLogin(); throw new Error('unauthorized'); }
  const text = await r.text();
  let v = null;
  try { v = text ? JSON.parse(text) : null; } catch { v = text; }
  if (!r.ok) throw new Error((v && v.error) || r.statusText);
  return v;
}

function fmtTime(ms) {
  return new Date(ms).toLocaleString(LANG, { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' });
}
function fmtBytes(n) {
  if (n < 1024) return n + ' B';
  if (n < 1048576) return (n / 1024).toFixed(1) + ' KB';
  return (n / 1048576).toFixed(1) + ' MB';
}
function badge(kind, text) { return el('span', { class: 'badge ' + kind }, text); }
function field(label, input, hint) { return el('div', { class: 'field' }, el('label', {}, label), input, hint ? el('p', { class: 'hint' }, hint) : null); }
function toggle(checked, label) {
  const input = el('input', { type: 'checkbox' });
  input.checked = !!checked;
  return { input, node: el('label', { class: 'switch' }, input, el('span', { class: 'track' }), label ? el('span', {}, label) : null) };
}
async function guard(fn) { try { await fn(); } catch (e) { if (e.message !== 'unauthorized') toast(e.message); } }

// ------------------------------------------------------------ markdown

function inline(s) {
  let out = '';
  for (const p of s.split(/(`[^`\n]+`)/g)) {
    if (p.length > 1 && p.startsWith('`') && p.endsWith('`')) { out += '<code>' + esc(p.slice(1, -1)) + '</code>'; continue; }
    let x = esc(p);
    x = x.replace(/\[([^\]]+)\]\((https?:\/\/[^\s)]+)\)/g, '<a href="$2" target="_blank" rel="noopener noreferrer">$1</a>');
    x = x.replace(/(^|[\s(])(https?:\/\/[^\s<)]+)/g, '$1<a href="$2" target="_blank" rel="noopener noreferrer">$2</a>');
    x = x.replace(/\*\*([^*\n]+)\*\*/g, '<strong>$1</strong>');
    x = x.replace(/(^|[^*\w])\*([^*\n]+)\*(?!\w)/g, '$1<em>$2</em>');
    x = x.replace(/~~([^~\n]+)~~/g, '<del>$1</del>');
    out += x;
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
        '<button class="copy ghost" type="button">' + esc(t('copy')) + '</button><pre><code>' + esc(code.join('\n')) + '</code></pre></div>');
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
      let tb = '<table><thead><tr>' + cells(line).map((c) => `<th>${c}</th>`).join('') + '</tr></thead><tbody>';
      i += 2;
      while (i < lines.length && /^\s*\|/.test(lines[i])) tb += '<tr>' + cells(lines[i++]).map((c) => `<td>${c}</td>`).join('') + '</tr>';
      out.push(tb + '</tbody></table>');
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
  navigator.clipboard?.writeText(b.parentElement.querySelector('code').textContent).then(() => {
    b.textContent = t('copied');
    setTimeout(() => (b.textContent = t('copy')), 1200);
  });
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
    $('login-error').textContent = err.message === 'unauthorized' || err.message === 'invalid token' ? t('login.invalid') : err.message;
  }
});

// ------------------------------------------------------------ threads

async function loadThreads() {
  S.threads = (await api('GET', '/threads')).threads;
  renderThreads();
}

function renderThreads() {
  $('threads').replaceChildren(...S.threads.map((th) =>
    el('div', { class: 'thread' + (th.id === S.current ? ' active' : '') },
      el('button', { class: 'open', title: th.title || t('newTitle'), onclick: () => { openThread(th.id); closeSide(); } },
        th.generating ? el('span', { class: 'dot' }) : null, el('span', { class: 't' }, th.title || t('newTitle'))),
      el('button', { class: 'more', title: t('more'), 'aria-label': t('more'), 'aria-haspopup': 'menu', onclick: (e) => openThreadMenu(e, th.id) }, '⋯'))));
}

let menuThread = null;
function openThreadMenu(e, id) {
  e.stopPropagation();
  const pop = $('thread-pop');
  const btn = e.currentTarget;
  if (menuThread === id && !pop.classList.contains('hidden')) return closeThreadMenu();
  closeThreadMenu();
  menuThread = id;
  btn.classList.add('on');
  pop.classList.remove('hidden');
  const r = btn.getBoundingClientRect();
  const w = pop.offsetWidth, h = pop.offsetHeight;
  pop.style.left = Math.min(r.right + 6, innerWidth - w - 8) + 'px';
  pop.style.top = Math.min(r.top, innerHeight - h - 8) + 'px';
}
function closeThreadMenu() {
  $('thread-pop').classList.add('hidden');
  for (const b of document.querySelectorAll('.thread .more.on')) b.classList.remove('on');
  menuThread = null;
}
document.addEventListener('click', closeThreadMenu);
$('threads').addEventListener('scroll', closeThreadMenu);
$('thread-pop').addEventListener('click', (e) => {
  const id = menuThread, act = e.target.dataset.act;
  closeThreadMenu();
  if (act === 'rename') renameThread(id);
  if (act === 'delete') deleteThread(id);
});

function closeSide() { $('app').classList.remove('side-open'); }

function newChat() {
  S.current = null; S.thread = null; S.messages = []; S.attachments = []; S.pending = []; S.generating = false;
  renderAll();
  $('input').focus();
}

async function openThread(id) {
  S.current = id;
  S.pending = [];
  try {
    const [th, m] = await Promise.all([api('GET', '/threads/' + id), api('GET', `/threads/${id}/messages`)]);
    if (S.current !== id) return;
    S.thread = th.thread; S.messages = m.messages; S.attachments = m.attachments; S.generating = m.generating;
    try { localStorage.setItem('alice.thread', id); } catch {}
  } catch (e) {
    toast(e.message);
    newChat();
    return;
  }
  renderAll();
  refreshApprovals();
}

function renderAll() { renderThreads(); renderHeader(); renderMessages(); renderComposer(); }

function renderHeader() {
  $('thread-title').textContent = S.thread?.title || t('newTitle');
  const sel = $('model-select');
  const cur = (S.thread ? S.thread.model : S.newModel) || S.models.default;
  sel.replaceChildren(...S.models.models.map((m) => el('option', { value: m }, m)));
  if (cur) sel.value = cur;
  sel.disabled = !S.models.models.length;
}

$('model-select').addEventListener('change', (e) => guard(async () => {
  const v = e.target.value;
  if (!S.current) { S.newModel = v; return; }
  S.thread = (await api('PATCH', '/threads/' + S.current, { model: v })).thread;
}));

function renameThread(id, title) {
  if (!id) return toast(t('cmd.noThread'));
  const cur = S.threads.find((x) => x.id === id)?.title || '';
  const name = title ?? prompt(t('prompt.title'), cur);
  if (name != null) guard(() => api('PATCH', '/threads/' + id, { title: name }));
}
function deleteThread(id) {
  if (!id) return toast(t('cmd.noThread'));
  if (confirm(t('confirm.delete'))) guard(() => api('DELETE', '/threads/' + id));
}
$('new-chat').addEventListener('click', () => { newChat(); closeSide(); });
$('toggle-side').addEventListener('click', () => $('app').classList.toggle('side-open'));
$('scrim').addEventListener('click', closeSide);

// ------------------------------------------------------------ messages

const toolResultFor = (id) => S.messages.find((m) => m.role === 'tool' && m.tool_call_id === id);
const attachmentsFor = (id) => S.attachments.filter((a) => a.message_id === id);

function attEl(a) {
  if (a.mime.startsWith('image/') && a.mime !== 'image/svg+xml') {
    return el('a', { class: 'att', href: '/api/attachments/' + a.id, target: '_blank', rel: 'noopener' }, el('img', { src: '/api/attachments/' + a.id, alt: a.name }));
  }
  return el('a', { class: 'att', href: '/api/attachments/' + a.id }, '📄 ', a.name, ' · ', fmtBytes(a.size));
}

function argSummary(a) {
  if (!a || typeof a !== 'object') return '';
  return Object.entries(a).map(([k, v]) => `${k}=${String(typeof v === 'string' ? v : JSON.stringify(v)).slice(0, 60)}`).join(' ').slice(0, 160);
}

function msgEl(m) {
  if (m.role === 'user') {
    return el('div', { class: 'msg user', id: 'msg-' + m.id }, el('div', { class: 'bubble' }, m.content, el('div', {}, attachmentsFor(m.id).map(attEl))));
  }
  if (m.role !== 'assistant') return null;
  const box = el('div', { class: 'msg assistant' + (m.status === 'error' ? ' error' : ''), id: 'msg-' + m.id });
  const content = el('div', { class: 'content' + (m.status === 'streaming' ? ' cursor' : '') });
  content.innerHTML = md(m.content || '');
  box.append(content);
  const tools = (m.tool_calls || []).map((c) => {
    const r = toolResultFor(c.id);
    const ok = r ? r.meta?.ok !== false : null;
    const node = el('span', { class: 'tool' + (ok === false ? ' fail' : '') }, (ok === null ? '⋯ ' : ok ? '✓ ' : '✗ ') + c.name + ' ' + argSummary(c.arguments));
    if (ok === false && r) node.title = r.content.slice(0, 500);
    return node;
  });
  if (tools.length) box.append(el('div', {}, tools));
  if (m.status === 'error') box.append(el('div', { class: 'meta' }, t('error') + ': ' + (m.meta?.error || '')));
  if (m.status === 'cancelled') box.append(el('div', { class: 'meta' }, t('stopped')));
  if (m.status === 'complete' && !(m.tool_calls || []).length && m.meta) {
    box.append(el('div', { class: 'meta' }, `${m.model || ''} · ${((m.meta.provider_ms || 0) / 1000).toFixed(1)}s`));
  }
  return box;
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
    const nearBottom = box.scrollHeight - box.scrollTop - box.clientHeight < 120;
    for (const mid of rafPending) {
      const m = S.messages.find((x) => x.id === mid);
      const old = $('msg-' + mid);
      const fresh = m && msgEl(m);
      if (old && fresh) old.replaceWith(fresh);
      else if (fresh) { box.append(fresh); $('empty').classList.add('hidden'); }
    }
    rafPending = new Set();
    if (nearBottom) box.scrollTop = box.scrollHeight;
  });
}

function approvalEl(a) {
  const decide = (approve) => guard(() => api('POST', '/approvals/' + a.approval_id, { approve }));
  return el('div', { class: 'approval', id: 'approval-' + a.approval_id },
    el('div', { class: 'spread' }, el('strong', {}, `🔐 ${a.tool} ${t('approval.q')}`),
      a.client && a.client !== 'web' ? el('span', { class: 'muted small' }, t('approval.from', { c: a.client })) : null),
    el('pre', {}, JSON.stringify(a.arguments, null, 2)),
    el('div', { class: 'row' }, el('button', { class: 'primary', onclick: () => decide(true) }, t('allow')), el('button', { onclick: () => decide(false) }, t('deny'))));
}

async function refreshApprovals() {
  try {
    const v = await api('GET', '/approvals');
    for (const a of v.approvals) if (!S.approvals.has(a.approval_id)) S.approvals.set(a.approval_id, { ...a, tool: '', arguments: {} });
    renderMessages();
  } catch {}
}

// ------------------------------------------------------------ composer

function renderComposer() {
  $('send').classList.toggle('hidden', S.generating);
  $('stop').classList.toggle('hidden', !S.generating);
  $('attach-list').replaceChildren(...S.pending.map((p, i) =>
    el('span', { class: 'att' }, p.name, el('button', { type: 'button', onclick: () => { S.pending.splice(i, 1); renderComposer(); } }, '✕'))));
}

const input = $('input');
input.addEventListener('input', () => {
  input.style.height = 'auto';
  input.style.height = Math.min(input.scrollHeight, window.innerHeight * 0.4) + 'px';
  renderSlash();
});
input.addEventListener('keydown', (e) => {
  const items = slashItems();
  if (items.length && !e.isComposing) {
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      slashSel = (slashSel + (e.key === 'ArrowDown' ? 1 : items.length - 1)) % items.length;
      return renderSlash();
    }
    if (e.key === 'Tab' || (e.key === 'Enter' && !e.shiftKey)) {
      e.preventDefault();
      return pickSlash(items[Math.min(slashSel, items.length - 1)], e.key === 'Tab');
    }
    if (e.key === 'Escape') { e.preventDefault(); input.value = ''; return renderSlash(); }
  }
  if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) { e.preventDefault(); $('composer').requestSubmit(); }
});
input.addEventListener('blur', () => setTimeout(() => $('slash').classList.add('hidden'), 150));
input.addEventListener('focus', renderSlash);

// ------------------------------------------------------------ slash commands

const COMMANDS = [
  { c: '/new', d: 'cmd.new', run: () => newChat() },
  { c: '/model', a: '<name>', d: 'cmd.model', run: (arg) => setModel(arg) },
  { c: '/rename', a: '<title>', d: 'cmd.rename', run: (arg) => renameThread(S.current, arg || undefined) },
  { c: '/delete', d: 'cmd.delete', run: () => deleteThread(S.current) },
  { c: '/attach', d: 'cmd.attach', run: () => $('file-input').click() },
  { c: '/usage', d: 'cmd.usage', run: () => showUsage() },
  { c: '/soul', d: 'cmd.soul', run: () => openSettings('soul') },
  { c: '/settings', d: 'cmd.settings', run: () => openSettings('general') },
  { c: '/help', d: 'cmd.help', run: () => { input.value = '/'; renderSlash(); input.focus(); } },
];
let slashSel = 0;

/// Suggestions for the current input: commands while typing `/name`,
/// model names after `/model `.
function slashItems() {
  const v = input.value;
  if (!v.startsWith('/') || v.includes('\n')) return [];
  const m = v.match(/^\/model\s+(\S*)$/);
  if (m) return S.models.models.filter((x) => x.includes(m[1])).map((x) => ({ c: '/model', a: x, model: x }));
  if (/\s/.test(v)) return [];
  return COMMANDS.filter((x) => x.c.startsWith(v));
}

function renderSlash() {
  const box = $('slash');
  const items = document.activeElement === input ? slashItems() : [];
  box.classList.toggle('hidden', !items.length);
  if (!items.length) { slashSel = 0; return; }
  slashSel = Math.min(slashSel, items.length - 1);
  box.replaceChildren(...items.map((it, i) => el('button', {
    type: 'button', class: i === slashSel ? 'on' : '', role: 'option',
    onmousedown: (e) => { e.preventDefault(); pickSlash(it, false); },
  }, el('span', { class: 'cmd' }, it.c), el('span', { class: 'arg' }, it.a || ''), el('span', { class: 'desc' }, it.model ? (it.model === currentModel() ? '✓' : '') : t(it.d)))));
}

function pickSlash(it, completeOnly) {
  slashSel = 0;
  if (it.model) { input.value = ''; renderSlash(); return setModel(it.model); }
  if (completeOnly || (it.a && !input.value.includes(' '))) {
    input.value = it.c + (it.a ? ' ' : '');
    renderSlash();
    return;
  }
  input.value = '';
  input.dispatchEvent(new Event('input'));
  it.run('');
}

/// Typed commands with arguments (`/rename Trip`, `/model x`).
function runCommand(text) {
  const [, name, arg = ''] = text.match(/^(\/\S+)\s*([\s\S]*)$/) || [];
  const cmd = COMMANDS.find((x) => x.c === name);
  if (!cmd) return toast(t('cmd.unknown', { c: name }));
  cmd.run(arg.trim());
}

const currentModel = () => (S.thread ? S.thread.model : S.newModel) || S.models.default;
function setModel(name) {
  if (!name) { $('model-select').focus(); return; }
  if (!S.models.models.includes(name)) return toast(t('cmd.noModel', { m: name }));
  $('model-select').value = name;
  $('model-select').dispatchEvent(new Event('change'));
}

async function ensureThread() {
  if (S.current) return S.current;
  const v = await api('POST', '/threads', { model: S.newModel || null });
  S.current = v.thread.id; S.thread = v.thread; S.messages = []; S.attachments = [];
  return S.current;
}

$('file-input').addEventListener('change', (e) => {
  const files = [...e.target.files];
  e.target.value = '';
  guard(async () => {
    const id = await ensureThread();
    for (const f of files) {
      const v = await api('POST', `/threads/${id}/attachments?name=${encodeURIComponent(f.name)}&mime=${encodeURIComponent(f.type || '')}`, f);
      S.pending.push(v);
      S.attachments.push({ ...v, message_id: null });
    }
    renderAll();
  });
});

$('composer').addEventListener('submit', async (e) => {
  e.preventDefault();
  const text = input.value.trim();
  if (text.startsWith('/') && !S.pending.length) {
    input.value = '';
    input.dispatchEvent(new Event('input'));
    return runCommand(text);
  }
  if ((!text && !S.pending.length) || S.generating) return;
  try {
    const id = await ensureThread();
    const atts = S.pending.map((p) => p.id);
    input.value = '';
    input.dispatchEvent(new Event('input'));
    const r = await api('POST', `/threads/${id}/messages`, { content: text, client: 'web', attachments: atts });
    S.pending = [];
    for (const m of [r.user_message, r.assistant_message]) upsertMessage(m);
    for (const a of atts) { const x = S.attachments.find((y) => y.id === a); if (x) x.message_id = r.user_message.id; }
    S.generating = true;
    renderAll();
  } catch (err) {
    toast(err.message);
    if (!input.value) input.value = text;
  }
});

$('stop').addEventListener('click', () => { if (S.current) guard(() => api('POST', `/threads/${S.current}/stop`)); });

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
  try { await loadThreads(); if (S.current) await openThread(S.current); } catch {}
}

const endsTurn = (m) => m.role === 'assistant' && (m.status !== 'complete' || !(m.tool_calls || []).length);

function handleEvent(v) {
  const here = v.thread_id && v.thread_id === S.current;
  switch (v.type) {
    case 'resync': resync(); break;
    case 'thread.created': case 'thread.updated': {
      const i = S.threads.findIndex((x) => x.id === v.thread.id);
      if (i >= 0) S.threads[i] = { ...S.threads[i], ...v.thread }; else S.threads.unshift(v.thread);
      S.threads.sort((a, b) => b.updated_at - a.updated_at);
      if (here) { S.thread = v.thread; renderHeader(); }
      renderThreads();
      break;
    }
    case 'thread.deleted':
      S.threads = S.threads.filter((x) => x.id !== v.thread_id);
      for (const [k, a] of S.approvals) if (a.thread_id === v.thread_id) S.approvals.delete(k);
      if (here) { newChat(); toast(t('deleted')); } else renderThreads();
      break;
    case 'threads.cleared': S.threads = []; S.approvals.clear(); newChat(); break;
    case 'message.created': {
      const th = S.threads.find((x) => x.id === v.thread_id);
      if (th) { th.updated_at = v.ts; if (v.message.status === 'streaming') th.generating = true; S.threads.sort((a, b) => b.updated_at - a.updated_at); renderThreads(); }
      else loadThreads().catch(() => {});
      if (!here) break;
      upsertMessage(v.message);
      if (v.message.role === 'user') api('GET', `/threads/${S.current}/messages`).then((m) => { S.attachments = m.attachments; rerenderMessage(v.message.id); }).catch(() => {});
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
        if (endsTurn(v.message)) { S.generating = false; renderComposer(); }
        rerenderMessage(v.message.id);
      }
      const th = S.threads.find((x) => x.id === v.thread_id);
      if (th && endsTurn(v.message)) { th.generating = false; renderThreads(); }
      break;
    }
    case 'tool.started': case 'tool.completed':
      if (here) api('GET', `/threads/${S.current}/messages`).then((m) => { if (S.current === v.thread_id) { S.messages = m.messages; rerenderMessage(v.message_id); } }).catch(() => {});
      break;
    case 'tool.approval_required':
      S.approvals.set(v.approval_id, v);
      if (here) renderMessages(); else toast(t('approval.other', { t: v.tool }));
      break;
    case 'tool.approval_resolved': S.approvals.delete(v.approval_id); $('approval-' + v.approval_id)?.remove(); break;
    case 'config.updated': loadModels().then(renderHeader).catch(() => {}); break;
  }
}

// ------------------------------------------------------------ settings

async function loadModels() { S.models = await api('GET', '/models'); }

function openSettings(tab) { $('settings').classList.remove('hidden'); showTab(tab); closeSide(); }
$('open-settings').addEventListener('click', () => openSettings('general'));
$('close-settings').addEventListener('click', () => $('settings').classList.add('hidden'));
$('settings').addEventListener('click', (e) => { if (e.target.id === 'settings') $('settings').classList.add('hidden'); });
$('close-modal').addEventListener('click', () => $('modal').classList.add('hidden'));
$('modal').addEventListener('click', (e) => { if (e.target.id === 'modal') $('modal').classList.add('hidden'); });
document.addEventListener('keydown', (e) => {
  if (e.key !== 'Escape') return;
  $('settings').classList.add('hidden'); $('modal').classList.add('hidden'); closeThreadMenu();
});

function showModal(title, ...nodes) {
  $('modal-title').textContent = title;
  $('modal-body').replaceChildren(...nodes);
  $('modal').classList.remove('hidden');
}

function relTime(ms) {
  const m = Math.max(0, Math.round((ms - Date.now()) / 60000));
  const rtf = new Intl.RelativeTimeFormat(LANG, { numeric: 'auto' });
  if (m < 60) return rtf.format(m, 'minute');
  if (m < 1440) return rtf.format(Math.round(m / 60), 'hour');
  return rtf.format(Math.round(m / 1440), 'day');
}
const compact = (n) => n >= 1e6 ? (n / 1e6).toFixed(1) + 'M' : n >= 1e3 ? (n / 1e3).toFixed(1) + 'k' : String(n);

async function showUsage() {
  showModal(t('u.title'), el('p', { class: 'muted' }, t('u.checking')));
  let v;
  try { v = await api('GET', '/usage'); } catch (e) { return showModal(t('u.title'), el('p', { class: 'error' }, e.message)); }
  const subs = v.subscriptions.map((sub) => {
    if (!sub.ok) return el('div', { class: 'panel' }, el('h4', {}, sub.id, ' ', badge('ng', t('m.ng'))), el('p', { class: 'hint' }, sub.error));
    const L = sub.limits;
    const win = (w) => {
      if (!w) return null;
      const name = w.window_minutes === 300 ? t('u.win.5h') : w.window_minutes === 10080 ? t('u.win.weekly') : `${Math.round(w.window_minutes / 60)}h`;
      const used = w.used_percent;
      const bar = el('i');
      bar.style.width = Math.min(100, used) + '%';
      return el('div', { class: 'field' },
        el('div', { class: 'spread' }, el('strong', {}, name), el('span', {}, el('b', {}, t('u.left', { p: Math.max(0, 100 - used) })), ' · ', el('span', { class: 'muted small' }, t('u.reset', { t: relTime(w.resets_at) })))),
        el('div', { class: 'meter' + (used >= 90 ? ' ng' : used >= 70 ? ' warn' : '') }, bar));
    };
    return el('div', { class: 'panel' },
      el('h4', {}, sub.id, ' ', L.plan ? badge('ok', L.plan) : null), el('br'),
      win(L.primary), win(L.secondary),
      L.reset_credits ? el('p', { class: 'hint' }, t('u.credits', { n: L.reset_credits })) : null);
  });
  const table = (rows) => rows.length
    ? el('table', { class: 'usage' }, el('tr', {}, el('th', {}, t('u.model')), el('th', {}, t('u.in')), el('th', {}, t('u.out')), el('th', {}, t('u.replies'))),
      ...rows.map((r) => el('tr', {}, el('td', {}, r.model), el('td', {}, compact(r.input_tokens)), el('td', {}, compact(r.output_tokens)), el('td', {}, r.responses))))
    : el('p', { class: 'muted small' }, '—');
  showModal(t('u.title'),
    ...(subs.length ? subs : [el('p', { class: 'muted' }, t('u.none'))]),
    el('h3', {}, t('u.tokens')),
    el('p', { class: 'small muted' }, t('u.day')), table(v.tokens.day),
    el('p', { class: 'small muted' }, t('u.month')), table(v.tokens.month));
}
$('settings-tabs').addEventListener('click', (e) => { if (e.target.dataset.tab) showTab(e.target.dataset.tab); });

let currentTab = 'general';
async function showTab(name) {
  currentTab = name;
  for (const b of $('settings-tabs').children) b.classList.toggle('active', b.dataset.tab === name);
  const box = $('settings-content');
  box.replaceChildren(el('p', { class: 'muted' }, t('loading')));
  try {
    const node = await TABS[name]();
    if (currentTab === name) box.replaceChildren(...[node].flat());
  } catch (e) {
    box.replaceChildren(el('p', { class: 'error' }, e.message));
  }
}

function applyTheme(mode) {
  if (mode === 'light' || mode === 'dark') document.documentElement.dataset.theme = mode;
  else delete document.documentElement.dataset.theme;
}

function chipsInput(values, numeric) {
  const list = [...values.map(String)];
  const box = el('div', { class: 'chips' });
  const entry = el('input', { type: 'text', placeholder: t('c.idsPh'), inputmode: 'numeric' });
  const render = () => box.replaceChildren(...list.map((v, i) => el('span', { class: 'chip' }, v, el('button', { type: 'button', title: '✕', onclick: () => { list.splice(i, 1); render(); } }, '✕'))), entry);
  const add = () => {
    for (const part of entry.value.split(/[\s,]+/)) if (/^\d+$/.test(part) && !list.includes(part)) list.push(part);
    entry.value = '';
    render();
    entry.focus();
  };
  entry.addEventListener('keydown', (e) => { if ((e.key === 'Enter' || e.key === ',') && !e.isComposing) { e.preventDefault(); add(); } });
  entry.addEventListener('blur', () => { if (entry.value.trim()) add(); });
  render();
  return { node: box, values: () => { if (entry.value.trim()) add(); return numeric ? list.map(Number) : list; } };
}

function channelPanel(c, st, name, icon, steps) {
  const g = c.channels[name];
  const secret = `${name}.bot_token`;
  const hasToken = c.secrets_set.includes(secret);
  const state = !g.enabled ? (hasToken ? 'disabled' : 'notset') : st.state;
  const badgeKind = { connected: 'ok', connecting: 'warn busy', error: 'ng' }[state] || '';
  const badgeText = t('c.st.' + state) + (st.detail && g.enabled ? ` · ${st.detail}` : '');
  const sw = toggle(g.enabled, t('c.enable'));
  const token = el('input', { type: 'password', placeholder: hasToken ? t('c.tokenSet') : t('c.token'), autocomplete: 'off' });
  const ids = chipsInput(g.allowed_user_ids, name === 'telegram');
  const save = () => guard(async () => {
    const list = ids.values();
    if (sw.input.checked && !list.length) throw new Error(t('c.needId'));
    if (sw.input.checked && !hasToken && !token.value) throw new Error(t('c.needToken'));
    if (token.value) await api('PUT', '/secrets/' + secret, { value: token.value.trim() });
    await api('PATCH', '/config', { [`channels.${name}.enabled`]: sw.input.checked, [`channels.${name}.allowed_user_ids`]: list });
    toast(t('saved'));
    setTimeout(() => showTab('channels'), 1500);
  });
  return el('div', { class: 'panel' },
    el('div', { class: 'spread' },
      el('div', {}, el('h4', {}, icon, ' ', name === 'telegram' ? 'Telegram' : 'Discord', ' ', badge(badgeKind, badgeText)), el('div', { class: 'sub' }, t(steps.sub))),
      sw.node),
    el('ol', { class: 'steps' },
      el('li', {}, el('div', {}, el('div', { class: 'step-title' }, t(steps.s1)), el('div', { class: 'hint' }, t(steps.s1b)))),
      el('li', {}, el('div', {}, el('div', { class: 'step-title' }, t(steps.s2)), token)),
      el('li', {}, el('div', {}, el('div', { class: 'step-title' }, t(steps.s3)), ids.node, el('p', { class: 'hint' }, t(steps.s3b) + ' ' + t('c.idsHint'))))),
    el('div', { class: 'row' }, el('button', { class: 'primary', onclick: save }, t('c.save')),
      hasToken ? el('button', { class: 'ghost danger', onclick: () => {
        if (confirm(t('c.removeq', { n: name }))) guard(async () => {
          await api('PATCH', '/config', { [`channels.${name}.enabled`]: false });
          await api('DELETE', '/secrets/' + secret);
          showTab('channels');
        });
      } }, t('c.remove')) : null));
}

async function googlePanel(c) {
  const g = await api('GET', '/google');
  const id = el('input', { type: 'text', value: c.tools.google.client_id, placeholder: '….apps.googleusercontent.com' });
  const secret = el('input', { type: 'password', placeholder: g.client_secret_set ? t('m.keySet') : t('t.csecret') });
  const boxes = g.available_services.map((sv) => {
    const sw = toggle(g.services.includes(sv), sv);
    sw.input.dataset.svc = sv;
    return sw;
  });
  const save = async () => {
    await api('PATCH', '/config', { 'tools.google.client_id': id.value.trim(), 'tools.google.services': boxes.filter((b) => b.input.checked).map((b) => b.input.dataset.svc) });
    if (secret.value) await api('PUT', '/secrets/google.client_secret', { value: secret.value });
  };
  return el('div', { class: 'panel' },
    el('div', { class: 'spread' }, el('h4', {}, 'Google ', g.connected ? badge('ok', t('t.connected')) : badge('', t('t.notConnected')))),
    el('p', { class: 'hint' }, t('t.gNote')),
    field(t('t.cid'), id), field(t('t.csecret'), secret),
    el('div', { class: 'row' }, ...boxes.map((b) => b.node)),
    el('div', { class: 'row' }, el('br')),
    el('div', { class: 'row' },
      el('button', { onclick: () => guard(async () => { await save(); toast(t('saved')); showTab('tools'); }) }, t('save')),
      el('button', { class: 'primary', onclick: () => guard(async () => {
        await save();
        const r = await api('POST', '/google/login', {});
        window.open(r.auth_url, '_blank', 'noopener');
        toast(t('t.gToast'));
      }) }, g.connected ? t('t.reconnect') : t('t.connect')),
      g.connected ? el('button', { class: 'ghost danger', onclick: () => guard(async () => { await api('POST', '/google/logout', {}); showTab('tools'); }) }, t('t.disconnect')) : null));
}

function webPanel(c) {
  const prov = el('select', {}, el('option', { value: '' }, t('t.off')), el('option', { value: 'brave' }, 'Brave Search API'), el('option', { value: 'searxng' }, 'SearXNG'));
  prov.value = c.tools.web.search_provider;
  const sx = el('input', { type: 'url', placeholder: 'https://searx.example.com', value: c.tools.web.searxng_url });
  const key = el('input', { type: 'password', placeholder: c.secrets_set.includes('brave.api_key') ? t('m.keySet') : t('t.braveKey') });
  const sxField = field('SearXNG URL', sx);
  const keyField = field(t('t.braveKey'), key);
  const sync = () => { sxField.classList.toggle('hidden', prov.value !== 'searxng'); keyField.classList.toggle('hidden', prov.value !== 'brave'); };
  prov.addEventListener('change', sync);
  sync();
  return el('div', { class: 'panel' }, el('h4', {}, '🔎 ', t('t.web')),
    el('p', { class: 'hint' }, t('t.webNote')), field(t('t.provider'), prov), sxField, keyField,
    el('button', { class: 'primary', onclick: () => guard(async () => {
      await api('PATCH', '/config', { 'tools.web.search_provider': prov.value, 'tools.web.searxng_url': sx.value });
      if (key.value) await api('PUT', '/secrets/brave.api_key', { value: key.value });
      toast(t('saved')); showTab('tools');
    }) }, t('save')));
}

const TABS = {
  async general() {
    const c = await api('GET', '/config');
    S.config = c;
    const inp = (v, type = 'text') => el('input', { type, value: v ?? '' });
    const name = inp(c.agent.name), bind = inp(c.server.bind), port = inp(c.server.port, 'number'), pub = inp(c.server.public_url, 'url');
    const lang = el('select', {}, el('option', { value: 'ja' }, '日本語'), el('option', { value: 'en' }, 'English'));
    lang.value = LANG;
    let theme = c.agent.appearance;
    const seg = el('div', { class: 'seg' });
    const renderSeg = () => seg.replaceChildren(...['system', 'light', 'dark'].map((m) =>
      el('button', { type: 'button', class: m === theme ? 'on' : '', onclick: () => { theme = m; applyTheme(m); renderSeg(); } }, t('theme.' + m))));
    renderSeg();
    const save = () => guard(async () => {
      await api('PATCH', '/config', {
        'agent.name': name.value, 'agent.language': lang.value, 'agent.appearance': theme,
        'server.bind': bind.value, 'server.port': Number(port.value), 'server.public_url': pub.value,
      });
      const restart = bind.value !== c.server.bind || Number(port.value) !== c.server.port;
      const newLang = lang.value.toLowerCase().startsWith('ja') ? 'ja' : 'en';
      if (newLang !== LANG) { LANG = newLang; applyI18n(); renderAll(); showTab('general'); }
      toast(restart ? t('g.restart') : t('saved'));
    });
    return [
      el('div', { class: 'panel' }, el('h4', {}, t('g.agent')), el('br'), field(t('g.name'), name), field(t('g.uiLang'), lang, t('g.soulHint')), field(t('g.theme'), seg)),
      el('details', { class: 'adv panel' }, el('summary', {}, t('g.server')), field(t('g.bind'), bind), field(t('g.port'), port), field(t('g.public'), pub)),
      el('div', { class: 'row' }, el('button', { class: 'primary', onclick: save }, t('save')),
        el('button', { class: 'ghost', onclick: () => guard(async () => { await api('POST', '/logout'); showLogin(); }) }, t('signout'))),
    ];
  },

  async soul() {
    const v = await api('GET', '/soul');
    const area = el('textarea', { class: 'soul', spellcheck: 'false' });
    area.value = v.content;
    return [
      el('p', { class: 'hint' }, t('soul.desc')),
      el('p', { class: 'hint' }, v.path),
      area,
      el('div', { class: 'row' }, el('br')),
      el('div', { class: 'row' },
        el('button', { class: 'primary', onclick: () => guard(async () => { await api('PUT', '/soul', { content: area.value }); toast(t('saved')); }) }, t('save')),
        el('button', { class: 'ghost', onclick: () => { if (confirm(t('soul.resetq'))) area.value = v.template; } }, t('soul.reset'))),
    ];
  },

  async models() {
    const v = await api('GET', '/models');
    S.models = v;
    const statusBadges = new Map();
    const panels = v.providers.map((p) => {
      const cap = p.capabilities || {};
      const st = badge('warn busy', t('m.checking'));
      statusBadges.set(p.id, st);
      const needsKey = ['openai', 'anthropic', 'gemini', 'openai-compatible'].includes(p.kind);
      const keyInput = el('input', { type: 'password', placeholder: p.has_key ? t('m.keySet') : t('m.key') });
      return el('div', { class: 'panel' },
        el('div', { class: 'spread' },
          el('div', {}, el('h4', {}, p.id, ' ', st), el('div', { class: 'sub' }, `${p.kind} · ${p.base_url}`)),
          el('button', { class: 'ghost danger', onclick: () => { if (confirm(t('m.removeq', { p: p.id }))) guard(async () => { await api('DELETE', '/providers/' + p.id); showTab('models'); }); } }, t('m.remove'))),
        el('div', { class: 'sub' }, t('m.caps', { t: cap.tool_calling, i: cap.image_input, s: cap.streaming })),
        el('div', { class: 'row' }, el('br')),
        el('div', { class: 'list' }, ...p.models.map((m) => {
          const id = `${p.id}/${m}`;
          const res = el('span', { class: 'small muted' });
          return el('div', { class: 'item' },
            el('div', { class: 'grow' }, m, ' ', id === v.default ? badge('ok', t('m.default')) : null, ' ', res),
            el('div', { class: 'row' },
              id === v.default ? null : el('button', { class: 'ghost', onclick: () => guard(async () => { await api('PUT', '/models/default', { model: id }); showTab('models'); }) }, t('m.makeDefault')),
              el('button', { onclick: async () => {
                res.textContent = t('m.testing');
                const r = await api('POST', '/models/test', { model: id }).catch((e) => ({ ok: false, error: e.message }));
                res.textContent = r.ok ? `✓ ${r.ttft_ms ?? '-'} ms / ${r.total_ms} ms` : `✗ ${r.error}`;
                const b = statusBadges.get(p.id);
                b.className = 'badge ' + (r.ok ? 'ok' : 'ng');
                b.textContent = r.ok ? t('m.ok') : t('m.ng');
              } }, t('m.test'))));
        })),
        ['codex-cli', 'gemini-cli'].includes(p.kind) ? el('p', { class: 'hint' }, t('m.cliLogin', { p: p.id })) : null,
        needsKey ? el('div', { class: 'copyfield' }, keyInput, el('button', { onclick: () => guard(async () => {
          if (!keyInput.value) return;
          await api('PUT', `/secrets/provider.${p.id}.api_key`, { value: keyInput.value });
          toast(t('saved')); showTab('models');
        }) }, t('m.saveKey'))) : null);
    });
    // Live availability check (no tokens spent).
    api('GET', '/providers/status').then((r) => {
      for (const s of r.providers) {
        const b = statusBadges.get(s.id);
        if (!b) continue;
        b.className = 'badge ' + (s.status === 'ok' ? 'ok' : s.status === 'ng' ? 'ng' : '');
        b.textContent = (s.status === 'ok' ? t('m.ok') : s.status === 'ng' ? t('m.ng') : t('m.unknown')) + (s.status === 'ok' ? '' : ` · ${s.detail}`);
        b.title = s.detail;
      }
    }).catch(() => {});

    const kind = el('select', {}, ...['openai', 'anthropic', 'gemini', 'openai-compatible', 'ollama', 'codex-cli', 'gemini-cli'].map((k) => el('option', { value: k }, k)));
    const id = el('input', { type: 'text', placeholder: t('m.id') });
    const base = el('input', { type: 'text', placeholder: t('m.base') });
    const models = el('input', { type: 'text', placeholder: t('m.models') });
    const key = el('input', { type: 'password', placeholder: t('m.key') });
    kind.addEventListener('change', () => {
      if (kind.value === 'anthropic' && !models.value) models.value = 'claude-opus-5-5';
      if (['codex-cli', 'gemini-cli'].includes(kind.value)) models.value = 'default';
    });
    const add = () => guard(async () => {
      await api('POST', '/providers', { id: id.value || kind.value, kind: kind.value, base_url: base.value, models: models.value.split(',').map((s) => s.trim()).filter(Boolean), api_key: key.value || null });
      toast(t('m.added')); showTab('models');
    });
    return [
      ...(panels.length ? panels : [el('p', { class: 'muted' }, t('m.none'))]),
      el('div', { class: 'panel' }, el('h4', {}, '＋ ', t('m.add')), el('br'),
        field(t('m.kind'), kind), field('ID', id), field(t('m.base'), base), field(t('m.models'), models), field(t('m.key'), key),
        el('button', { class: 'primary', onclick: add }, t('m.addBtn')),
        el('p', { class: 'hint' }, t('m.sub'))),
    ];
  },

  async channels() {
    const [c, st] = await Promise.all([api('GET', '/config'), api('GET', '/channels/status')]);
    const url = c.server.public_url || location.origin;
    const copyUrl = el('button', { onclick: () => navigator.clipboard?.writeText(url).then(() => toast(t('copied'))) }, t('copy'));
    return [
      el('div', { class: 'panel' },
        el('h4', {}, '📱 ', t('c.iphone')), el('div', { class: 'sub' }, t('c.iphone.sub')),
        el('ol', { class: 'steps' },
          el('li', {}, el('div', { class: 'step-title' }, t('c.ios1'))),
          el('li', {}, el('div', {}, el('div', { class: 'step-title' }, t('c.ios2')), el('div', { class: 'copyfield' }, el('input', { type: 'text', value: url, readonly: true }), copyUrl))),
          el('li', {}, el('div', {}, el('div', { class: 'step-title' }, t('c.ios3')), el('div', { class: 'hint' }, t('c.ios3.hint'))))),
        el('p', { class: 'hint' }, t('c.remote'))),
      channelPanel(c, st.telegram, 'telegram', '✈️', { sub: 'c.tg.sub', s1: 'c.tg1', s1b: 'c.tg1.b', s2: 'c.tg2', s3: 'c.tg3', s3b: 'c.tg3.b' }),
      channelPanel(c, st.discord, 'discord', '🎮', { sub: 'c.dc.sub', s1: 'c.dc1', s1b: 'c.dc1.b', s2: 'c.dc2', s3: 'c.dc3', s3b: 'c.dc3.b' }),
      el('p', { class: 'hint' }, t('c.del')),
    ];
  },

  async tools() {
    const [v, c] = await Promise.all([api('GET', '/tools'), api('GET', '/config')]);
    const groups = { fs: '📁 ' + t('t.fs'), web: '🔎 ' + t('t.web'), google: 'Google' };
    const lists = {};
    for (const tool of v.tools) {
      const sel = el('select', {}, ...['disabled', 'ask', 'allow'].map((p) => el('option', { value: p }, t('t.' + p))));
      sel.value = tool.permission;
      sel.addEventListener('change', () => guard(async () => { await api('PATCH', '/tools', { [tool.name]: sel.value }); toast(`${tool.name}: ${t('t.' + sel.value)}`); }));
      (lists[tool.group] ||= []).push(el('div', { class: 'item' },
        el('div', { class: 'grow' }, el('strong', {}, tool.name), el('div', { class: 'small muted' }, tool.description + (tool.available ? '' : ' ' + t('t.notcfg')))),
        el('div', {}, sel)));
    }
    const ws = el('input', { type: 'text', value: c.tools.fs.workspace || '', placeholder: '~/.alice/workspace' });
    const roots = el('textarea', { rows: 3, placeholder: '~/Documents' });
    roots.value = (c.tools.fs.roots || []).join('\n');
    return [
      el('h3', {}, t('t.perm')),
      ...Object.entries(lists).map(([g, items]) => el('div', {}, el('p', { class: 'small muted' }, groups[g] || g), el('div', { class: 'list' }, items))),
      el('p', { class: 'hint' }, t('t.permNote')),
      el('h3', {}, t('t.fs')),
      el('div', { class: 'panel' }, field(t('t.ws'), ws, t('t.wsHint') + ' ' + t('t.effective') + v.workspace),
        field(t('t.extra'), roots, t('t.effective') + v.fs_roots.join(', ')),
        el('button', { class: 'primary', onclick: () => guard(async () => {
          await api('PATCH', '/config', { 'tools.fs.workspace': ws.value.trim(), 'tools.fs.roots': roots.value.split('\n').map((s) => s.trim()).filter(Boolean) });
          toast(t('saved')); showTab('tools');
        }) }, t('t.saveDirs'))),
      webPanel(c),
      await googlePanel(c),
    ];
  },

  async memory() {
    const st = (await api('GET', '/memories/status')).status;
    const q = el('input', { type: 'text', placeholder: t('mem.search') });
    const list = el('div', { class: 'list' });
    const detail = el('div');
    const load = async () => {
      const v = await api('GET', '/memories?limit=500' + (q.value ? '&q=' + encodeURIComponent(q.value) : ''));
      list.replaceChildren(...(v.memories.length ? v.memories.map((m) =>
        el('div', { class: 'item' },
          el('div', { class: 'grow' }, m.content, el('div', { class: 'small muted' }, `${m.key} · ${t('mem.count', { n: m.source_count })} · ${fmtTime(m.updated_at)}`)),
          el('button', { class: 'ghost', onclick: () => showSources(m.id) }, t('mem.sources'))))
        : [el('div', { class: 'item muted' }, t('mem.none'))]));
    };
    const showSources = async (id) => {
      const v = await api('GET', '/memories/' + id);
      detail.replaceChildren(el('h3', {}, t('mem.sources')), el('div', { class: 'list' }, ...v.sources.map((s) =>
        el('div', { class: 'item' }, el('div', { class: 'grow' }, s.content, el('div', { class: 'small muted' }, `${s.thread_title || s.thread_id} · ${fmtTime(s.created_at)}`)),
          el('button', { class: 'ghost', onclick: () => { $('settings').classList.add('hidden'); openThread(s.thread_id); } }, t('mem.open'))))));
      detail.scrollIntoView({ behavior: 'smooth' });
    };
    q.addEventListener('input', () => { clearTimeout(q.timer); q.timer = setTimeout(load, 200); });
    await load();
    const stat = (n, label) => el('div', {}, el('b', {}, n), label);
    return [
      el('div', { class: 'stat' }, stat(st.memories, t('st.memories')), stat(st.sources, t('st.sources')), stat(st.threads, t('st.threads')),
        stat(fmtBytes(st.db_bytes), t('st.db')), stat(fmtBytes(st.attachments_bytes), t('st.att'))),
      q, el('div', { class: 'row' }, el('br')), list, detail,
      el('div', { class: 'panel' }, el('div', { class: 'row' },
        el('button', { onclick: () => { if (confirm(t('mem.clearq'))) guard(async () => { await api('DELETE', '/memories', { confirm: 'clear-memories' }); showTab('memory'); }); } }, t('mem.clear')),
        el('button', { class: 'danger', onclick: () => { if (prompt(t('mem.allq')) === 'delete-all') guard(async () => { await api('DELETE', '/threads', { confirm: 'delete-all' }); showTab('memory'); }); } }, t('mem.all'))),
        el('p', { class: 'hint' }, t('mem.note'))),
    ];
  },

  async system() {
    const [s, logs] = await Promise.all([api('GET', '/status'), api('GET', '/logs?lines=200')]);
    const up = s.uptime_secs;
    const stat = (n, label) => el('div', {}, el('b', {}, n), label);
    return [
      el('div', { class: 'stat' },
        stat(s.version, t('st.version')), stat(`${Math.floor(up / 3600)}h ${Math.floor(up / 60) % 60}m`, t('st.uptime')),
        stat((s.rss_kb / 1024).toFixed(1) + ' MB', t('st.mem')), stat(s.cpu_percent + '%', t('st.cpu')),
        stat(fmtBytes(s.db_bytes), t('st.db')), stat(s.counts.threads, t('st.threads')), stat(s.counts.messages, t('st.messages'))),
      el('p', { class: 'hint' }, `${s.profile} · ${s.os}/${s.arch} · ${s.bind} · ${s.home}`),
      el('h3', {}, t('sys.logs')), el('pre', { class: 'log' }, logs.lines || '—'),
      el('button', { onclick: () => showTab('system') }, t('refresh')),
    ];
  },
};

// ------------------------------------------------------------ start

async function start() {
  applyI18n();
  try { await loadModels(); } catch (e) { if (e.message !== 'unauthorized') toast(e.message); return; }
  $('app').classList.remove('hidden');
  try {
    const c = await api('GET', '/config');
    applyTheme(c.agent.appearance);
    if (c.agent.language) LANG = c.agent.language.toLowerCase().startsWith('ja') ? 'ja' : 'en';
    applyI18n();
    S.profile = c.profile;
    if (c.profile && c.profile !== 'default') {
      document.title = `Alice · ${c.profile}`;
      $('profile-tag').textContent = t('profile', { p: c.profile });
    }
  } catch {}
  connectEvents();
  await loadThreads();
  let last = null;
  try { last = localStorage.getItem('alice.thread'); } catch {}
  if (last && S.threads.some((x) => x.id === last)) await openThread(last); else newChat();
}

start();
