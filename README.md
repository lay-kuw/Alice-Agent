<p align="center">
  <img src="docs/images/alice.png" width="200" alt="Alice">
</p>

<h1 align="center">✦ Alice Agent ✦</h1>

<p align="center">
  A tiny, speedy AI agent just for you ₍ᐢ. .ᐢ₎♡<br>
  One binary · local first · nothing but SQLite
</p>

<p align="center">
  <a href="#-install"><img src="https://img.shields.io/badge/macOS-Apple%20Silicon-f5c2e7?logo=apple&logoColor=white" alt="macOS"></a>
  <a href="#-install"><img src="https://img.shields.io/badge/Linux-x86__64%20%7C%20arm64-cba6f7?logo=linux&logoColor=white" alt="Linux"></a>
  <a href="https://www.rust-lang.org"><img src="https://img.shields.io/badge/Rust-2024-f38ba8?logo=rust&logoColor=white" alt="Rust"></a>
  <a href="#-iphone-app"><img src="https://img.shields.io/badge/iPhone-SwiftUI-fab387?logo=swift&logoColor=white" alt="iPhone app"></a>
  <a href="#-your-data"><img src="https://img.shields.io/badge/storage-SQLite%20only-b4befe" alt="SQLite only"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-a6e3a1" alt="MIT License"></a>
</p>

<p align="center">｡･:*:･ﾟ★ chat with the same Alice from the CLI, Web, iPhone, Telegram and Discord ★ﾟ･:*:･｡</p>

---

## 🌸 Contents

