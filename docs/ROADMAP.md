# Alice Agent (A2) — ロードマップ

状態: ☐ 未着手 / ◐ 進行中 / ☑ 完了（自動テストまたは実機確認済み）

## 計測値（macOS Apple Silicon, release build）
- CLI 起動: `alice --version` 中央値 3.5ms、`alice chat list`（API 往復込み）3.8ms
- サーバー待機時メモリ (RSS): 約 11–13MB
- リリースバイナリ: 8.5MB（Web GUI 埋め込み済み）

## プロファイル
- ☑ `alice profile <name>` で独立インスタンス（設定・秘密情報・DB・gateway・ポート・サービス）

## Phase 1 — Core
- ☑ Rust プロジェクト・設定・秘密情報・パス
- ☑ SQLite スキーマ・マイグレーション
- ☑ Server API（threads / messages / events / status）・トークン認証
- ◐ Provider: OpenAI / OpenAI-compatible / Ollama / Anthropic / Gemini（ストリーミング・ツール呼び出し）— 各 API の SSE 形式を再現したテストで検証済み。実 API キーでの疎通は未確認
- ☑ Agent Loop・キャンセル・削除競合対策
- ☑ CLI: setup / start / stop / restart / status / log / chat / model / config / memory / tool / token
- ☑ テスト: ローカルの OpenAI 互換テストサーバーを使った結合テスト

## Phase 2 — Memory
- ☑ 抽出（user メッセージのみ）・key による統合
- ☑ FTS5 trigram 検索・コンテキスト注入
- ☑ 出典追跡・スレッド削除時の再構築・全削除
- ☑ CLI: memory list / search / show / status / clear
- ☑ テスト: 横断参照・削除・非復活・競合・全削除後に空

## Phase 3 — Web
- ☑ 埋め込み Web Chat（ストリーミング・Markdown・停止・モデル選択）
- ☑ Settings（General / Models / Channels / Tools / Memory / System）
- ☑ Cookie 認証・CSRF・CSP
- ☑ SSE によるリアルタイム同期

## Phase 4 — Tools
- ☑ 権限ゲート（disabled / ask / allow）・承認 API
- ☑ File System（ルート制限・シンボリックリンク対策）
- ☑ Web Search（Brave / SearXNG）・URL Fetch・本文抽出（SSRF 対策込み。Brave/SearXNG は実キーでの疎通未確認）
- ☑ Google（OAuth 2.0 PKCE, Gmail / Calendar / Drive / Docs / Sheets）— 偽 Google API で検証。実アカウントでの疎通は未確認

## Phase 5 — Channels
- ☑ Telegram（long polling, allowlist, コマンド, 承認ボタン）— 偽 Bot API で検証。実 Bot での疎通は未確認
- ☑ Discord（Gateway, allowlist, コマンド, 承認）— 偽 Gateway/REST で検証。実 Bot での疎通は未確認
- ◐ Subscription Provider: codex-cli（公式 JSONL 形式で検証、実ログインでの疎通は未確認）/ gemini-cli（Antigravity CLI への置き換えにより実験的）

## Phase 6 — iPhone
- ◐ SwiftUI アプリ（接続・Keychain・スレッド・ストリーミング・承認・添付・停止・モデル選択）— AliceKit は実サーバーとの通しテスト済み、アプリはシミュレータ向けビルド成功。実機/シミュレータでの画面確認は未実施（シミュレータランタイム未導入）

## Phase 7 — Release
- ☐ インストールスクリプト・launchd / systemd
- ☐ ベンチマーク（CLI 起動・アイドルメモリ・Alice 処理時間）
- ☐ セキュリティレビュー・ドキュメント
