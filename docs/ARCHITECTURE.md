# Alice Agent (A2) — Architecture

## 1. Overview

```
 alice (CLI) ─┐
 Web (embed) ─┤ HTTP REST + SSE (Bearer / Cookie)
 iPhone App ──┤
              ▼
        ┌──────────────── alice serve (one process / tokio) ────────────────┐
        │ server   : axum router, auth, SSE bus                            │
        │ agent    : turn loop (context → compaction → provider → tools)   │
        │ memory   : extract / search / delete / rebuild                   │
        │ providers: openai(-compatible) / anthropic / gemini / CLIs       │
        │ skills   : manager + tool registry + gate (native / MCP / text)  │
        │ schedule : scheduled agents and background tasks                 │
        │ channels : telegram (long-poll) / discord (gateway), in-process  │
        │ storage  : SQLite (rusqlite, bundled, FTS5, WAL)                 │
        └───────────────────────────────────────────────────────────────────┘
```

- **One binary, one crate.** `alice serve` is the server; every other subcommand is an HTTP client.
- Telegram and Discord run as tasks inside the server and call the agent core directly (no HTTP, but the same `agent::start_turn`).

## 2. Dependencies (kept small)

| crate | Used for | Why |
|-------|----------|-----|
| tokio | async runtime | streaming, SSE, many connections |
| axum | HTTP/SSE | thin and tokio-native |
| reqwest (rustls, stream) | model and outside APIs | TLS independent of the OS |
| rusqlite (bundled) | SQLite + FTS5 | no differences between OS SQLite versions |
| serde / serde_json / toml | serialization | settings and API |
| clap | CLI arguments | declarative, fewer mistakes |
| getrandom | IDs and tokens | the OS CSPRNG |
| tokio-util | CancellationToken | stopping a reply |
| futures-util | streams | SSE parsing |
| tokio-tungstenite | WebSocket | Discord gateway |
| sha2 | SHA-256 | OAuth PKCE (S256) |
| ratatui, unicode-width | terminal UI | the chat TUI and the CLI's look |

Not used: agent frameworks such as LangChain, an ORM, the tracing ecosystem (a small logger of its own), template engines, frontend build tools.

## 3. Modules

```
src/
├── main.rs          subcommand dispatch
├── util.rs          IDs, time, logger
├── config/          Config (config.toml) / Secrets (secrets.toml, 0600) / Paths
├── storage/         DB connection, migrations, threads/messages CRUD
├── memory/          extraction, search, deletion, rebuild
├── providers/       shared types + openai.rs / anthropic.rs / gemini.rs / cli.rs (codex, antigravity) / codex_server.rs
├── agent/           turns, context building, compaction, tool loop, approvals
├── schedule.rs      scheduled agents and background tasks
├── blurb.rs         Alice's one-line comments for the app screens
├── skills/          skills (manifests, discovery, tool registry, tool gate, auth, http/command handlers)
├── mcp.rs           `alice mcp` (stdio MCP relay, for Codex)
├── tools/           built-in skill code (fs / web / google / recall)
├── server/          router, auth, handlers, SSE, embedded Web UI
├── channels/        telegram.rs / discord.rs
└── cli/             HTTP client, commands, streaming Markdown, TUI, the CLI's look (ui.rs)
web/                 index.html / app.js / google.js / style.css (no build step, embedded in the binary)
migrations/          0001_init.sql … (embedded, tracked with user_version)
```

## 4. Provider adapter

The abstraction stays at **one enum and one function**.

```rust
pub struct ChatRequest { model, system, messages: Vec<ChatMessage>, tools: Vec<ToolSpec>, max_tokens }
pub enum Delta { Text(String), Reasoning(String) }
pub struct Completion { text, tool_calls: Vec<ToolCall>, provider_state: Option<Value>, usage, ttft_ms }
pub async fn stream(provider: &ProviderConfig, secret: Option<&str>, req, on_delta) -> Result<Completion>
```

- `ProviderKind` picks the module's `stream`. The agent core never knows which provider it talks to.
- Alice always builds the whole context and sends it every time (no provider-side sessions or memory).
- Provider state that must be sent back (Anthropic thinking blocks, Gemini thoughtSignature) is kept on the message as `provider_state` and resent only to the same provider. Switching models drops it, so the conversation carries on.

### Capabilities

| Kind | Streaming | Tool calling | Image | File | Reasoning | Auth |
|------|-----------|--------------|-------|------|-----------|------|
| openai | ✓ | ✓ | ✓ | as text | ✓ (model-dependent) | API key |
| openai-compatible | ✓ | server-dependent | server-dependent | as text | – | API key (optional) |
| ollama | ✓ | model-dependent | model-dependent | as text | – | none |
| anthropic | ✓ | ✓ | ✓ | as text / PDF | ✓ (adaptive) | API key |
| gemini | ✓ | ✓ | ✓ | as text | ✓ | API key |
| codex-cli | ✓ (app-server) | native (app-server dynamic tools) or MCP (`alice mcp`); text protocol if the safety check fails | ✗ | ✗ | inside Codex | Alice's own ChatGPT login (official CLI, `~/.alice/codex`) |
| antigravity-cli | per message | text protocol (```alice-tool```) | ✗ | ✗ | inside agy | Google sign-in (once with `agy`; runs in plan mode, sandboxed, in an empty directory) |
| gemini-cli | – | – | – | – | – | retired (Google ended the subscription login on 2026-06-18; old configs load but cannot be used) |

The context window comes from the model settings (`context_tokens`) and is used to trim history.

## 5. Agent loop

