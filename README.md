<p align="center">
  <img src="docs/images/alice.png" width="200" alt="Alice">
</p>

<h1 align="center">✦ Alice Agent ✦</h1>

<p align="center">
  自分ひとりのための、ちいさくて速い AI エージェント ₍ᐢ. .ᐢ₎♡<br>
  単一バイナリ · ローカルファースト · SQLite だけで動きます
</p>

<p align="center">
  <a href="#-インストール"><img src="https://img.shields.io/badge/macOS-Apple%20Silicon-f5c2e7?logo=apple&logoColor=white" alt="macOS"></a>
  <a href="#-インストール"><img src="https://img.shields.io/badge/Linux-x86__64%20%7C%20arm64-cba6f7?logo=linux&logoColor=white" alt="Linux"></a>
  <a href="https://www.rust-lang.org"><img src="https://img.shields.io/badge/Rust-2024-f38ba8?logo=rust&logoColor=white" alt="Rust"></a>
  <a href="#-iphone-アプリ"><img src="https://img.shields.io/badge/iPhone-SwiftUI-fab387?logo=swift&logoColor=white" alt="iPhone app"></a>
  <a href="#-データ"><img src="https://img.shields.io/badge/storage-SQLite%20only-b4befe" alt="SQLite only"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-a6e3a1" alt="MIT License"></a>
</p>

<p align="center">｡･:*:･ﾟ★ CLI · Web · iPhone · Telegram · Discord で、同じ Alice とおしゃべり ★ﾟ･:*:･｡</p>

---

## 🌸 もくじ

