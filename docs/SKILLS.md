# Skill システム

Alice の機能拡張はすべて「スキル」として追加する。ファイル操作・Web 検索・Google Workspace も内蔵スキルで、外部スキルと同じ仕組みで動く。Alice 本体を変更せずに、`~/.alice/skills/` にフォルダを置くだけで機能を増やせる。

## 全体像

```
 ~/.alice/skills/<id>/          内蔵スキル（バイナリに同梱）
   SKILL.md   skill.toml        files / web / google-workspace
        │                              │
        └──────────┬───────────────────┘
                   ▼
            Skill Manager   検出・検証・依存チェック・有効/無効・設定
                   ▼
            Tool Registry   有効なスキルのツールを一元管理（名前・スキーマ・既定権限・リスク）
                   ▼
            Tool Gate       権限 → 承認 → 実行 → ログ（モデル・経路に依存しない唯一の入口）
         ┌─────────┼─────────────────┐
         ▼         ▼                 ▼
  ネイティブ   MCP (stdio)        テキスト方式
  Tool Calling  alice mcp ⇄ Codex   ```alice-tool``` ブロック
  OpenAI /      CLI など            Gemini CLI・ツール非対応モデル
  Anthropic /                       Codex の安全確認に失敗した場合
  Gemini / Ollama
```

- どの経路から呼ばれても、権限確認・承認・実行・ログは **Tool Gate（`skills::gate`）だけ**が行う。
- 承認は Alice の UI（Web / TUI / iPhone / Telegram / Discord）でのみ行える。モデル・MCP・スキルから承認する API は存在しない。

## スキルの構成

```
~/.alice/skills/github/
├── SKILL.md      モデル向けの説明と操作手順（必要なときだけ読まれる）
├── skill.toml    manifest：ツール・依存関係・必要権限・認証・設定
└── run.py        （command ツールを使う場合のみ）
```

### SKILL.md

モデルに渡す使い方の説明。毎回のプロンプトには入れず、システムプロンプトには「スキル名 + 1 行説明」の索引だけを入れる。モデルは必要なときに `skill_guide` ツールで全文を読む（トークン節約）。

### skill.toml（manifest）

```toml
id = "github"                      # フォルダ名と同じ。a-z 0-9 -
name = "GitHub"
version = "0.1.0"
description = "Issues and pull requests on GitHub"   # 索引に載る 1 行説明

[requires]
bins = []                          # PATH に必要な実行ファイル（command ツール用）
settings = ["owner"]               # 必須の設定項目
auth = true                        # 認証が済んでいないとツールは使えない

[permissions]                      # 有効化するときにユーザーへ表示する宣言
network = ["api.github.com"]       # http ツールはこのホストにしか接続できない（強制）
files = false                      # ファイルを扱うか（表示用）
description = "Reads and writes issues in your repositories"

[auth]
type = "token"                     # none | token | oauth2
label = "Personal access token"

# type = "oauth2" の場合
# authorize_url = "https://…/authorize"
# token_url = "https://…/token"
# revoke_url = "https://…/revoke"     # 任意
# pkce = true
# params = { access_type = "offline" } # 認可 URL に足すパラメータ

[settings.owner]
label = "Default owner (user or org)"
# secret = true                    # true なら secrets.toml に保存（client_secret など）

[[groups]]                         # 任意：ユーザーが個別にオンオフできる機能グループ
id = "issues"
label = "Issues"
scopes = []                        # oauth2 で要求するスコープ（有効なグループの分だけ要求）

[[tools]]
name = "github_issue_create"       # 全スキルで一意。a-z 0-9 _
description = "Create an issue."
group = "issues"                   # 任意
permission = "ask"                 # 既定の権限 disabled | ask | allow
risk = "write"                     # read | write（MCP の注釈と UI 表示に使う）
handler = "http"                   # builtin | http | command
method = "POST"
url = "https://api.github.com/repos/{{owner}}/{{repo}}/issues"
body = ["title", "body"]           # JSON ボディにする引数
[tools.input_schema]               # JSON Schema
type = "object"
required = ["repo", "title"]
properties.repo = { type = "string" }
properties.title = { type = "string" }
properties.body = { type = "string" }
```