```
start_turn(thread, user_text, client)
  ├─ tx: INSERT user message, INSERT assistant message (status=streaming)
  ├─ SSE message.created ×2
  └─ spawn:
       compaction: fold the oldest overflow into the thread's summary (when needed)
       loop (up to 16 steps)
         ctx = SOUL + summary + <memories> (FTS top N) + history (within the character budget)
         completion = providers::stream(...)   ← SSE message.delta per delta
         no tool calls → break
         each tool call: permission (allow / ask → wait for approval / disabled → refuse)
                         run → INSERT tool message
       UPDATE assistant message (content, status=complete, meta)
       SSE message.completed, then turn.done
       memory::extract(user message)  ← background; a failure never affects the chat
```

- Cancelling: a `CancellationToken` per thread. `select!` interrupts the stream (and compaction), and the partial reply is saved as `status=cancelled`.
- Delete races: every write depends on the `thread_id` / `message_id` foreign keys. After a delete an UPDATE touches 0 rows and an INSERT fails, so the generation ends quietly.
- A panic in a turn or a tool closes the turn as failed, so a thread never stays "generating".
- Timing: `meta` keeps `alice_ms` (context and DB), `ttft_ms` and `provider_ms` apart.

## 6. Storage

- WAL, `foreign_keys=ON`, `secure_delete=ON`, `busy_timeout=5000`.
- One `Connection` behind a `std::sync::Mutex`, never held across an await.
- Migrations: `migrations/*.sql` via `include_str!`, applied by `PRAGMA user_version`.

## 7. Auth

- `server.token` in `secrets.toml` (32 random bytes, hex) is made by `alice setup` or on first start, or set by the user (`alice token --set`). A changed file is picked up without a restart.
- The CLI reads the token from `secrets.toml` on the same machine. A remote CLI uses `ALICE_URL` / `ALICE_TOKEN`.
- Web: posting the token to `/api/login` sets the `alice_session` cookie (HttpOnly, SameSite=Strict, Secure over HTTPS). Cookie requests that change state need the `X-Alice-CSRF` header (cross-origin requests fail the preflight).
- iPhone: the token is kept in the Keychain and sent as a Bearer token.
- 30 wrong tokens in a minute from one address lock that address for a minute.

## 8. Profiles

- A profile = its own data directory + its own server process. `default` is `~/.alice`, others are `~/.alice/profiles/<name>/`.
- Path resolution: `$ALICE_HOME` (when started as a service) → `-p/--profile` → `$ALICE_PROFILE` → `~/.alice/current_profile` → `default`.
- A free port is picked on creation. The launchd label, systemd unit and Web cookie name are per profile too.
- No shared code paths with state, no shared state: models, memories, gateways, Google, tool permissions are all separate.

## 9. Channels (Telegram / Discord)

- Run as tasks inside the server and call `agent::start_turn` directly.
- Shared layer (`channels/mod.rs`): commands, thread selection (`channel_state`), streaming edits (every 1.2 s), approval buttons, attachments, recording posted messages (`channel_messages`) and best-effort deletion.
- Each platform implements only the `Surface` trait (send / edit / delete / typing). Telegram uses long polling, Discord the gateway WebSocket + REST.
- A settings change (`config.updated`) restarts the gateways.

## 10. Skills (Google Workspace and others)

Details in [SKILLS.md](SKILLS.md).

- Files, Web, Google Workspace, Recall and Background are built in. Outside skills live in `~/.alice/skills/<id>/` (`skill.toml` + `SKILL.md`).
- Every tool call goes through `skills::gate::invoke` (permission → approval → run → strip secrets → record in `tool_runs`).
- Three routes: native tool calling (API models), MCP (Codex: `alice mcp` + a per-turn token), and the text protocol (other CLIs).
- OAuth is generic (PKCE + single-use `state` + a loopback `/oauth/callback`). When the browser is on another machine, paste the redirect URL into the CLI to finish.

## 11. Scheduled agents and background tasks

- `agents` rows hold an instruction, a schedule (every N minutes, or daily times in the server's local time) and a target thread. A 20-second tick starts the ones that are due.
- The instruction is posted as a hidden user message (`client = agent`), so the reply reads as Alice speaking up. A reply of `NO_UPDATE` means nothing to report: the run's messages (and a chat made for it) are removed while the thread is still reserved.
- The last ten runs per agent are kept in `agent_runs`; runs cut off by a restart become `interrupted`.
- `background_task` (a tool) starts a one-off run in a new chat (`client = background`), at most three at once.

## 12. iPhone app

- `ios/AliceKit` (Swift Package): API client, SSE, `ChatStore` / `GoogleStore` (@Observable), Markdown blocks, Keychain, offline copy, widget snapshot. Tested on macOS too.
- `ios/Alice` (SwiftUI): the screens. The last loaded data is kept encrypted on the device for offline reading (complete file protection, no backup, removed on disconnect). Deletions arrive over SSE.
- Calendar, Mail and To-do call `/api/google/*` (`src/server/google.rs`). Fetching shares code with the Google Workspace skill (`src/tools/google.rs`), and tokens never leave the server. These are the user's own actions, so no approval is asked, but the skill's enabled state, connection and groups (calendar / gmail / tasks) apply.
- Widgets (`ios/AliceWidgets`) read a snapshot the app writes to the App Group; they make no network calls and hold no token.

## 13. Deployment

- macOS: `~/Library/LaunchAgents/local.alice.agent.plist` (launchd, KeepAlive).
- Linux: `~/.config/systemd/user/alice.service` (systemd user unit).
- Without a registered service, `alice start` starts `alice serve` detached and tracks it with a PID file (e.g. in containers).
- For remote access, Tailscale (`tailscale serve`) or an HTTPS reverse proxy is recommended.