- [できること](#-できること)
- [インストール](#-インストール)
- [はじめかた](#-はじめかた)
- [コマンド](#-コマンド)
- [モデルとサブスク](#-モデルとサブスク)
- [つながるもの](#-つながるもの)
- [iPhone アプリ](#-iphone-アプリ)
- [人格（SOUL.md）](#-人格soulmd)
- [プロファイル](#-プロファイル)
- [データ](#-データ)
- [リモートアクセス](#-リモートアクセス)
- [開発](#-開発)

## ✨ できること

- **🪄 ひとつのバイナリ**: `alice`（Rust）にサーバー・CLI・TUI・Web 画面が全部入り。Docker / Node.js / Redis / 外部 DB は不要
- **💬 どこからでも同じ会話**: CLI / Web / iPhone / Telegram / Discord で、スレッドも記憶も共有
- **🧠 出典つきの記憶**: 覚えたことには必ず元の会話が付きます。スレッドを消せば、そこから覚えたことも消えます
- **🔎 思い出す**: 「前に話したカフェ」みたいな聞き方でも、Alice がキーワードを考えて記憶と過去の会話を探します（検索はぜんぶ手元で）
- **📅 Google とつながる**: カレンダー・Gmail・ToDo を見たり、追加したり。トークンはサーバーだけが持ちます
- **⏰ エージェント**: 「毎朝 8 時にメールをまとめて」のように、決まった時間に Alice が自分から動きます。報告がなければ何も残さず静かに終了
- **🌙 裏で動くタスク**: 「時間かかるから裏でやっておいて」で、別のチャットでじっくり作業 → 終わったら通知
- **📱 かわいい iPhone アプリ**: ホーム・チャット・カレンダー・メール・やること、ウィジェット、通知、オフライン表示つき
- **🛡️ 安全第一**: ツールは承認制、HTML メールはスクリプトなしで表示、ログに会話や秘密は出しません
- **⚡ 軽い**: 待機中のメモリは十数 MB。返答はストリーミングで、長い会話は古い部分を要約して覚えておきます

## 🍓 インストール

```sh
git clone https://github.com/lay-kuw/Alice-Agent.git
cd Alice-Agent
scripts/install.sh            # ビルドして ~/.local/bin/alice に配置
scripts/install.sh --uninstall
```

対応: macOS（Apple Silicon）、Linux（x86_64 / ARM64）。[Rust](https://rustup.rs) が必要です。

## 🎀 はじめかた

```sh
alice setup      # 対話型セットアップ（サーバー・API トークン・モデル・自動起動）
alice start      # サーバー起動（launchd / systemd 登録済みならサービス経由）
alice chat       # TUI でおしゃべり（/ でコマンド候補、Esc で応答停止、--plain で行表示）
```

Web 画面は `http://127.0.0.1:7878/` を開いて、`alice token` で出てくるトークンでサインインします。やった〜 ٩(ˊᗜˋ*)و

左のアイコン（スマホ幅では下のタブ）で、チャット・カレンダー・メール・タスク・エージェント・設定を切り替えます。`#calendar`・`#mail`・`#tasks`・`#agents` の URL で直接開けます。

## 🐾 コマンド

| コマンド | 説明 |
|---|---|
| `alice chat "質問"` | 新しいスレッドで 1 回だけ質問 |
| `alice chat list` / `alice chat <id>` | スレッド一覧 / 再開 |
| `alice chat rename <id>` / `delete <id>` / `clear` | 名前変更 / 削除 / 全削除（設定は維持） |
| `alice model [list\|refresh\|add\|remove\|use\|login\|logout\|test]` | モデル管理（`use 1` のように番号でも、`use 2 high` で推論の深さも指定可） |
| `alice soul [edit\|show\|reset]` | 人格ファイル SOUL.md |
| `alice usage` | ChatGPT サブスクの 5 時間枠・週間枠の残量と、モデル別の使用トークン |
| `alice memory [list\|search "語"\|show <id>\|status\|clear]` | 記憶の管理（出典は `show` で） |
| `alice tool [list\|enable <tool> [ask\|allow]\|disable <tool>]` | ツールの権限 |
| `alice skill [list\|info\|enable\|disable\|setup\|login\|logout\|log]` | スキル（機能拡張）の管理・認証・実行ログ |
| `alice google [setup\|login\|logout\|status]` | google-workspace スキルの短縮形 |
| `alice gateway [list\|setup [telegram\|discord]\|delete <id>]` | Telegram / Discord |
| `alice profile [list\|<name>\|use <name>\|delete <name>]` | プロファイル |
| `alice config [get\|set <key> <value>\|secret <name>]` | 設定・秘密情報 |
| `alice service [install\|uninstall\|status]` | 自動起動（launchd / systemd） |
| `alice status` / `log -f` / `start` / `stop` / `restart` / `token` | サーバー管理 |

どのコマンドも `-p <profile>` で、一時的に別のプロファイルを対象にできます。

## 🧁 モデルとサブスク

API キーのほか、公式 CLI を通してサブスクのまま使えます。両方につないでおけば、チャットごとに Web・TUI・iPhone のモデル選択から切り替えられます。

<details>
<summary><b>💬 ChatGPT サブスク（codex-cli）</b></summary>

- `alice model add` → `codex-cli` → `alice model login codex-cli`
- Alice 専用のログインで、普段の `~/.codex` は使いません。ブラウザのないサーバーでは `alice model login codex-cli --device-auth`
- サーバーに [Codex CLI](https://github.com/openai/codex) を入れておく必要があります（ChatGPT のサブスクを使う公式の経路が CLI だけなので）
- スキルは内蔵の MCP サーバー経由で使え、承認は Alice の画面で行います

</details>

<details>
<summary><b>✨ Google AI サブスク（antigravity-cli）</b></summary>

- サーバーに Antigravity CLI を入れます: `curl -fsSL https://antigravity.google/cli/install.sh | bash`
- `alice model add` → `antigravity-cli` → `alice model login antigravity-cli`（中身は `agy` の起動）で一度サインイン
- ブラウザのないサーバーでは、表示された URL を手元のブラウザで開き、出てきたコードを貼り戻します
- `agy` は plan モード・サンドボックス・空の作業フォルダで動かし、スキルは Alice のテキスト方式で使います
- Gemini CLI のサブスクログインは 2026-06-18 に終了したので、`gemini-cli` は追加できなくなりました

</details>

<details>
<summary><b>🔑 API キー / ローカル</b></summary>

- OpenAI / Anthropic / Gemini / OpenAI 互換サーバー / Ollama
- モデル一覧は起動時と 6 時間ごとに取り直します（`alice model refresh` で今すぐ）
- 推論の深さは `codex-cli/gpt-6.2-sol#high` のように、モデル ID の後ろに付けて指定します

</details>

## 🧸 つながるもの

<details>
<summary><b>📅 Google（カレンダー・Gmail・ToDo）</b></summary>

- Google Cloud で「デスクトップ アプリ」の OAuth クライアントを作成 → `alice skill setup google-workspace` → `alice skill login google-workspace`
- 有効にしたサービスのスコープだけを要求します。ToDo は `alice skill groups google-workspace` で Tasks をオンにして再接続
- メールの既読切り替えには Gmail の変更権限（`gmail.modify`）が要ります。以前に接続した場合は一度再接続してください

</details>

<details>
<summary><b>🧩 スキル（機能拡張）</b></summary>

- 内蔵スキル: Google Workspace・ファイル・Web・思い出す（Recall）・裏で動くタスク（Background）
- `~/.alice/skills/<id>/` にフォルダ（`skill.toml` + `SKILL.md`）を置けば、Alice 本体を変えずに機能を追加できます（例: `skills/examples/github`）
- 作業フォルダ: `alice config set tools.fs.workspace ~/AliceWork`（ほかのフォルダは `tools.fs.roots` で追加）
- Web 検索: `alice config set tools.web.search_provider brave` + `alice config secret brave.api_key`（または SearXNG）
- 詳しくは [SKILLS](docs/SKILLS.md)

</details>

<details>
<summary><b>💌 Telegram / Discord</b></summary>

- `alice gateway setup`。許可したユーザー ID 以外は無視します
- Telegram は `/new /threads /model /delete /stop /status`、Discord は DM かメンションで同じコマンドが使えます

</details>

<details>
<summary><b>⏰ エージェント（自動実行）</b></summary>

- タイミングは「繰り返し」（N 分・時間・日・週ごと、最短 5 分。保存したらすぐ 1 回目）か「時刻指定」（毎日 12:00 と 18:00 など、サーバーの時刻で）
- 結果は指定したチャットに、Alice が自分から話しかけたように届きます（お願いの文はチャットに出ません）。指定しなければ最初の実行でチャットを作り、そのチャットを消すと「新しいチャット」に戻ります
- 「新しいメールがなければ何もしない」と書いておけば、報告がないときはチャットに何も残しません
- Web の「エージェント」で直近 10 回の実行履歴（時刻・結果・送り先・失敗理由）が見られます
- サーバーが止まっていた間の分はまとめて実行せず、次の時刻から再開します。API は `/api/agents`

</details>

<details>
<summary><b>🌙 裏で動くタスク・長い会話・通知</b></summary>

- **裏で動くタスク**: 「時間かかるから裏でやっておいて」と頼むと、新しいチャットで作業して、終わったら通知します（同時に 3 件まで）
- **長い会話**: 送れる分（`agent.history_chars`、既定 6 万字）を超えたら、古い部分を数行の要約にして覚えておきます。履歴自体は消えません
- **チャットの通知**: 開いていないチャットに返事が届くと通知します。Web はエージェント画面の「通知」をオンに（HTTPS か localhost のときのみ）

</details>

## 📱 iPhone アプリ

`ios/Alice.xcodeproj` を Xcode で開き、Signing の Team を両ターゲット（Alice / AliceWidgets）に設定して実機に入れます。サーバー URL と `alice token` のトークンで接続します（外からは Tailscale の `tailscale serve` などの HTTPS がおすすめ）。

- **🏠 ホーム**: Alice のあいさつ、今日の予定とやること、そのまま相談できる入力欄（設定でオン/オフ）
- **💬 チャット**: 左上 ≡ でチャット一覧、右上 ✨ で設定・エージェント・モデル。左から右へスワイプで最初の画面へ。長押しでスター（スター付きは削除されません）
- **📅 カレンダー**: 週・月表示、11 色の色タグ、日付の横の「今日」ボタン
- **✉️ メール**: すべて / 未読 / 重要、右スワイプで既読、返信。HTML メールは「元のレイアウトで表示」を押したときだけ（スクリプトなし、外部画像は押すまで読み込みません）
- **✅ やること**: すべて / 今日 / 今週、進み具合のバー、長押しで編集、完了でハートがぽんっ
- **🗨️ Alice のひとこと**: 各画面の上で Alice がひとこと。口調は自由に決められて、オフにもできます。最初は件数と時刻だけをモデルに送り、件名なども使うかは設定で選べます
- **🌗 ことば**: ライトモードは日本語、ダークモードは英語の表示

<details>
<summary><b>🔔 通知・🧩 ウィジェット・📴 オフライン</b></summary>

- **通知**: 予定（10 分前、終日の予定は当日 9 時）、やることの期日（当日 9 時）、新着メール、チャットの返事。どれも iPhone の中で作るローカル通知で、プッシュサーバーは使いません。新着メールとチャットは、アプリを閉じている間は iOS のバックグラウンド更新のときに届くので、遅れることがあります。種類ごとのオン/オフは ✨ → 設定 → 通知
- **ウィジェット**: Today / Month / Tasks / Today + Tasks（ロック画面用もあり）。ウィジェットはサーバーにもトークンにも触れず、アプリが App Group（`group.local.alice.app`）に書いたスナップショットを表示するだけです。Bundle ID を変える場合は、App Group 名を `Alice.entitlements`・`AliceWidgets.entitlements`・`WidgetSnapshot.appGroup` の 3 か所で揃えます
- **オフライン**: つながらないときも、最後に読み込んだ予定・メール・やること・最近のチャットを表示します（見るだけ）。iPhone 内に暗号化して保存し（ロック中は読めない、バックアップ対象外）、サーバーを切り替えると消えます

</details>

## 💖 人格（SOUL.md）

`~/.alice/SOUL.md`（プロファイルごと）が、Alice に渡す指示のすべてです。役割・言語・性格・話し方・仕事の進め方・安全のための約束まで、このファイルに書かれていて、書き換えればぜんぶ上書きできます。Alice が自動で足すのは、現在時刻・作業フォルダ・使えるスキルの一覧・話題に関係する記憶（最大 8 件）だけ。`{{name}}` はエージェント名に置き換わります。

`alice soul` か Web の「設定 → 人格」で編集でき、次の返答から反映されます。会話をぜんぶ消しても残り、AI 自身は書き換えられません。

## 🎈 プロファイル

```sh
alice profile eve        # "eve" を作って切り替え（別ポートで独立して動きます）
alice model add          # eve 専用のモデル
alice start              # eve のサーバーを起動
alice -p default chat    # 一時的に default を使う
alice profile default    # default に戻す
```

`default` は `~/.alice`、ほかは `~/.alice/profiles/<name>/` に、まるごと分かれて保存されます。

## 🍬 データ

```
~/.alice/
├── config.toml     設定（会話由来の情報は含まない）
├── SOUL.md         人格
├── secrets.toml    API キー・トークン（0600）
├── alice.db        会話・記憶・エージェント（SQLite, FTS5）
├── attachments/    添付ファイル（スレッド単位）
├── workspace/      ファイルツールの既定の作業フォルダ
├── skills/         追加スキル（<id>/skill.toml + SKILL.md）
├── codex/          Alice 専用の Codex 設定・ログイン
├── logs/           ログ（会話本文・秘密情報は出しません）
└── profiles/<name>/ ほかのプロファイル（同じ構成）
```

お引っ越しは `~/.alice` をまるごとコピーすれば OK です（サブスクの CLI だけは、新しいマシンでログインし直してください）。

## 🌐 リモートアクセス

既定は `127.0.0.1` だけ。同じ Wi-Fi や Tailscale の端末からつなぐときは、そのマシンの IP を足します（`127.0.0.1` は常に有効。複数はカンマ区切り。コンテナの中なら `0.0.0.0`）:

```sh
alice config set server.bind 192.168.1.20          # LAN の IP
alice config set server.bind 192.168.1.20,100.x.y.z # LAN と Tailscale
alice restart && alice token                       # つなぐ URL を表示
```

- その IP が今ない（Wi-Fi が変わった等）ときは、警告して飛ばし、Alice 自体は起動します
- 通信は平文 HTTP なので、信頼できるネットワークだけで。外から使うなら Tailscale（例: `tailscale serve 7878`）か HTTPS リバースプロキシ経由にして、`alice config set server.public_url https://...` を設定します
- API トークンは `alice token --set` で自分で決められます（12 文字以上）。`alice token --rotate` でランダムに作り直し。どちらも再起動なしですぐ有効です
- 間違ったトークンが 1 分に 30 回続いた接続元は、1 分間おやすみしてもらいます

## 🛠️ 開発

```sh
cargo test                                  # Rust（LLM・各サービスはローカルの偽サーバーで代替）
(cd ios/AliceKit && swift test)             # iOS クライアント層
scripts/bench.py target/release/alice 30    # ベンチマーク
```

設計のくわしい話: [SPEC](docs/SPEC.md) · [ARCHITECTURE](docs/ARCHITECTURE.md) · [SKILLS](docs/SKILLS.md) · [SECURITY](docs/SECURITY.md) · [BENCHMARK](docs/BENCHMARK.md) · [ROADMAP](docs/ROADMAP.md)

## 📜 ライセンス

[MIT](LICENSE) ♡
