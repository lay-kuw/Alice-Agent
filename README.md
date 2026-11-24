# Alice Agent (A2)

自分一人のための、高速・軽量・ローカルファーストな Personal AI Agent。

- 単一バイナリ `alice`（Rust）。サーバー・CLI・埋め込み Web GUI を含む
- CLI / Web / iPhone / Telegram / Discord でスレッドと Memory を共有
- Memory はすべて出典付き。スレッドを消せば、そこから得た記憶も消える
- プロファイルごとに独立した Alice（モデル・設定・記憶・gateway）
- Docker / Node.js / Redis / 外部 DB 不要。SQLite のみ

設計: [SPEC](docs/SPEC.md) / [ARCHITECTURE](docs/ARCHITECTURE.md) / [SKILLS](docs/SKILLS.md) / [SECURITY](docs/SECURITY.md) / [BENCHMARK](docs/BENCHMARK.md) / [ROADMAP](docs/ROADMAP.md)

## インストール

```sh
scripts/install.sh            # cargo でビルドして ~/.local/bin/alice に配置
scripts/install.sh --uninstall
```

対応: macOS (Apple Silicon), Linux x86_64 / ARM64。Rust (https://rustup.rs) が必要です。

## はじめかた

```sh
alice setup      # 対話型セットアップ（サーバー・API トークン・モデル・自動起動）
alice model      # モデルの一覧・既定モデル変更・追加・テスト
alice start      # サーバー起動（launchd / systemd 登録済みならサービス経由）
alice chat       # TUI チャット（/ でコマンド候補、Esc で応答停止、--plain で行表示）
alice usage      # サブスク（Codex）の残量と使用トークン
alice soul       # 人格（SOUL.md）をエディタで編集
```

Web GUI: `http://127.0.0.1:7878/` を開き、`alice token` で表示されるトークンでサインイン。

## コマンド

| コマンド | 説明 |
|---|---|
| `alice chat "質問"` | 新しいスレッドで 1 回だけ質問 |
| `alice chat list` / `alice chat <id>` | スレッド一覧 / 再開 |
| `alice chat rename <id>` / `delete <id>` / `clear` | 名前変更 / 削除 / 全削除（設定は維持） |
| `alice model [list\|add\|remove\|use\|login\|logout\|test]` | モデル管理（`use 1` / `test 1` のように番号でも指定可。`list` に利用可否 ✓/✗ を表示） |
| `alice soul [edit\|show\|reset]` | 人格ファイル SOUL.md（Alice への指示のすべて） |
| `alice usage` | ChatGPT サブスク（codex-cli）の 5 時間枠・週間枠の残量と、モデル別の使用トークン |
| `alice memory [list\|search "語"\|show <id>\|status\|clear]` | Memory 管理（出典の確認は `show`） |
| `alice tool [list\|enable <tool> [ask\|allow]\|disable <tool>]` | ツール権限 |
| `alice gateway [list\|setup [telegram\|discord]\|delete <id>]` | Telegram / Discord |
| `alice skill [list\|info\|enable\|disable\|setup\|login\|logout\|log]` | スキル（機能拡張）の管理・認証・実行ログ |
| `alice google [setup\|login\|logout\|status]` | google-workspace スキルの短縮形 |
| `alice profile [list\|<name>\|use <name>\|delete <name>]` | プロファイル |
| `alice config [get\|set <key> <value>\|secret <name>]` | 設定・秘密情報 |
| `alice service [install\|uninstall\|status]` | 自動起動（launchd / systemd） |
| `alice status` / `log -f` / `start` / `stop` / `restart` / `token` | サーバー管理 |

すべてのコマンドは `-p <profile>` で一時的に別プロファイルを対象にできます。

## プロファイル

```sh
alice profile eve        # "eve" を作成して切り替え（別ポートで独立動作）
alice model add          # eve 専用のモデル
alice start              # eve のサーバーを起動
alice -p default chat    # 一時的に default を使う
alice profile default    # default に戻す
```

`default` は `~/.alice`、その他は `~/.alice/profiles/<name>/` に完全に分離して保存されます。

## 連携

- **Telegram / Discord**: `alice gateway setup`。許可したユーザー ID 以外は無視します。Telegram は `/new /threads /model /delete /stop /status`、Discord は DM またはメンションで同じコマンドが使えます。
- **スキル**: Google Workspace・ファイル・Web は内蔵スキル。`~/.alice/skills/<id>/` にフォルダ（`skill.toml` + `SKILL.md`）を置けば、Alice 本体を変えずに機能を追加できます（例: `skills/examples/github`）。詳しくは [SKILLS](docs/SKILLS.md)。
- **Google**: Google Cloud で「デスクトップ アプリ」の OAuth クライアントを作成 → `alice skill setup google-workspace` → `alice skill login google-workspace`。有効にしたサービスのスコープだけを要求します。
- **作業フォルダ**: `alice config set tools.fs.workspace ~/AliceWork`（または Web の「設定 → ツール」）。相対パスはここに解決され、ほかのフォルダは `tools.fs.roots` で追加できます。
- **Web 検索**: `alice config set tools.web.search_provider brave` + `alice config secret brave.api_key`（または SearXNG）。
- **ChatGPT サブスク**: `alice model add` → `codex-cli` → `alice model login codex-cli`（Alice 専用のログイン。普段の `~/.codex` は使いません）。スキルは内蔵 MCP サーバー経由で使え、承認は Alice の画面で行います。

## iPhone アプリ

`ios/Alice.xcodeproj` を Xcode で開き、Signing の Team を設定して実機にインストールします。サーバー URL と `alice token` のトークンで接続します（Tailscale の `tailscale serve` などの HTTPS 推奨）。

## 人格（SOUL.md）

`~/.alice/SOUL.md`（プロファイルごと）が Alice に渡すシステムプロンプトのすべてです。既定の内容（役割・言語・性格・話し方・仕事の進め方・安全のための約束）もこのファイルに書かれていて、書き換えればすべて上書きできます。Alice が自動で足すのは現在時刻、作業フォルダ、使えるスキルの一覧（1 行ずつ）、話題に関係する記憶（最大 8 件）だけです。`{{name}}` はエージェント名に置き換わります。

`alice soul` か Web の「設定 → 人格」で編集でき、次の返答から反映されます。会話をすべて削除しても残り、AI 自身は書き換えられません。

## データ

```
~/.alice/
├── config.toml     設定（会話由来の情報は含まない）
├── SOUL.md         人格
├── secrets.toml    API Key・トークン（0600）
├── alice.db        会話・Memory（SQLite, FTS5）
├── attachments/    添付ファイル（スレッド単位）
├── workspace/      ファイルツールの既定の作業フォルダ（tools.fs.workspace で変更可）
├── skills/         追加スキル（<id>/skill.toml + SKILL.md）
├── codex/          Alice 専用の Codex 設定・ログイン
├── logs/           ログ（会話本文・秘密情報は出力しない）
└── profiles/<name>/ 他のプロファイル（同じ構成）
```

## リモートアクセス

既定は `127.0.0.1` のみ。他の端末からは Tailscale（例: `tailscale serve 7878`）または HTTPS リバースプロキシ経由で接続し、`alice config set server.public_url https://...` を設定してください。

## 開発

```sh
cargo test                                  # Rust（LLM・各サービスはローカルの偽サーバーで代替）
(cd ios/AliceKit && swift test)             # iOS クライアント層
scripts/bench.py target/release/alice 30    # ベンチマーク
```
