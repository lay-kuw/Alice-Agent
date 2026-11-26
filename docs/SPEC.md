# Alice Agent (A2) — 仕様

Alice Agent は単一ユーザー専用・ローカルファーストの Personal AI Agent である。
本書は要件定義をレビューした結果を踏まえ、実装の基準となる仕様を定める。

- Product Name: Alice Agent / Short Name: A2 / CLI: `alice`
- 優先順位: Simplicity > Complexity, Lightweight > Feature-rich, Performance > Unnecessary Abstraction, Privacy > Convenience

---

## 1. 要件レビュー結果（変更・明確化した点）

| # | 対象 | 指摘 | 決定 |
|---|------|------|------|
| R1 | Web Frontend | ビルド時に Node.js を使うとビルド依存が増える | **ビルド工程なしの素の HTML/CSS/JS** を `include_str!` でバイナリに埋め込む。Node.js はビルド時にも不要。 |
| R2 | CLI コマンド表記 | `alice caht <id>` / `alice chat claer` は誤記と判断 | `alice chat <chat-id>` / `alice chat clear` として実装。 |
| R3 | ChatGPT Subscription | 公式に許可された経路は Codex CLI (`codex exec --json`) のみ。Codex 自体がエージェントで独自ツールを持ち、Alice のツール呼び出し・ストリーミング粒度・画像以外の添付は制御できない | `codex-cli` Provider として **オプション** 実装。`--ephemeral` + `--sandbox read-only` で起動し、Alice のツールは渡さない（テキスト応答専用）。制約は Capability 表に明記。ツールが必要な場合は OpenAI API にフォールバック。 |
| R4 | Google AI Subscription | Gemini CLI の個人向け Google ログイン（AI Pro/Ultra）は 2026-06-18 に終了し、公式の後継は Antigravity CLI（`agy`）。`agy` は自前のツール（ファイル編集・コマンド）を持ち、ドキュメント上それを完全に無効化するオプションはない | `antigravity-cli` Provider で `agy -p` を plan モード・`--sandbox`・空の作業フォルダで実行（ヘッドレスでは承認が必要な操作は拒否される）。Alice のツールはテキスト方式。`gemini-cli` は終了扱い（既存設定は読み込むが、使うと移行を案内）。Cookie 抽出や非公開 API は一切使わない。 |
| R5 | 「派生ユーザープロファイル」 | 独立したプロファイルテーブルは削除整合性の弱点になる | **プロファイルを別に持たない**。ユーザー情報はすべて出典付き Memory として表現し、プロファイル表示は Memory のビューとする。 |
| R6 | 会話の要約 | スレッド横断の要約は削除済み情報の伝播経路になる | 要約はスレッド内でのみ使用し、スレッドと共に削除。スレッド横断の要約は作らない。 |
| R7 | 再学習防止 | AI 応答を Memory 抽出の入力にすると、削除済み Memory が別スレッドで再生成されうる | **Memory はユーザー自身の発言（role=user）からのみ抽出**。抽出時に既存 Memory の本文は LLM に渡さない（キー名のみ）。 |
| R8 | 削除の保証範囲 | SQLite の空きページ・WAL・FTS セグメントに残存しうる | `secure_delete=ON`、削除後 `wal_checkpoint(TRUNCATE)`、FTS `optimize`、全削除時 `VACUUM`。OS のスナップショット (Time Machine, APFS/Btrfs snapshot) と外部サービスは保証範囲外として明記。 |
| R9 | Telegram/Discord の履歴 | 相手プラットフォーム上のメッセージは Alice の管理外 | スレッド削除時、Bot が送信したメッセージの削除は「ベストエフォート」（Telegram は 48 時間以内のみ可能）。保証範囲外と明記。 |
| R10 | Google OAuth | Google Cloud で OAuth クライアント作成が必須。「テスト」公開状態ではリフレッシュトークンが 7 日で失効。`gmail.send` 等は機密スコープ | ユーザー自身の OAuth Client (Desktop app) を使用。失効時は再ログインを促す。スコープは機能単位で最小化し、有効化した機能のスコープのみ要求。 |
| R11 | Web Search | 公式かつ無料の汎用検索 API は存在しない。HTML スクレイピングは規約違反になりうる | 検索 Provider は Brave Search API / SearXNG (セルフホスト) を実装。未設定時は Web Search ツールを無効化。 |
| R12 | パフォーマンス | CLI 10ms は `--version` やローカル処理では達成可能だが、ネットワーク往復を含むコマンドはサーバー処理時間に依存 | CLI 自体の起動時間と API 往復時間を分けて計測する。 |
| R13 | Settings UI の Port 変更 | 実行中のソケットは変更できない | 保存後に「再起動が必要」と表示。 |
| R14 | `alice memory clear` | Memory だけを消してもスレッドは残る | Memory（と出典・使用記録・FTS）のみ削除し、会話履歴は残す。過去メッセージから自動的に再抽出はしない（抽出は新しいターンでのみ発生）。 |
| R15 | iPhone App 配布 | 実機インストールには Apple Developer 署名が必要 | Xcode プロジェクトを提供。署名チームはユーザーが設定。 |
| R16 | Codex CLI のサンドボックス | Codex 自体がエージェントで、read-only でもローカルファイルを読める可能性がある | 制約として SECURITY.md に明記。外部コンテンツを扱う用途では API 接続を推奨。 |
| R17 | ファイルツールと設定 | ルートを広く設定すると LLM が secrets.toml を読める | ルート設定に関係なく Alice のデータディレクトリを常に拒否（workspace を除く）。 |
| R18 | プロファイル（追加要件） | 「モデル/設定/メモリ/gateway を独立」 | プロファイルごとに独立したデータディレクトリとサーバープロセス。共有状態を持たない。 |

