# Alice benchmark (darwin, 30 turns, fake LLM first-token delay 50 ms)

| Metric | Value |
|---|---|
| CLI: `alice --version` (median) | 4.2 ms |
| CLI: `alice chat list (API round trip)` (median) | 4.0 ms |
| CLI: `alice status (API round trip)` (median) | 5.5 ms |
| Idle RSS after start | 11.0 MB |
| Idle RSS after 30 turns | 13.6 MB |
| Alice overhead per turn (context + memory search + DB), median | 0.30 ms |
| Provider time per turn (fake LLM), median | 54.0 ms |
| Provider time to first token, median | 54.0 ms |
| Client wall time per turn incl. polling, median | 60.3 ms |
| Alice overhead per turn, max | 0.97 ms |
| Database size after 30 turns (incl. WAL) | 929 KB |

測定日: 2026-11-24 / macOS 27.2, Apple Silicon / release ビルド（バイナリ 9.3 MB）
再現: `cargo build --release && scripts/bench.py target/release/alice 30`

## 目標との比較

| 目標 | 結果 |
|---|---|
| CLI Startup 10 ms 未満 | 達成（`--version` 約 4 ms、API 往復を含むコマンドも 4〜6 ms） |
| Idle Memory 20 MB 未満 | 達成（起動直後 約 11 MB、30 ターン後 約 14 MB） |
| Alice 自体の処理時間 | 1 ターンあたり中央値 0.3 ms（コンテキスト構築・Memory 検索・DB 書き込み）。応答時間のほぼ全てが LLM 側 |
| 不要なバックグラウンド処理なし | 待機中のタイマーはなし（SSE 接続ごとの 20 秒キープアライブ、有効な gateway の long polling / heartbeat のみ） |
| 不要な LLM 呼び出しの削減 | スレッド名は LLM を使わず自動生成。Memory 抽出は 1 ターン 1 回、短文はスキップ |

## 注記
- "Client wall time" はベンチマークの 5 ms ポーリング間隔を含む。実クライアントは SSE で即時に受信する。
- DB サイズは WAL を含む。スレッド削除や全削除の際に WAL は切り詰められる。
- Linux（x86_64 / ARM64）の測定は未実施。CI ワークフロー（`.github/workflows`）で同じテストを実行する。
