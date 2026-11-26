# Skills

Everything Alice can do beyond talking is added as a "skill". Files, web search, Google Workspace, recall and background tasks are built-in skills and work the same way as outside ones. Dropping a folder into `~/.alice/skills/` adds abilities without changing Alice itself.

## Overview

```
 ~/.alice/skills/<id>/          built-in skills (shipped in the binary)
   SKILL.md   skill.toml        files / web / recall / background / google-workspace
        │                              │
        └──────────┬───────────────────┘
                   ▼
            Skill Manager   discovery, validation, dependency checks, on/off, settings
                   ▼
            Tool Registry   one place for the tools of enabled skills (name, schema, default permission, risk)
                   ▼
            Tool Gate       permission → approval → run → log (the single entry point, whatever the model or route)
         ┌─────────┼─────────────────┐
         ▼         ▼                 ▼
  native        MCP (stdio)        text protocol
  tool calling  alice mcp ⇄ Codex   ```alice-tool``` blocks
  OpenAI /      CLI and others      Antigravity CLI, models without tools,
  Anthropic /                       Codex when its safety check fails
  Gemini / Ollama
```

- Whichever route a call takes, permission, approval, running and logging happen **only in the Tool Gate (`skills::gate`)**.
- Approvals can be given only in Alice's own UI (Web / TUI / iPhone / Telegram / Discord). There is no API for a model, MCP or a skill to approve anything.

## What a skill is made of

```
~/.alice/skills/github/
├── SKILL.md      how to use it, for the model (read only when needed)
├── skill.toml    manifest: tools, dependencies, permissions, auth, settings
└── run.py        (only for command tools)
```

### SKILL.md

Usage notes for the model. They are not put in every prompt: the system prompt holds only an index ("skill name + one line"), and the model reads the full text with the `skill_guide` tool when it needs it (saves tokens).

### skill.toml (manifest)

```toml
id = "github"                      # same as the folder name. a-z 0-9 -
name = "GitHub"
version = "0.1.0"
description = "Issues and pull requests on GitHub"   # the one line in the index

[requires]
bins = []                          # executables needed on PATH (for command tools)
settings = ["owner"]               # settings that must be filled in
auth = true                        # tools are unusable until signed in

[permissions]                      # shown to the user when enabling the skill
network = ["api.github.com"]       # http tools can reach only these hosts (enforced)
files = false                      # whether it handles files (for display)
description = "Reads and writes issues in your repositories"

[auth]
type = "token"                     # none | token | oauth2
label = "Personal access token"

# for type = "oauth2"
# authorize_url = "https://…/authorize"
# token_url = "https://…/token"
# revoke_url = "https://…/revoke"     # optional
# pkce = true
# params = { access_type = "offline" } # extra parameters for the authorize URL

[settings.owner]
label = "Default owner (user or org)"
# secret = true                    # stored in secrets.toml when true (client_secret and such)

[[groups]]                         # optional: feature groups the user can turn on and off
id = "issues"
label = "Issues"
scopes = []                        # oauth2 scopes (only the enabled groups' are requested)

[[tools]]
name = "github_issue_create"       # unique across all skills. a-z 0-9 _
description = "Create an issue."
group = "issues"                   # optional
permission = "ask"                 # default permission: disabled | ask | allow
risk = "write"                     # read | write (MCP annotations and the UI)
handler = "http"                   # builtin | http | command
method = "POST"
url = "https://api.github.com/repos/{{owner}}/{{repo}}/issues"
body = ["title", "body"]           # arguments that go into the JSON body
[tools.input_schema]               # JSON Schema
type = "object"
required = ["repo", "title"]
properties.repo = { type = "string" }
properties.title = { type = "string" }
properties.body = { type = "string" }
```

### Handlers

| handler | What it runs | Changes to Alice |
|---|---|---|
| `builtin` | code inside Alice (`builtin = "fs" \| "web" \| "google" \| "recall" \| "background"`) | for built-in skills |
| `http` | the declared HTTP request. `{{arg}}` / `{{settings.x}}` are filled into the URL (URL-encoded), arguments go to `query` / `body`, and the skill's token is sent as `Authorization: Bearer`. **Only hosts in `permissions.network` can be reached** | none |
| `command` | runs `command = ["python3", "run.py"]` inside the skill folder. stdin gets `{"tool", "arguments", "settings"}`; stdout is the result. The environment holds only PATH / HOME / LANG and the skill's own token (`ALICE_SKILL_TOKEN`). 60 seconds and 256 KB of output at most | none |

## Skill Manager

- Finds skills at startup and whenever the skills folder changes (by modification time). `alice skill rescan` does it on demand.
- Validates manifests and gives each skill a state: `ready` / `needs_setup` (settings or sign-in missing) / `missing_dependency` / `invalid` (manifest error) / `disabled`.
- **Newly found outside skills stay disabled.** The user enables them after seeing what they declare (network hosts, files, auth).
- A folder with the same id as a built-in skill takes its place (overriding the built-in).
- Skill folders are under `~/.alice`, which the file tools cannot read or write, so a model cannot create or change skills.

## Tool Registry

- Registers the tools of enabled skills whose dependencies are met. If two tools share a name, the skill loaded later becomes `invalid`.
- Effective permission: the tool's own setting → the skill id (older group names included) → the manifest default.
- Permissions are stored only in `tools.permissions` in `config.toml`, and only the user's API token can change them.

## Tool Gate (permission, approval, running, logging)

