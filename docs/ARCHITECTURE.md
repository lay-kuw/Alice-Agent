# Alice Agent (A2) — アーキテクチャ

## 1. 全体像

```
 alice (CLI) ─┐
 Web (embed) ─┤ HTTP REST + SSE (Bearer / Cookie)
 iPhone App ──┤
              ▼
        ┌──────────────── alice serve (単一プロセス / tokio) ───────────────┐
        │ server  : axum router, auth, SSE bus                              │
        │ agent   : turn loop (context build → provider stream → tools)    │
        │ memory  : extract / search / delete / rebuild                    │
        │ providers: openai(-compatible) / anthropic / gemini / codex-cli  │
        │ skills  : manager + tool registry + gate (native / MCP / text)   │
        │ channels: telegram (long-poll) / discord (gateway) ─ 内部クライアント │
        │ storage : SQLite (rusqlite, bundled, FTS5, WAL)                  │
        └───────────────────────────────────────────────────────────────────┘
```

- **単一バイナリ・単一 crate**。`alice serve` がサーバー、その他のサブコマンドは HTTP クライアント。
- Telegram / Discord はサーバー内のタスクとして動作し、Agent Core を直接呼ぶ（HTTP を経由しないが、同じ `agent::start_turn` を使う）。

## 2. 依存関係（最小限）

| crate | 用途 | 理由 |
|-------|------|------|
| tokio | 非同期ランタイム | ストリーミング・SSE・同時接続 |
| axum | HTTP/SSE | 薄く、tokio ネイティブ |
| reqwest (rustls, stream) | LLM / 外部 API | TLS を OS 非依存に |
| rusqlite (bundled) | SQLite + FTS5 | OS の SQLite バージョン差を排除 |
| serde / serde_json / toml | シリアライズ | 設定と API |
| clap | CLI 引数 | 宣言的で誤りが少ない |
| getrandom | ID・トークン生成 | OS CSPRNG |
| tokio-util | CancellationToken | 生成停止 |
| futures-util | ストリーム処理 | SSE 解析 |
| tokio-tungstenite | WebSocket | Discord Gateway |
| sha2 | SHA-256 | OAuth PKCE (S256) |

採用しないもの: LangChain 等のフレームワーク、ORM、tracing エコシステム（自前の小さなロガー）、テンプレートエンジン、フロントエンドビルドツール。

## 3. モジュール

```
src/
├── main.rs          サブコマンド振り分け
├── util.rs          ID 生成・時刻・ロガー
├── config/          Config (config.toml) / Secrets (secrets.toml, 0600) / Paths
├── storage/         DB 接続・マイグレーション・threads/messages の CRUD
├── memory/          抽出・検索・削除・再構築
├── providers/       共通型 + openai.rs / anthropic.rs / gemini.rs / cli.rs(codex, antigravity)
├── agent/           ターン実行・コンテキスト構築・ツールループ・承認待ち
├── skills/          スキル（manifest・検出・Tool Registry・Tool Gate・認証・http/command ハンドラ）
├── mcp.rs           `alice mcp`（stdio MCP 中継。Codex 用）
├── tools/           内蔵スキルの実装（fs / web / google）
├── server/          ルーター・認証・ハンドラ・SSE・埋め込み Web
├── channels/        telegram.rs / discord.rs
└── cli/             HTTP クライアント・各コマンド・ストリーミング Markdown 表示
web/                 index.html / app.js / style.css（ビルドなし、バイナリに埋め込み）
migrations/          0001_init.sql …（バイナリに埋め込み、user_version で管理）
```

## 4. Provider Adapter

抽象化は **1 つの enum と 1 つの関数** に留める。

```rust
pub struct ChatRequest { model, system, messages: Vec<ChatMessage>, tools: Vec<ToolSpec>, max_tokens }
pub enum Delta { Text(String), Reasoning(String) }
pub struct Completion { text, tool_calls: Vec<ToolCall>, provider_state: Option<Value>, usage, ttft_ms }
pub async fn stream(provider: &ProviderConfig, secret: Option<&str>, req, on_delta) -> Result<Completion>
```

- `ProviderKind` に応じて各モジュールの `stream` を呼ぶ。Agent Core は Provider の種類を知らない。
- 会話コンテキストは常に Alice が組み立てて毎回送信する（Provider 側のセッション・Memory 機能には依存しない）。
- Provider 固有の再送が必要な状態（Anthropic の thinking ブロック、Gemini の thoughtSignature）は `provider_state` としてメッセージに保存し、同じ Provider のときだけ再送する。モデル切替時は破棄されるので会話は継続できる。

### Capability 表

| Kind | Streaming | Tool Calling | Image | File | Reasoning | Auth |
|------|-----------|--------------|-------|------|-----------|------|
| openai | ✓ | ✓ | ✓ | テキスト化 | ✓ (モデル依存) | API Key |
| openai-compatible | ✓ | サーバー依存 | サーバー依存 | テキスト化 | – | API Key (任意) |
| ollama | ✓ | モデル依存 | モデル依存 | テキスト化 | – | なし |
| anthropic | ✓ | ✓ | ✓ | テキスト化 / PDF | ✓ (adaptive) | API Key |
| gemini | ✓ | ✓ | ✓ | テキスト化 | ✓ | API Key |
| codex-cli | 行単位 | MCP（`alice mcp`）。安全確認に失敗したらテキスト方式 | ✗ | ✗ | Codex 内部 | Alice 専用の ChatGPT ログイン (公式 CLI, `~/.alice/codex`) |
| antigravity-cli | 行単位 | テキスト方式（```alice-tool```） | ✗ | ✗ | – | Google ログイン（`agy` で一度サインイン。plan モード・サンドボックス・空フォルダで実行） |
| gemini-cli | – | – | – | – | – | 終了（2026-06-18 に Google がサブスクログインを終了。設定は読めるが使用不可） |