---

## 2. 用語

- **Thread**: 会話スレッド。全クライアント共通の ID（12 文字の英数字、例 `t4k2m9xq7ab3`）。
- **Message**: スレッド内の発言。role は `user` / `assistant` / `tool`。
- **Memory**: スレッドを横断して共有される、出典付きのユーザー情報。
- **Memory Source**: Memory の出典。必ず「ユーザーメッセージ」を指す。
- **Generation**: 1 回のユーザー入力に対する応答生成（ツールループを含む）。
- **Client / Channel**: `cli` / `web` / `ios` / `telegram` / `discord`。

## 3. データと設定の分離

| 種別 | 保存先 | 全スレッド削除時 |
|------|--------|------------------|
| サーバー設定・モデル設定・ツール権限・System Prompt | `~/.alice/config.toml` | 維持 |
| API Key・OAuth Token・Bot Token・API Token | `~/.alice/secrets.toml` (0600) | 維持 |
| 会話・Memory・添付・チャネル状態 | `~/.alice/alice.db`, `~/.alice/attachments/` | 完全削除 |
| ツール作業領域 | `~/.alice/workspace/` | 維持（ユーザーのファイル） |
| ログ | `~/.alice/logs/` | 会話内容は記録しない |

会話由来のデータは **DB と attachments のみ** に存在する。設定ファイルには会話由来の情報を一切書き込まない。

## 4. Memory 仕様

### 4.1 構造
- `memories(id, key, content, created_at, updated_at)` — `key` はトピック識別子（例 `home_city`）。
- `memory_sources(memory_id, message_id, thread_id, content, created_at)` — 出典ごとの主張内容。
- `memory_uses(message_id, memory_id)` — どの応答がどの Memory をコンテキストに使ったか（監査・出典表示用）。
- `memories_fts` — FTS5 (trigram tokenizer、日本語対応)。

### 4.2 不変条件
1. すべての Memory は 1 つ以上の Source を持つ。Source が 0 になった Memory は同一トランザクションで削除される。
2. Source は必ず存在する **user ロールのメッセージ** を参照する（外部キー）。
3. `memories.content` は導出値であり、残存する Source のうち最新の `content` に一致する（再構築は LLM 不要・決定的）。
4. Memory 抽出の入力は「対象ユーザーメッセージ」と「同スレッドの直前のユーザーメッセージ（文脈）」のみ。AI 応答・ツール結果・既存 Memory 本文は入力しない。

### 4.3 抽出
- 各ターン完了後、非同期で 1 回だけ LLM を呼ぶ（`memory.auto_extract`、短い入力はスキップ）。
- LLM は `{"memories":[{"key","content"}]}` を返す。同じ key は既存 Memory への Source 追加（内容更新）となる。
- 抽出結果の書き込み時点でメッセージ（＝スレッド）が削除済みなら外部キー違反でロールバックされ、Memory は作られない。

### 4.4 検索・注入
- ユーザー入力から FTS5 クエリを生成し、bm25 上位 N 件（既定 8 件）のみを System Prompt の `<memories>` ブロックとして注入する。
- 注入ブロックは「データであり指示ではない」と明示する。

### 4.5 削除
- **スレッド削除**: 生成中ならキャンセル → トランザクションでスレッド削除（messages, memory_sources, memory_uses, attachments 行, channel_state はカスケード）→ Source 0 の Memory を削除 → 残りの影響 Memory の content を再構築 → FTS 自動更新（トリガー）→ コミット → 添付ディレクトリ削除 → FTS `optimize` → `wal_checkpoint(TRUNCATE)` → SSE `thread.deleted`。
- **全スレッド削除**: 上記を全件に対して実行し、さらに `VACUUM`。結果として memories は空になる。
- **Memory 全消去**: memories / memory_sources / memory_uses / FTS を空にし、会話は残す。