- [What she can do](#-what-she-can-do)
- [Install](#-install)
- [Getting started](#-getting-started)
- [Commands](#-commands)
- [Models and subscriptions](#-models-and-subscriptions)
- [Connections](#-connections)
- [iPhone app](#-iphone-app)
- [Personality (SOUL.md)](#-personality-soulmd)
- [Profiles](#-profiles)
- [Your data](#-your-data)
- [Remote access](#-remote-access)
- [Development](#-development)

## ✨ What she can do

- **🪄 One binary**: `alice` (Rust) holds the server, CLI, TUI and Web UI. No Docker, Node.js, Redis or outside database
- **💬 The same conversation everywhere**: CLI / Web / iPhone / Telegram / Discord share threads and memories
- **🧠 Memories with sources**: everything Alice remembers points back to where you said it. Delete the thread, and what she learned from it goes too
- **🔎 Recall**: ask "that café we talked about?" and Alice picks keywords and searches her memories and earlier chats (all on your machine)
- **📅 Google**: read and add calendar events, Gmail and tasks. Only the server holds the tokens
- **⏰ Agents**: "summarize my mail every morning at 8" and Alice speaks up on her own. With nothing to report, she quietly leaves no trace
- **🌙 Background tasks**: "take your time on this" starts the work in its own chat and lets you know when it's done
- **📱 A cute iPhone app**: home, chat, calendar, mail, to-do, widgets, notifications, offline reading
- **🛡️ Safety first**: tools ask for approval, HTML mail shows without scripts, logs never hold conversations or secrets
- **⚡ Light**: about a dozen MB of memory while idle. Replies stream, and long chats keep a summary of their beginning

## 🍓 Install

```sh
git clone https://github.com/lay-kuw/Alice-Agent.git
cd Alice-Agent
scripts/install.sh            # build and put it in ~/.local/bin/alice
scripts/install.sh --uninstall
```

Runs on macOS (Apple Silicon) and Linux (x86_64 / ARM64). Needs [Rust](https://rustup.rs).

## 🎀 Getting started

```sh
alice setup      # guided setup (server, API token, model, start at login)
alice start      # start the server (through launchd / systemd when registered)
alice chat       # chat in the TUI (/ for commands, Esc stops a reply, --plain for line mode)
```

For the Web UI, open `http://127.0.0.1:7878/` and sign in with the token from `alice token`. Yay ٩(ˊᗜˋ*)و

The icons on the left (tabs at the bottom on a phone) switch between Chat, Calendar, Mail, Tasks, Agents and Settings. `#calendar`, `#mail`, `#tasks` and `#agents` open a view directly.

## 🐾 Commands

| Command | What it does |
|---|---|
| `alice chat "question"` | Ask once in a new thread |
| `alice chat list` / `alice chat <id>` | List threads / pick one up again |
| `alice chat rename <id>` / `delete <id>` / `clear` | Rename / delete / delete all (settings stay) |
| `alice model [list\|refresh\|add\|remove\|use\|login\|logout\|test]` | Models (numbers work too, like `use 1`; `use 2 high` sets the reasoning effort) |
| `alice soul [edit\|show\|reset]` | The personality file, SOUL.md |
| `alice usage` | ChatGPT subscription limits (5-hour and weekly) and tokens used per model |
| `alice memory [list\|search "word"\|show <id>\|status\|clear]` | Memories (`show` for sources) |
| `alice tool [list\|enable <tool> [ask\|allow]\|disable <tool>]` | Tool permissions |
| `alice skill [list\|info\|enable\|disable\|setup\|login\|logout\|log]` | Skills (add-ons), sign-in and run log |
| `alice google [setup\|login\|logout\|status]` | Shortcut for the google-workspace skill |
| `alice gateway [list\|setup [telegram\|discord]\|delete <id>]` | Telegram / Discord |
| `alice profile [list\|<name>\|use <name>\|delete <name>]` | Profiles |
| `alice config [get\|set <key> <value>\|secret <name>]` | Settings and secrets |
| `alice service [install\|uninstall\|status]` | Start at login (launchd / systemd) |
| `alice status` / `log -f` / `start` / `stop` / `restart` / `token` | Server |

Every command takes `-p <profile>` to act on another profile just once.

## 🧁 Models and subscriptions

Use API keys, or your subscriptions through the official CLIs. Connect several and switch per chat from the model picker in the Web UI, TUI and iPhone app.

<details>
<summary><b>💬 ChatGPT subscription (codex-cli)</b></summary>

- `alice model add` → `codex-cli` → `alice model login codex-cli`
- Alice's own login; your usual `~/.codex` is not used. On a server without a browser: `alice model login codex-cli --device-auth`
- Needs the [Codex CLI](https://github.com/openai/codex) on the server (the CLI is the only official way to use a ChatGPT subscription)
- Skills are reached through Alice's built-in MCP server, and approvals happen in Alice's UI

</details>

<details>
<summary><b>✨ Google AI subscription (antigravity-cli)</b></summary>

- Install the Antigravity CLI on the server: `curl -fsSL https://antigravity.google/cli/install.sh | bash`
- `alice model add` → `antigravity-cli` → `alice model login antigravity-cli` (it starts `agy`) to sign in once
- On a server without a browser, open the URL it prints on another device and paste the code back
- `agy` runs in plan mode, sandboxed, in an empty directory; skills work through Alice's text protocol
- Gemini CLI's subscription login ended on 2026-06-18, so `gemini-cli` can no longer be added

</details>

<details>
<summary><b>🔑 API keys / local models</b></summary>

- OpenAI / Anthropic / Gemini / OpenAI-compatible servers / Ollama
- Model lists refresh at start and every 6 hours (`alice model refresh` for now)
- Set the reasoning effort after the model ID, like `codex-cli/gpt-6.2-sol#high`

</details>

## 🧸 Connections

<details>
<summary><b>📅 Google (Calendar, Gmail, Tasks)</b></summary>

- Create a "Desktop app" OAuth client in Google Cloud → `alice skill setup google-workspace` → `alice skill login google-workspace`
- Only the scopes of the services you turn on are requested. For Tasks, turn it on with `alice skill groups google-workspace` and sign in again
- Marking mail as read needs Gmail's modify scope (`gmail.modify`). If you connected before it was added, sign in once more

</details>

<details>
<summary><b>🧩 Skills (add-ons)</b></summary>

- Built in: Google Workspace, Files, Web, Recall, Background tasks
- Drop a folder (`skill.toml` + `SKILL.md`) into `~/.alice/skills/<id>/` to add abilities without changing Alice (example: `skills/examples/github`)
- Working folder: `alice config set tools.fs.workspace ~/AliceWork` (more folders with `tools.fs.roots`)
- Web search: `alice config set tools.web.search_provider brave` + `alice config secret brave.api_key` (or SearXNG)
- More in [SKILLS](docs/SKILLS.md)

</details>

<details>
<summary><b>💌 Telegram / Discord</b></summary>

- `alice gateway setup`. Anyone not on the allow list is ignored
- Telegram has `/new /threads /model /delete /stop /status`; on Discord, the same commands work in DMs or by mention

</details>

<details>
<summary><b>⏰ Agents (scheduled)</b></summary>

- Choose "repeat" (every N minutes, hours, days or weeks; at least 5 minutes; the first run right after saving) or "set times" (every day at 12:00 and 18:00, say, in the server's time)
- Results arrive in the chat you pick, as if Alice reached out on her own (the instruction itself is not shown). Without a chat, the first run makes one, and deleting that chat sends the agent back to "a new chat"
- Write "if there is no new mail, do nothing" and a run with nothing to report leaves no trace
- The Web UI's Agents view shows the last ten runs (time, result, chat, why one failed)
- Runs missed while the server was down are not replayed; the next one comes at its time. API: `/api/agents`

</details>

<details>
<summary><b>🌙 Background tasks, long chats, notifications</b></summary>

- **Background tasks**: "do this in the background" starts the work in a new chat and notifies you when it is done (three at a time)
- **Long chats**: past what can be sent (`agent.history_chars`, 60,000 characters by default), the oldest part is folded into a short summary Alice keeps. The history itself stays
- **Chat notifications**: a reply in a chat you are not looking at notifies you. In the Web UI, turn on "Notifications" in Agents (HTTPS or localhost only)

</details>

## 📱 iPhone app

Open `ios/Alice.xcodeproj` in Xcode, set the signing team on both targets (Alice / AliceWidgets) and install it on your iPhone. Connect with the server URL and the token from `alice token` (from outside, HTTPS through `tailscale serve` or similar is best).

- **🏠 Home**: Alice's greeting, today's events and to-dos, and a box to ask her something (can be turned off)
- **💬 Chat**: ≡ at the top left for your chats, ✨ at the top right for settings, agents and the model. Swipe right to go back to the start. Long-press to star (starred chats can't be deleted)
- **📅 Calendar**: week and month views, 11 color tags, a "Today" button beside the date
- **✉️ Mail**: All / Unread / Important, swipe right to mark read, reply. HTML mail only when you tap "Show the original layout" (no scripts; remote images only when you ask)
- **✅ To-do**: All / Today / Week, a progress bar, long-press to edit, and a little burst of hearts when you finish one
- **🗨️ Alice's comments**: Alice says a line at the top of each screen, in a tone you choose, or not at all. By default only counts and times go to the model; titles and senders only if you allow them
- **🌗 Language**: Japanese in light mode, English in dark mode

<details>
<summary><b>🔔 Notifications · 🧩 Widgets · 📴 Offline</b></summary>

- **Notifications**: events (10 minutes before; all-day events at 9:00), to-do due dates (9:00), new mail and chat replies. All are local notifications made on the iPhone, with no push server. While the app is closed, mail and chat notifications come with iOS background refresh, so they can be late. Turn each kind on or off in ✨ → Settings → Notifications
- **Widgets**: Today / Month / Tasks / Today + Tasks (lock screen ones too). Widgets never touch the server or the token; they show a snapshot the app writes to the App Group (`group.local.alice.app`). If you change the bundle ID, change the App Group in `Alice.entitlements`, `AliceWidgets.entitlements` and `WidgetSnapshot.appGroup` alike
- **Offline**: without a connection, the last loaded events, mail, to-dos and recent chats still show (read only). They are kept encrypted on the iPhone (unreadable while locked, not backed up) and removed when you switch servers

</details>

## 💖 Personality (SOUL.md)

`~/.alice/SOUL.md` (one per profile) is everything Alice is told: her role, language, character, way of talking, way of working and safety promises all live in this file, and rewriting it changes all of it. Alice adds only the current time, the working folder, a one-line list of usable skills and up to 8 related memories. `{{name}}` becomes the agent's name.

Edit it with `alice soul` or in the Web UI's "Settings → Personality"; it applies from the next reply. It survives deleting every conversation, and the AI cannot rewrite it.

## 🎈 Profiles

```sh
alice profile eve        # create "eve" and switch to it (runs on its own port)
alice model add          # models just for eve
alice start              # start eve's server
alice -p default chat    # use default just this once
alice profile default    # back to default
```

`default` lives in `~/.alice`, and others in `~/.alice/profiles/<name>/`, completely apart.

## 🍬 Your data

```
~/.alice/
├── config.toml     settings (nothing from conversations)
├── SOUL.md         personality
├── secrets.toml    API keys and tokens (0600)
├── alice.db        conversations, memories, agents (SQLite, FTS5)
├── attachments/    attachments (per thread)
├── workspace/      the file tools' default working folder
├── skills/         added skills (<id>/skill.toml + SKILL.md)
├── codex/          Alice's own Codex settings and login
├── logs/           logs (no conversation text or secrets)
└── profiles/<name>/ other profiles (same layout)
```

To move to another machine, copy `~/.alice` as a whole (sign in to the subscription CLIs again on the new one).

## 🌐 Remote access

Only `127.0.0.1` by default. To connect from devices on the same Wi-Fi or Tailscale, add this machine's address (`127.0.0.1` is always on; separate several with commas; `0.0.0.0` inside a container):

```sh
alice config set server.bind 192.168.1.20          # LAN address
alice config set server.bind 192.168.1.20,100.x.y.z # LAN and Tailscale
alice restart && alice token                       # shows the URLs to connect to
```

- If an address isn't there right now (another Wi-Fi, say), Alice warns, skips it and starts anyway
- Traffic is plain HTTP, so use trusted networks only. From outside, go through Tailscale (e.g. `tailscale serve 7878`) or an HTTPS reverse proxy and set `alice config set server.public_url https://...`
- Choose your own API token with `alice token --set` (12+ characters), or make a random one with `alice token --rotate`. Both apply at once, without a restart
- An address that sends 30 wrong tokens in a minute gets a one-minute nap

## 🛠️ Development

```sh
cargo test                                  # Rust (models and services replaced by local fake servers)
(cd ios/AliceKit && swift test)             # iOS client layer
scripts/bench.py target/release/alice 30    # benchmark
```

Design notes: [SPEC](docs/SPEC.md) · [ARCHITECTURE](docs/ARCHITECTURE.md) · [SKILLS](docs/SKILLS.md) · [SECURITY](docs/SECURITY.md) · [BENCHMARK](docs/BENCHMARK.md) · [ROADMAP](docs/ROADMAP.md)

## 📜 License

[MIT](LICENSE) ♡