Context Window はモデル設定 (`context_tokens`) で管理し、履歴の切り詰めに使う。

## 5. Agent Loop

```
start_turn(thread, user_text, client)
  ├─ tx: user message INSERT, assistant message INSERT(status=streaming)
  ├─ SSE message.created ×2
  └─ spawn:
       loop (最大 16 ステップ)
         ctx = system + <memories>(FTS 上位 N) + 履歴(文字数予算内)
         completion = providers::stream(...)   ← delta ごとに SSE message.delta
         tool_calls なし → break
         各 tool call: 権限確認 (allow / ask→承認待ち / disabled→拒否)
                       実行 → tool message INSERT
       UPDATE assistant message (content, status=complete, meta)
       SSE message.completed
       memory::extract(user message)  ← 非同期、失敗しても会話に影響なし
```

- キャンセル: スレッドごとの `CancellationToken`。`select!` でストリームを中断し、部分応答を `status=cancelled` で保存。
- 削除競合: 書き込みはすべて `thread_id` / `message_id` の外部キーに依存。削除済みなら UPDATE が 0 行・INSERT が失敗し、生成タスクは静かに終了する。
- 計測: `meta` に `alice_ms`（コンテキスト構築・DB）、`ttft_ms`、`provider_ms` を分けて保存。

## 6. Storage

- WAL、`foreign_keys=ON`、`secure_delete=ON`、`busy_timeout=5000`。
- 単一 `Connection` を `std::sync::Mutex` で保護。await を跨いで保持しない。
- マイグレーションは `migrations/*.sql` を `include_str!` し、`PRAGMA user_version` で適用管理。

## 7. 認証

- `secrets.toml` の `server.token`（32 バイト乱数 hex）を `alice setup` / 初回起動時に生成。
- CLI は同じマシンの `secrets.toml` からトークンを読む。リモート CLI は `ALICE_URL` / `ALICE_TOKEN` 環境変数。
- Web: `/api/login` でトークンを送ると `alice_session` Cookie（HttpOnly, SameSite=Strict, HTTPS 時 Secure）。Cookie 認証の状態変更には `X-Alice-CSRF` ヘッダ必須（クロスオリジンからはプリフライトで拒否される）。
- iOS: トークンを Keychain に保存し Bearer で送信。

## 8. プロファイル

- プロファイル = 独立したデータディレクトリ + 独立したサーバープロセス。`default` は `~/.alice`、他は `~/.alice/profiles/<name>/`。
- パス解決: `$ALICE_HOME`（サービスから起動時）→ `-p/--profile` → `$ALICE_PROFILE` → `~/.alice/current_profile` → `default`。
- ポートは作成時に空きを割り当て。launchd ラベル / systemd ユニット / Web の Cookie 名もプロファイルごとに分離。
- 共有コードなし・共有状態なし。モデル、記憶、gateway、Google 連携、ツール権限がすべて独立する。

## 9. チャネル（Telegram / Discord）

- サーバー内のタスクとして動作し、`agent::start_turn` を直接呼ぶ。
- 共通層（`channels/mod.rs`）: コマンド、スレッド選択（`channel_state`）、ストリーミング編集（1.2 秒間隔）、承認ボタン、添付、投稿した外部メッセージの記録（`channel_messages`）と削除時のベストエフォート削除。
- プラットフォーム層は `Surface` トレイト（send / edit / delete / typing）だけを実装。Telegram は long polling、Discord は Gateway WebSocket + REST。
- 設定変更（`config.updated`）を検知して gateway を自動再起動。

## 10. スキル（Google Workspace ほか）

詳細は [SKILLS.md](SKILLS.md)。

- ファイル・Web・Google Workspace は内蔵スキル。外部スキルは `~/.alice/skills/<id>/`（`skill.toml` + `SKILL.md`）。
- すべてのツール呼び出しは `skills::gate::invoke` を通る（権限 → 承認 → 実行 → 秘密情報の除去 → `tool_runs` に記録）。
- 経路は 3 つ：ネイティブ Tool Calling（API モデル）、MCP（Codex: `alice mcp` + ターントークン）、テキスト方式（その他の CLI）。
- OAuth は汎用実装（PKCE + `state` 使い捨て + ループバック `/oauth/callback`）。ブラウザが別マシンなら、リダイレクト URL を CLI に貼り付けて完了。

## 11. iPhone アプリ

- `ios/AliceKit`（Swift Package）: API クライアント、SSE、`ChatStore`（@Observable、メモリのみ）、Markdown ブロック分割、Keychain。macOS でもテスト可能。
- `ios/Alice`（SwiftUI）: 画面のみ。永続キャッシュなし。スレッド削除は SSE で即時に反映。
- カレンダー・メール・ToDo タブ: `/api/google/*`（`src/server/google.rs`）を呼ぶ。データ取得は Google Workspace スキルのハンドラ（`src/tools/google.rs`）と共通で、トークンはサーバーから出ない。ユーザー自身の操作なので承認は求めないが、スキルの有効化・接続・グループ（calendar / gmail / tasks）に従う。

## 12. デプロイ

- macOS: `~/Library/LaunchAgents/local.alice.agent.plist`（launchd、KeepAlive）。
- Linux: `~/.config/systemd/user/alice.service`（systemd user unit）。
- サービス未登録時は `alice start` が `alice serve` をデタッチ起動し PID ファイルで管理。
- リモートアクセスは Tailscale (`tailscale serve`) または HTTPS リバースプロキシを推奨。