1. Look the tool up in the registry. Missing, skill off or not set up → an error goes back to the model
2. `disabled` → refused
3. `ask` → an approval request goes to every client; wait for the user's decision (up to 10 minutes)
4. Run it (the handler). The result is wrapped in `<untrusted>`
5. Remove token strings from output and errors
6. Record it in `tool_runs` (skill, tool, route, result, duration, error). Arguments and results are not recorded (they stay as messages in the thread and go when it is deleted)

A tool that panics returns an error instead of taking the turn down.

## MCP server (`alice mcp`)

- An MCP server over local **stdio**. Nothing is exposed to the network.
- It only relays to the Alice server and never runs tools itself. It holds no secrets (API keys, OAuth tokens, Alice's API token).
- Auth is a **turn token**: a single-use token Alice issues for one turn, usable only for `/api/mcp/*` (listing and calling tools). It dies when the turn ends, is stopped, or its thread is deleted. It cannot reach the approval or settings APIs.
- `tools/call` goes through the Tool Gate. For tools that need approval, the MCP reply waits until the user allows it in Alice's UI.

### Connecting the Codex CLI

Alice starts Codex with **its own CODEX_HOME (`~/.alice/codex/`)**.

- **Usual route (fast)**: one `codex app-server` stays running (started with the server). Each Alice turn gets an ephemeral Codex thread with the whole prompt Alice built, and the reply streams as it is written. No process starts per turn as with `codex exec`, which saves a few seconds before the first word.
- Tools are given as **dynamic tools** (an experimental app-server API). When Codex uses one, it sends `item/tool/call` on the same connection and Alice answers in process after the Tool Gate. No MCP child process and no token, so there is nothing to go around.
- **Fallback**: if the app-server cannot start, `codex exec` + `alice mcp` (below) is used. `ALICE_CODEX_EXEC=1` forces it.

- The user's own `~/.codex` (plugins, MCP servers, hooks, history, memories) is never loaded. The login is Alice's own too (`alice model login codex-cli`).
- Alice writes `config.toml` every time: shell, unified exec, browser, computer use, image generation, apps, plugins, hooks and web search off; a read-only sandbox; approval policy never; an empty temporary working directory; `alice mcp` as the only MCP server.
- Codex can call MCP tools only from code mode's `exec` (a V8 isolate without files, network or Node), so `code_mode_host` alone is left on. The isolate can call only the tools listed in `exec`'s description, and those are part of the safety check. Without `codex-code-mode-host`, MCP cannot work and Alice switches to the text protocol.
- MCP approvals are not done on the Codex side (`default_tools_approval_mode = "approve"`); Alice's Tool Gate decides.
- **Safety check**: before giving tools, Alice reads the tools the model would see with `codex debug prompt-input`. If even one tool outside the allow list (a shell, say) is there, MCP is not used and the text protocol takes over. The result is cached per Codex version.

### Why Codex cannot get around approvals

| Way around | Countermeasure |
|---|---|
| Calling Alice's approval API directly | On the app-server route, tool calls are handled inside Alice's process and no token exists. On the MCP route, the relay holds only the turn token, which cannot reach the approval API. Alice's API token is never given to Codex |
| Reading secrets.toml to steal a token | Codex's shell, code execution, plugins and other MCP servers are off, verified by the safety check. The file tools always refuse `~/.alice` |
| Posing as another tool through MCP | Tool names are resolved in the registry and permissions come from Alice's settings. MCP arguments carry no approval flag |
| Rewriting a skill to change permissions | Skills and settings live under `~/.alice`, which models cannot write. Changing permissions needs the user's API token |
| Auto-approval on the Codex side | Whatever Codex approves, Alice's Gate decides on its own |

## Text protocol (the other half of the hybrid)

For models that have neither native tool calling nor MCP (the Antigravity CLI, Codex when its safety check fails, …). The registry's tools go into the prompt, and when the model writes

````
```alice-tool
{"name": "calendar_create", "arguments": {"summary": "Dentist", "start": "…", "end": "…"}}
```
````

Alice picks it up, runs it through the Tool Gate, and hands the result back for the model to continue.

## Auth and token storage

- `token`: the token the user pastes goes to `secrets.toml` (0600) as `skill.<id>.token`.
- `oauth2`: PKCE + a loopback redirect (`http://127.0.0.1:<port>/oauth/callback`). `state` is single-use and valid for 15 minutes. The refresh token is stored as `skill.<id>.refresh_token`; access tokens stay in memory only. Signing out calls the revoke endpoint, then deletes.
- Tokens never appear to the model, the MCP relay, the logs or Web API responses. A command tool gets only its own skill's access token, through the environment.
- Settings such as the client ID go to `[skills.<id>.settings]` in `config.toml`; settings with `secret = true` go to `secrets.toml`.

## Turning skills on and off

```sh
alice skill                       # list and state
alice skill info google-workspace # description, tools, declared permissions
alice skill enable github         # enable (shows the declared permissions to confirm)
alice skill disable github
alice skill login google-workspace / logout
alice skill set github owner my-org
alice skill log                   # tool run log
```

The Web UI's "Settings → Skills" does the same.

## Logs

- The `tool_runs` table: time, thread, skill, tool, route (native / mcp / text), result (ok / error / denied / declined / cancelled), duration, error summary. Deleted with the thread, and all of it with delete-all.
- The server log (`alice log`): the same on one line. No arguments, result text or tokens.
