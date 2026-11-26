# Alice Agent (A2) — Roadmap

State: ☐ not started / ◐ in progress / ☑ done (automated tests or checked for real)

## Measurements (macOS, Apple Silicon, release build)
- CLI startup: `alice --version` median 3.5 ms, `alice chat list` (with an API round trip) 3.8 ms
- Server idle memory (RSS): about 11–13 MB
- Release binary: about 11 MB (Web UI embedded)

## Profiles
- ☑ `alice profile <name>` for independent instances (settings, secrets, DB, gateways, port, service)

## Phase 1 — Core
- ☑ Rust project, settings, secrets, paths
- ☑ SQLite schema and migrations
- ☑ Server API (threads / messages / events / status) and token auth
- ◐ Providers: OpenAI / OpenAI-compatible / Ollama / Anthropic / Gemini (streaming, tool calls) — verified against tests that replay each API's SSE format; not yet tried with real API keys
- ☑ Agent loop, cancelling, delete races
- ☑ CLI: setup / start / stop / restart / status / log / chat / model / config / memory / tool / token
- ☑ Tests: integration tests against a local OpenAI-compatible test server

## Phase 2 — Memory
- ☑ Extraction (user messages only), merged by key
- ☑ FTS5 trigram search and context injection
- ☑ Source tracking, rebuild on thread delete, delete-all
- ☑ CLI: memory list / search / show / status / clear
- ☑ Tests: cross-thread use, deletion, no resurrection, races, empty after delete-all
- ☑ Recall skill (memory and conversation search) and rolling compaction of long threads

## Phase 3 — Web
- ☑ Embedded Web chat (streaming, Markdown, stop, model choice)
- ☑ Settings (General / Models / Channels / Tools / Memory / System)
- ☑ Cookie auth, CSRF, CSP
- ☑ Live sync over SSE
- ☑ Calendar, Mail, Tasks and Agents views

## Phase 4 — Tools
- ☑ Permission gate (disabled / ask / allow) and approval API
- ☑ File system (root limits, symlink protection)
- ☑ Web search (Brave / SearXNG), URL fetch, text extraction (with SSRF protection; Brave/SearXNG not yet tried with real keys)
- ☑ Google (OAuth 2.0 PKCE; Gmail / Calendar / Tasks / Drive / Docs / Sheets) — used with a real account

## Phase 5 — Channels and subscriptions
- ☑ Telegram (long polling, allow list, commands, approval buttons) — verified against a fake Bot API; not yet tried with a real bot
- ☑ Discord (gateway, allow list, commands, approvals) — verified against a fake gateway/REST; not yet tried with a real bot
- ☑ codex-cli (ChatGPT subscription) — used with a real login on macOS and Linux
- ◐ antigravity-cli (Google AI subscription) — follows the documented stream-json format; not yet tried with a real sign-in. gemini-cli is retired

## Phase 6 — iPhone
- ☑ SwiftUI app (connection, Keychain, threads, streaming, approvals, attachments, stop, model choice) — used on a real iPhone
- ☑ Home, Calendar, Mail, To-do, widgets, notifications, offline reading, agents, Alice's comments

## Phase 7 — Scheduled work
- ☑ Scheduled agents (repeat or daily times, quiet runs, run history)
- ☑ Background tasks

## Phase 8 — Release
- ☑ Install script (`scripts/install.sh`: build from source / binary / uninstall)
- ☑ launchd registration, start, stop, restart on crash and removal checked on macOS
- ◐ systemd user unit — implemented; not yet checked on a Linux machine with systemd (containers use the detached process instead)
- ☑ Linux x86_64 / ARM64 builds and tests (CI), and running on a Linux server
- ☑ Benchmark (docs/BENCHMARK.md)
- ☑ Security review (docs/SECURITY.md) and fixes
- ☑ Documentation (README / SPEC / ARCHITECTURE / SKILLS)

## Still to try for real
- OpenAI / Anthropic / Gemini with real API keys (`alice model test`)
- Antigravity CLI with a real Google sign-in
- Telegram / Discord with real bots
- systemd on a Linux machine
