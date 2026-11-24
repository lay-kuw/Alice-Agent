# Alice Agent (A2)

自分一人のための、高速・軽量・ローカルファーストな Personal AI Agent。

- 単一バイナリ `alice`（Rust）。サーバー・CLI・埋め込み Web GUI を含む
- CLI / Web / iPhone / Telegram / Discord でスレッドと Memory を共有
- Memory はすべて出典付き。スレッドを消せば、そこから得た記憶も消える
- プロファイルごとに独立した Alice（モデル・設定・記憶・gateway）
- Docker / Node.js / Redis / 外部 DB 不要。SQLite のみ

設計: [SPEC](docs/SPEC.md) / [ARCHITECTURE](docs/ARCHITECTURE.md) / [SECURITY](docs/SECURITY.md) / [BENCHMARK](docs/BENCHMARK.md) / [ROADMAP](docs/ROADMAP.md)

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
alice chat       # TUI チャット（? でヘルプ、Esc で応答停止、--plain で行表示）
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
| `alice soul [edit\|show\|reset]` | 人格ファイル SOUL.md（性格・話し方・仕事の進め方） |
| `alice memory [list\|search "語"\|show <id>\|status\|clear]` | Memory 管理（出典の確認は `show`） |
| `alice tool [list\|enable <tool> [ask\|allow]\|disable <tool>]` | ツール権限 |
| `alice gateway [list\|setup [telegram\|discord]\|delete <id>]` | Telegram / Discord |
| `alice google [setup\|login\|logout\|status]` | Google アカウント連携 |
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
- **Google**: Google Cloud で「デスクトップ アプリ」の OAuth クライアントを作成 → `alice google setup` → `alice google login`。有効にしたサービスのスコープだけを要求します。
- **Web 検索**: `alice config set tools.web.search_provider brave` + `alice config secret brave.api_key`（または SearXNG）。
- **ChatGPT サブスク**: `alice model add` → `codex-cli`（公式 Codex CLI のログインを利用。テキスト専用）。

## iPhone アプリ

`ios/Alice.xcodeproj` を Xcode で開き、Signing の Team を設定して実機にインストールします。サーバー URL と `alice token` のトークンで接続します（Tailscale の `tailscale serve` などの HTTPS 推奨）。

## 人格（SOUL.md）

`~/.alice/SOUL.md`（プロファイルごと）に書いた性格・話し方・仕事の進め方が Alice の人格になります。`alice soul` か Web の「設定 → 人格」で編集でき、次の返答から反映されます。会話をすべて削除しても残り、AI 自身は書き換えられません。

## データ

```
~/.alice/
├── config.toml     設定（会話由来の情報は含まない）
├── SOUL.md         人格
├── secrets.toml    API Key・トークン（0600）
├── alice.db        会話・Memory（SQLite, FTS5）
├── attachments/    添付ファイル（スレッド単位）
├── workspace/      ファイルツールの既定作業領域
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