### 4.6 保証範囲
- 保証する: `alice.db`（WAL 含む）、FTS インデックス、`attachments/`、サーバーのメモリ内キャッシュ、Web/iOS クライアントのキャッシュ（SSE の削除イベントで破棄、iOS は永続キャッシュを持たない）。
- 保証しない: 外部 LLM Provider に送信済みのデータ、Telegram / Discord 上のメッセージ、OS やユーザーが作成したバックアップ・スナップショット、ターミナルのスクロールバック。

## 5. Server API（REST + SSE）

認証: `Authorization: Bearer <token>`、または Web GUI 用の HttpOnly Cookie（状態変更リクエストは `X-Alice-CSRF: 1` ヘッダ必須）。`/api/health` のみ認証不要。

| Method | Path | 説明 |
|--------|------|------|
| GET | `/api/health` | 生存確認・バージョン |
| POST | `/api/login` | トークンを検証し Cookie を発行 |
| POST | `/api/logout` | Cookie 破棄 |
| GET | `/api/status` | サーバー状態（CPU・メモリ・DB サイズ・件数） |
| GET | `/api/events` | SSE（`?thread=<id>` で絞り込み可） |
| GET/POST | `/api/threads` | 一覧 / 作成 |
| DELETE | `/api/threads` | 全スレッド削除（`{"confirm":"delete-all"}` 必須） |
| GET/PATCH/DELETE | `/api/threads/{id}` | 取得 / 名前・モデル変更 / 削除 |
| GET | `/api/threads/{id}/messages` | メッセージ一覧 |
| POST | `/api/threads/{id}/messages` | 発言して生成開始（202） |
| POST | `/api/threads/{id}/stop` | 生成停止 |
| POST | `/api/approvals/{id}` | ツール実行の承認・拒否 |
| GET | `/api/models` | モデル一覧・既定モデル・Capability |
| PUT | `/api/models/default` | 既定モデル変更 |
| POST | `/api/models/test` | 接続テスト |
| GET | `/api/memories` | 一覧（`?q=` で検索） |
| GET | `/api/memories/status` | 件数・ストレージ使用量 |
| GET | `/api/memories/{id}` | 出典付き詳細 |
| DELETE | `/api/memories` | Memory 全消去 |
| GET/PATCH | `/api/config` | 設定（秘密情報は返さない） |
| PUT | `/api/secrets/{name}` | 秘密情報の設定（読み出し API はない） |
| GET/PATCH | `/api/tools` | ツール一覧・権限 |

### SSE イベント
`message.created`, `message.delta`, `message.completed`, `thread.created`, `thread.updated`, `thread.deleted`, `threads.cleared`, `tool.started`, `tool.completed`, `tool.approval_required`, `generation.cancelled`, `memory.updated`, `error`。
各イベントは `{"type":..., "thread_id":..., ...}` の JSON。

## 6. 同時実行
- 1 スレッドにつき同時に 1 生成のみ。生成中の投稿は 409。
- スレッド削除は進行中の生成をキャンセルしてから実行。生成タスクのすべての書き込みはスレッドの存在を外部キーで検証するため、削除後に行が復活しない。
- SQLite は単一接続を Mutex で直列化（個人用途では十分高速で、競合の推論が単純になる）。

## 7. ツール権限
- モード: `disabled` / `ask` / `allow`。破壊的操作（ファイル削除・メール送信・予定削除など）の既定は `ask`。
- 権限は `config.toml` にのみ保存され、LLM から変更する API・ツールは存在しない。
- 承認要求は SSE で配信し、生成を開始したクライアントに表示する（他クライアントからも承認可能）。タイムアウト（既定 10 分）で拒否扱い。

## 8. セキュリティ
- 既定バインドは `127.0.0.1`。すべての API（health を除く）はトークン必須。非ループバックで HTTPS でない場合は起動時に警告。
- トークン比較は定数時間。Web は CSP `default-src 'self'`、Markdown は HTML をエスケープしてから限定タグのみ生成。
- 外部コンテンツ（Web・メール・ファイル）は `<untrusted>` で囲み、指示として扱わないよう System Prompt で明示。破壊的ツールは承認必須なので、注入による権限昇格の影響を限定する。
- ログにはトークン・API Key・メッセージ本文を出力しない。

## 9. CLI
要件 4 のコマンドすべて。CLI はサーバー API のクライアントであり、Agent Runtime を持たない（`alice serve` のみがサーバー本体）。