### ハンドラ

| handler | 実行内容 | 本体変更 |
|---|---|---|
| `builtin` | Alice 内蔵の実装（`builtin = "fs" \| "web" \| "google"`） | 内蔵スキル用 |
| `http` | 宣言した HTTP リクエストを送る。`{{arg}}` / `{{settings.x}}` を URL に埋め込み（URL エンコード）、`query` / `body` に引数を割り当て、認証トークンを `Authorization: Bearer` で付ける。**接続先は `permissions.network` のホストに限定** | 不要 |
| `command` | スキルフォルダ内で `command = ["python3", "run.py"]` を実行。stdin に `{"tool", "arguments", "settings"}`、stdout が結果。環境変数は PATH / HOME / LANG と、そのスキル自身のトークン（`ALICE_SKILL_TOKEN`）だけ。60 秒・出力 256 KB まで | 不要 |

## Skill Manager

- 起動時と、スキルフォルダが変わったとき（更新時刻を見て自動）にスキルを検出する。`alice skill rescan` でも再検出できる。
- manifest を検証し、各スキルの状態を決める：`ready` / `needs_setup`（設定・認証が足りない） / `missing_dependency` / `invalid`（manifest エラー） / `disabled`。
- **新しく見つかった外部スキルは無効のまま**。ユーザーが宣言された権限（ネットワーク先・ファイル・認証）を確認して有効化する。
- 同じ id のフォルダがあれば内蔵スキルより優先する（内蔵スキルの上書き）。
- スキルフォルダは `~/.alice` 配下なので、ファイルツールからは読み書きできない。モデルがスキルを作ったり書き換えたりすることはできない。

## Tool Registry

- 有効で依存関係を満たすスキルのツールだけを登録する。名前が衝突した場合は後から読み込んだスキルを `invalid` にする。
- 実効権限：設定のツール名 → スキル id（旧グループ名を含む）→ manifest の既定値。
- 権限は `config.toml` の `tools.permissions` にだけ保存され、ユーザーの API トークンでしか変更できない。

## Tool Gate（権限・承認・実行・ログ）

1. ツールを Registry から引く。無い・スキル無効・未設定 → エラーをモデルに返す
2. 権限が `disabled` → 拒否
3. `ask` → 承認待ちイベントを全クライアントに送り、ユーザーの決定（最大 10 分）を待つ
4. 実行（ハンドラ）。結果は `<untrusted>` で包む
5. 出力・エラーから認証トークンの文字列を除去する
6. `tool_runs` に記録（スキル・ツール・経路・結果・所要時間・エラー）。引数と結果本文は記録しない（本文はスレッドのメッセージとして残り、スレッド削除で消える）

## MCP サーバー（`alice mcp`）

- ローカルの **stdio** で動く MCP サーバー。外部公開は不要。
- 中身は Alice サーバーへの中継だけで、自分ではツールを実行しない。秘密情報（API キー・OAuth トークン・Alice の API トークン）を持たない。
- 認証は **ターントークン**：Alice がそのターンのためだけに発行する使い捨てトークンで、`/api/mcp/*`（ツール一覧と実行）にしか使えない。ターンが終わる・停止される・スレッドが消えると即無効。承認 API や設定 API には使えない。
- `tools/call` は Tool Gate を通る。承認が必要なツールは、ユーザーが Alice の UI で許可するまで MCP の応答を返さない。

### Codex CLI との接続

Alice は Codex を **Alice 専用の CODEX_HOME（`~/.alice/codex/`）** で起動する。

