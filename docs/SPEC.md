# Alice Agent (A2) — Specification

Alice Agent is a single-user, local-first personal AI agent.
This document sets the specification the implementation follows, after a review of the original requirements.

- Product name: Alice Agent / short name: A2 / CLI: `alice`
- Priorities: Simplicity > Complexity, Lightweight > Feature-rich, Performance > Unnecessary Abstraction, Privacy > Convenience

---

## 1. Requirements review (changes and clarifications)

| # | Area | Finding | Decision |
|---|------|---------|----------|
| R1 | Web frontend | Using Node.js at build time adds a build dependency | **Plain HTML/CSS/JS with no build step**, embedded in the binary with `include_str!`. No Node.js, not even to build. |
| R2 | CLI command names | `alice caht <id>` / `alice chat claer` read as typos | Implemented as `alice chat <chat-id>` / `alice chat clear`. |
| R3 | ChatGPT subscription | The only officially allowed route is the Codex CLI. Codex is an agent with its own tools | `codex-cli` provider, **optional**. Runs with Alice's own login, ephemeral, read-only, in an empty directory, with Codex's shell/exec/browser/plugins off. Alice's tools are offered natively through a warm `codex app-server`, or through `alice mcp` after a safety check; otherwise the text protocol. Limits are listed in the capability table. |
| R4 | Google AI subscription | Gemini CLI's personal Google login (AI Pro/Ultra) ended on 2026-06-18; the official successor is the Antigravity CLI (`agy`). `agy` has its own tools (file edits, commands), and its docs offer no option to turn them all off | `antigravity-cli` provider runs `agy -p` in plan mode with `--sandbox` in an empty directory (headless runs refuse actions that need approval). Alice's tools go through the text protocol. `gemini-cli` is retired (old configs load, and using it explains what to switch to). No cookie extraction or unofficial APIs, ever. |
| R5 | "Derived user profile" | A separate profile table is a weak point for consistent deletion | **No separate profile.** Everything about the user is a sourced Memory; a profile view is a view over memories. |
| R6 | Conversation summaries | Cross-thread summaries would carry deleted information along | Summaries live inside their thread only and are deleted with it. No cross-thread summaries. |
| R7 | Relearning after deletion | Extracting memories from AI replies could recreate a deleted memory in another thread | **Memories are extracted only from the user's own messages (role=user).** Extraction never sees existing memory contents (key names only). |
| R8 | What deletion guarantees | Data can remain in SQLite free pages, the WAL and FTS segments | `secure_delete=ON`, `wal_checkpoint(TRUNCATE)` after deletes, FTS `optimize`, `VACUUM` on delete-all. OS snapshots (Time Machine, APFS/Btrfs) and outside services are stated as out of scope. |
| R9 | Telegram/Discord history | Messages on those platforms are outside Alice's control | When a thread is deleted, deleting the bot's messages is best effort (Telegram allows it only within 48 hours). Stated as out of scope. |
| R10 | Google OAuth | An OAuth client must be created in Google Cloud. In "testing" status, refresh tokens expire after 7 days. `gmail.send` and others are sensitive scopes | Uses the user's own OAuth client (Desktop app). Asks to sign in again when it expires. Scopes are minimal per feature; only enabled features are requested. |
| R11 | Web search | There is no official, free, general search API; scraping HTML can break terms of service | Search providers: Brave Search API / SearXNG (self-hosted). Without one, the search tool is off. |
| R12 | Performance | 10 ms for the CLI is reachable for `--version` and local work, but commands that talk to the server depend on the server | CLI startup time and API round trips are measured separately. |
| R13 | Changing the port in Settings | A running socket cannot change | After saving, says a restart is needed. |
| R14 | `alice memory clear` | Clearing memories leaves the threads | Deletes memories (and sources, uses, FTS) only; conversations stay. Old messages are not re-extracted (extraction happens only on new turns). |
| R15 | Installing the iPhone app | Installing on a device needs an Apple developer signature | An Xcode project is provided; the user sets the signing team. |
| R16 | Codex CLI sandbox | Codex is an agent and may read local files even when read-only | Noted as a limit in SECURITY.md. For untrusted content, an API connection is recommended. |
| R17 | File tools and settings | A broad root would let the model read secrets.toml | Alice's data directory is always refused whatever the roots (except the workspace). |
| R18 | Profiles (added requirement) | "Independent models / settings / memory / gateways" | Each profile has its own data directory and server process. No shared state. |

