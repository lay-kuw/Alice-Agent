# Alice Agent (A2)

自分一人のための、高速・軽量・ローカルファーストな Personal AI Agent。

- 単一バイナリ `alice`（Rust）。サーバー・CLI・埋め込み Web GUI を含む
- 全クライアント（CLI / Web / iPhone / Telegram / Discord）でスレッドと Memory を共有
- Memory はすべて出典付き。スレッドを消せば、そこから得た記憶も消える
- Docker / Node.js / Redis / 外部 DB 不要。SQLite のみ

設計: [docs/SPEC.md](docs/SPEC.md) / [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) / 進捗: [docs/ROADMAP.md](docs/ROADMAP.md)

## ビルド

```sh
cargo build --release
cp target/release/alice ~/.local/bin/   # PATH の通った場所へ
```

対応: macOS (Apple Silicon), Linux x86_64 / ARM64。

## 使い方

```sh
alice setup      # 対話型セットアップ（サーバー設定・API トークン・モデル・自動起動）
alice model      # モデルの一覧・既定モデル変更・追加・テスト
alice start      # サーバー起動（launchd / systemd 登録済みならサービス経由）
alice chat       # 対話チャット（Ctrl-C で応答停止、/help でコマンド一覧）
```

Web GUI: `http://127.0.0.1:7878/` を開き、`alice token` で表示されるトークンでサインイン。

主なコマンド:

| コマンド | 説明 |
|---|---|
| `alice chat "質問"` | 新しいスレッドで 1 回だけ質問 |
| `alice chat list` / `alice chat <id>` | スレッド一覧 / 再開 |
| `alice chat delete <id>` / `alice chat clear` | スレッド削除 / 全削除（設定は維持） |
| `alice memory list` / `search "語"` / `show <id>` / `status` / `clear` | Memory 管理 |
| `alice model add` / `use <id>` / `login <provider>` / `test <id>` | モデル管理 |
| `alice tool list` / `enable <tool> [ask\|allow]` / `disable <tool>` | ツール権限 |
| `alice config get/set <key>` / `config secret <name>` | 設定・秘密情報 |
| `alice status` / `log -f` / `stop` / `restart` | サーバー管理 |

## データ

```
~/.alice/
├── config.toml     設定（会話由来の情報は含まない）
├── secrets.toml    API Key・トークン（0600）
├── alice.db        会話・Memory（SQLite, FTS5）
├── attachments/    添付ファイル（スレッド単位）
├── workspace/      ファイルツールの既定作業領域
└── logs/           ログ（会話本文・秘密情報は出力しない）
```

## リモートアクセス

既定は `127.0.0.1` のみ。他の端末からは Tailscale（例: `tailscale serve 7878`）または HTTPS リバースプロキシ経由で接続し、`server.public_url` を設定してください。

## テスト

```sh
cargo test
```

LLM はローカルのテスト用サーバーで代替するため、API キーは不要です。
