# Alice Agent — Security review

Version 0.1.0. The threat model for a single-user, self-hosted agent, what is done about each threat, and the risks that remain.

## 1. Threat model

| Threat | Examples |
|---|---|
| Others on the network | API access from the LAN or the internet |
| Malicious websites (through the browser) | CSRF, DNS rebinding, XSS |
| Prompt injection | Instructions hidden in web pages, mail, files or attachments |
| Others on chat platforms | Telegram / Discord messages from people who found the bot |
| Other users on the same machine | Reading the settings or the DB file |

## 2. Countermeasures

### Auth and network
- Binds to `127.0.0.1` by default (loopback is always on; `server.bind` adds LAN / Tailscale addresses). A non-loopback address logs a warning at startup; Tailscale or an HTTPS reverse proxy is recommended.
- The API token is 256 random bits by default. A self-chosen one (`alice token --set`) must be 12+ characters of ASCII that is safe in headers and cookies.
- Wrong tokens are counted per address; 30 in a minute locks that address out for a minute (against guessing).
- Every API except `/api/health` and the OAuth callback needs the token (local connections included). Comparison is constant time, and a failure waits 500 ms.
- The Web UI uses an HttpOnly `SameSite=Strict` cookie (`Secure` over HTTPS). Cookie requests that change state need the `X-Alice-CSRF` header, which other sites cannot add.
- The cookie name differs per profile (cookies are not separated by port).
- DNS rebinding: the attacker's domain gets no cookie and does not know the token, so it cannot reach the API.

### Web UI
- CSP `default-src 'self'` (no inline scripts or styles), `frame-ancestors 'none'`, `nosniff`, `no-referrer`.
- Markdown escapes all HTML first, then adds a small set of tags; links must be http(s). Code highlighting cuts code into tokens before escaping, so it adds only its own spans. Tests cover XSS payloads.
- HTML mail is served from its own URL under a strict CSP (inline styles only, no scripts or forms, remote images only on request) inside a sandboxed frame.
- Attachments other than images (SVG excluded) are served as `application/octet-stream` with `Content-Disposition: attachment`.

### Tools and prompt injection
- Tool permissions (disabled / ask / allow) live only in `config.toml`; no tool or API lets the model change them.
- Destructive actions and anything that sends data out need confirmation by default: writing, editing, moving or deleting files, `web_fetch` (so a URL cannot leak information), sending mail, creating or deleting events, writing Docs/Sheets.
- Outside content (web, mail, files, attachments, search results, tool results, earlier chats found by recall) is wrapped in `<untrusted>`, and the system prompt says not to follow it.
- File tools stay inside the configured roots. `..` is refused, symlinks are judged after resolving, and walks do not follow links. **Alice's data directory (`~/.alice`, every profile) is always refused, except the workspace, whatever the roots** (so API keys cannot be read and permissions cannot be rewritten). New paths are checked in full, not only their existing parent.
- `web_fetch`: http(s) only, redirects followed by hand, and every hop refused for private / loopback / link-local / CGNAT addresses (against SSRF).
- Memories come only from the user's own messages (web or mail content never becomes a memory) and are marked as data, not instructions, when injected.
- Sending Gmail refuses header injection (line breaks).

### Channels
- Telegram / Discord handle only allow-listed user IDs and ignore everyone else (only the ID is logged). Approval buttons check the same list.
- URLs that contain the bot token are stripped from error messages (`without_url`).

### Secrets and data
- `secrets.toml`, `config.toml` and `alice.db` are 0600; the data directory is 0700. No API can read a secret back (set and delete only).
- Logs never contain tokens, API keys or message text (thread IDs, timings and error kinds only).
- Skill sign-in: PKCE (S256) + single-use `state` + a loopback redirect. Refresh tokens and tokens live only in `secrets.toml` (`skill.<id>.*`), access tokens only in memory. Disconnecting revokes them at the provider. Token strings are removed from tool output, errors and logs.
- Skills: outside skills stay disabled when found. Enabling shows what they declare (network hosts, files, local execution). `http` tools can reach only the declared hosts, and arguments are URL-encoded so they cannot change the path or host. `command` tools get an empty environment except PATH/HOME/LANG/TMPDIR and their own token. Skill folders are under `~/.alice`, so models cannot rewrite them.
- Codex: its own CODEX_HOME with shell, browser, plugins, subagents, hooks and web search off (code mode's `exec`, needed for MCP tools, stays on: it is a V8 isolate without files or network). Read-only sandbox, empty working directory. Before tools are given, the tools the model would see are checked, and anything outside the allow list switches to the text protocol (fail closed). `alice mcp` holds only a turn token, which works for nothing but `/api/mcp/*` (not approvals, settings or secrets). Approvals happen only in Alice's UI.
- Antigravity CLI: plan mode, `--sandbox`, an empty working directory; headless runs refuse actions that need approval. Alice's tools go through the Tool Gate with the text protocol.
- Deletion: `secure_delete`, `TRUNCATE` WAL checkpoints, FTS `optimize`, `VACUUM` on delete-all. A test checks that deleted strings do not remain in the DB file.
- iPhone: connection details in the Keychain (`AfterFirstUnlockThisDeviceOnly`), an ephemeral URLSession (no disk cache or cookies), images in memory only. For offline reading, the last loaded events, mail (list and recent bodies), tasks and chats (the 20 most recent threads) are kept in Application Support with complete data protection (unreadable while locked), excluded from backups, and removed when switching servers. Alice's screen comments send only counts and times unless the user allows titles and senders.

## 3. Remaining risks (limits of the design)

| Risk | What it means | Advice |
|---|---|---|
| Data already sent out | Whatever reached a model provider, Telegram/Discord or Google is beyond Alice's deletion | Use a local model (Ollama) for very sensitive content |
| Codex CLI / Antigravity CLI | These are agents themselves and may **read** local files even when sandboxed. They run outside Alice's Tool Gate | Use them with trusted input; for web or mail content, prefer an API connection |
| Tools switched to `allow` | Run without asking, so injection affects them more | Keep tools that send data out (`web_fetch`, `gmail_send`) on `ask` |
| Broad file roots | Allowing `~` makes `~/.ssh` and the like readable (writes still ask by default) | Allow only the folders you need |
| Local privileges | Any process running as the same OS user can read `secrets.toml` | Relies on OS account separation and disk encryption |
| Backups and snapshots | Data left in Time Machine and the like cannot be erased | Exclude `~/.alice` from backups if needed |
| Who approves | Any signed-in client can approve | By design, for a single user |

## 4. How it is checked

Automated tests (`cargo test`) cover:
unauthenticated requests refused, cookie + CSRF, no secrets in API responses, CSP headers, the mail frame's CSP / tool permissions (disabled, ask, allow) / path traversal, symlink escapes, data directory protection / SSRF (private addresses refused) / XSS in Markdown / mail header injection / PKCE and `state` checks / chat users outside the allow list ignored / no deleted strings left in the DB file / the wrong-token lockout.