---

## 2. Terms

- **Thread**: a conversation. One ID for every client (12 lowercase letters and digits, e.g. `t4k2m9xq7ab3`).
- **Message**: one entry in a thread. Role is `user` / `assistant` / `tool`.
- **Memory**: a sourced fact about the user, shared across threads.
- **Memory source**: where a memory came from. Always a user message.
- **Generation**: producing the reply to one user message (tool loop included).
- **Client / channel**: `cli` / `web` / `ios` / `telegram` / `discord` (plus `agent` and `background` for Alice's own runs).

## 3. Data and settings kept apart

| Kind | Stored in | On delete-all-threads |
|------|-----------|-----------------------|
| Server, model and tool settings | `~/.alice/config.toml` | Kept |
| Personality (system prompt) | `~/.alice/SOUL.md` | Kept |
| API keys, OAuth tokens, bot tokens, API token | `~/.alice/secrets.toml` (0600) | Kept |
| Conversations, memories, attachments, channel state | `~/.alice/alice.db`, `~/.alice/attachments/` | Deleted completely |
| Tool working folder | `~/.alice/workspace/` | Kept (the user's files) |
| Logs | `~/.alice/logs/` | Never contain conversation content |

Data that comes from conversations exists **only in the DB and attachments**. Settings files never receive anything from a conversation.

## 4. Memory

### 4.1 Structure
- `memories(id, key, content, created_at, updated_at)`: `key` names the topic (e.g. `home_city`).
- `memory_sources(memory_id, message_id, thread_id, content, created_at)`: what each source says.
- `memory_uses(message_id, memory_id)`: which reply used which memory in its context (for audits and showing sources).
- `memories_fts`: FTS5 (trigram tokenizer, works for Japanese).

### 4.2 Invariants
1. Every memory has at least one source. A memory left with no source is deleted in the same transaction.
2. A source always points to an existing **user-role message** (foreign key).
3. `memories.content` is derived: it equals the newest `content` among the remaining sources (rebuilt without a model, deterministically).
4. Extraction sees only the user message itself and the previous user messages of the same thread (for context). Never AI replies, tool results or existing memory contents.

### 4.3 Extraction
- After each turn, one model call in the background (`memory.auto_extract`; short messages are skipped).
- The model returns `{"memories":[{"key","content"}]}`. The same key adds a source to the existing memory (updating it).
- If the message (and so the thread) was deleted by the time the result is written, the foreign key fails, the write rolls back, and no memory is made.

### 4.4 Search and injection
- An FTS5 query is built from the user's message, and only the top N by bm25 (default 8) go into the system prompt as a `<memories>` block.
- The block says plainly that it is data, not instructions.
- The `recall` skill lets the model search memories and earlier chats itself, with its own keywords.

### 4.5 Deletion
- **Delete a thread**: cancel a running generation → in one transaction delete the thread (messages, memory_sources, memory_uses, attachment rows and channel_state cascade) → delete memories left with no source → rebuild the content of the other affected memories → FTS updates by trigger → commit → remove the attachment directory → FTS `optimize` → `wal_checkpoint(TRUNCATE)` → SSE `thread.deleted`.
- **Delete all threads**: the same for every thread, then `VACUUM`. Memories end up empty.
- **Clear memories**: empty memories / memory_sources / memory_uses / FTS; conversations stay.

### 4.6 What is guaranteed
- Guaranteed: `alice.db` (WAL included), the FTS index, `attachments/`, the server's in-memory caches, and the Web / iPhone caches (dropped on the SSE delete event; the iPhone's offline copy is updated the same way and removed on disconnect).
- Not guaranteed: data already sent to an outside model provider, messages on Telegram / Discord, backups and snapshots made by the OS or the user, terminal scrollback.

## 5. Server API (REST + SSE)

Auth: `Authorization: Bearer <token>`, or the Web UI's HttpOnly cookie (requests that change state need the `X-Alice-CSRF: 1` header). Only `/api/health` needs no auth.

| Method | Path | What it does |
|--------|------|--------------|
| GET | `/api/health` | Alive check and version |
| POST | `/api/login` | Checks the token and sets the cookie |
| POST | `/api/logout` | Drops the cookie |
| GET | `/api/status` | Server state (CPU, memory, DB size, counts) |
| GET | `/api/events` | SSE (`?thread=<id>` to narrow) |
| GET/POST | `/api/threads` | List / create |
| DELETE | `/api/threads` | Delete all threads (needs `{"confirm":"delete-all"}`) |
| GET/PATCH/DELETE | `/api/threads/{id}` | Get / rename, change model, star / delete |
| GET | `/api/threads/{id}/messages` | Messages |
| POST | `/api/threads/{id}/messages` | Post and start a reply (202) |
| POST | `/api/threads/{id}/stop` | Stop the reply |
| POST | `/api/approvals/{id}` | Approve or deny a tool run |
| GET | `/api/models` | Models, default model, capabilities |
| PUT | `/api/models/default` | Change the default model |
| POST | `/api/models/test` | Connection test |
| GET | `/api/memories` | List (`?q=` to search) |
| GET | `/api/memories/status` | Counts and storage use |
| GET | `/api/memories/{id}` | Details with sources |
| DELETE | `/api/memories` | Clear memories |
| GET/PATCH | `/api/config` | Settings (secrets are never returned) |
| PUT | `/api/secrets/{name}` | Set a secret (there is no read API) |
| GET/PATCH | `/api/tools` | Tools and permissions |
| GET/POST, PATCH/DELETE | `/api/agents`, `/api/agents/{id}` | Scheduled agents |
| POST / GET | `/api/agents/{id}/run`, `/api/agents/{id}/runs` | Run now / last ten runs |
| POST | `/api/blurb` | Alice's one-line comment for an app screen |
| — | `/api/google/*` | Calendar, mail and tasks for the apps |

### SSE events
`message.created`, `message.delta`, `message.completed`, `thread.created`, `thread.updated`, `thread.deleted`, `threads.cleared`, `tool.started`, `tool.completed`, `tool.approval_required`, `tool.approval_resolved`, `generation.cancelled`, `turn.done`, `agents.updated`, `memory.updated`, `error`.
Each event is JSON like `{"type":..., "thread_id":..., ...}`. Every turn ends with exactly one `turn.done`, however it ended.

## 6. Concurrency
- One generation per thread at a time. Posting while one runs returns 409.
- Deleting a thread cancels its generation first. Every write of a generation checks the thread through a foreign key, so rows never come back after a delete.
- SQLite runs on one connection behind a mutex (fast enough for one person, and simple to reason about).

## 7. Tool permissions
- Modes: `disabled` / `ask` / `allow`. Destructive actions (deleting files, sending mail, deleting events, …) default to `ask`.
- Permissions live only in `config.toml`; there is no API or tool for the model to change them.
- Approval requests go out over SSE and show on the client that started the turn (any client can answer). After a timeout (10 minutes by default) they count as denied.

## 8. Security
- Binds to `127.0.0.1` by default. Every API except health needs the token. A non-loopback address without HTTPS logs a warning at startup.
- Tokens are compared in constant time. The Web UI uses CSP `default-src 'self'`; Markdown escapes HTML first and then adds a small set of safe tags.
- Outside content (web, mail, files) is wrapped in `<untrusted>`, and the system prompt says not to follow it. Destructive tools need approval, which limits what an injection could do.
- Logs never contain tokens, API keys or message text.

## 9. CLI
Every command from the requirements. The CLI is a client of the server API with no agent runtime of its own (`alice serve` is the only server).