- ユーザー普段の `~/.codex`（プラグイン・MCP サーバー・フック・履歴・記憶）は読み込まない。ログインも Alice 専用（`alice model login codex-cli`）。
- `config.toml` は Alice が毎回書き出す：シェル・統合 exec・ブラウザ・コンピューター操作・画像生成・アプリ・プラグイン・フック・Web 検索を無効化、サンドボックスは読み取り専用、承認ポリシーは never、作業フォルダは空の一時ディレクトリ、MCP サーバーは `alice mcp` だけ。
- Codex は MCP ツールを code mode の `exec`（ファイル・ネットワーク・Node のない V8 isolate）の中からしか呼べないため、`code_mode_host` だけは有効にする。isolate から呼べるのは `exec` の説明に列挙されたツールだけで、それも安全確認の対象。`codex-code-mode-host` が無ければ MCP は使えないので、テキスト方式に切り替える。
- MCP の承認は Codex 側では行わず（`default_tools_approval_mode = "approve"`）、Alice の Tool Gate が行う。
- **安全確認**：ツールを渡す前に `codex debug prompt-input` でモデルに見えるツール一覧を取り出し、許可リスト外のツール（シェルなど）が 1 つでもあれば MCP を使わず、テキスト方式に切り替える。結果は Codex のバージョンごとにキャッシュする。

### Codex から承認を迂回できない理由

| 迂回の手段 | 対策 |
|---|---|
| Alice の承認 API を直接呼ぶ | MCP 中継が持つのはターントークンだけで、承認 API には使えない。Alice の API トークンは中継にも Codex にも渡さない |
| secrets.toml などを読んでトークンを盗む | Codex のシェル・コード実行・プラグイン・他の MCP を無効化し、安全確認で検証。ファイルツールは `~/.alice` を常に拒否 |
| MCP 経由で別のツールを装う | ツール名は Registry で解決し、権限は Alice の設定で判定する。MCP の引数に承認フラグは存在しない |
| スキルを書き換えて権限を変える | スキルと設定は `~/.alice` 配下で、モデルからは書けない。権限変更はユーザーの API トークンが必要 |
| Codex 側の自動承認 | Codex が承認しても Alice の Gate は独立して判定する |

## テキスト方式（ハイブリッドのもう半分）

ネイティブ Tool Calling も MCP も使えないモデル（Gemini CLI、Codex の安全確認に失敗した場合など）向け。Registry のツール一覧をプロンプトに入れ、モデルが

````
```alice-tool
{"name": "calendar_create", "arguments": {"summary": "歯医者", "start": "…", "end": "…"}}
```
````

と書いたら Alice が取り出して Tool Gate で実行し、結果を渡して続きを書かせる。

## 認証とトークンの保存

- `token`：ユーザーが貼り付けたトークンを `secrets.toml`（0600）に `skill.<id>.token` として保存。
- `oauth2`：PKCE + ループバックリダイレクト（`http://127.0.0.1:<port>/oauth/callback`）。state は使い捨てで 15 分有効。リフレッシュトークンは `skill.<id>.refresh_token` に保存し、アクセストークンはメモリにだけ置く。ログアウト時は失効 API を呼んでから削除する。
- トークンはモデル・MCP 中継・ログ・Web API の応答に出さない。command ツールには、そのスキル自身のアクセストークンだけを環境変数で渡す。
- クライアント ID などの設定は `config.toml` の `[skills.<id>.settings]`、`secret = true` の設定は `secrets.toml` に入る。

## スキル単位の有効化・無効化

```sh
alice skill                       # 一覧と状態
alice skill info google-workspace # 説明・ツール・宣言された権限
alice skill enable github         # 有効化（宣言された権限を表示して確認）
alice skill disable github
alice skill login google-workspace / logout
alice skill set github owner my-org
alice skill log                   # ツール実行ログ
```

Web の「設定 → スキル」でも同じ操作ができる。

## ログ

- `tool_runs` テーブル：時刻・スレッド・スキル・ツール・経路（native / mcp / text）・結果（ok / error / denied / declined / cancelled）・所要時間・エラー概要。スレッド削除で一緒に消え、全削除で全消去。
- サーバーログ（`alice log`）：同じ内容を 1 行で。引数・結果本文・トークンは出さない。
