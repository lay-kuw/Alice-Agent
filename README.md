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

Web GUI: `http://127.0.0.1:7878/` を開き、`alice token` で表示されるトークンでサインイン。左のアイコン（スマホでは下のタブ）でチャット・カレンダー・メール・タスクを切り替えます。中身と操作は iPhone アプリと同じです（予定の色タグ、メールの既読切り替えと返信、タスクの追加・編集・完了など）。HTML メールはスクリプトを動かさない専用の枠で表示し、外部の画像は「画像を表示」を押すまで読み込みません。`#calendar`・`#mail`・`#tasks` の URL で直接開けます。

## コマンド

| コマンド | 説明 |
|---|---|
| `alice chat "質問"` | 新しいスレッドで 1 回だけ質問 |
| `alice chat list` / `alice chat <id>` | スレッド一覧 / 再開 |
| `alice chat rename <id>` / `delete <id>` / `clear` | 名前変更 / 削除 / 全削除（設定は維持） |
| `alice model [list\|refresh\|add\|remove\|use\|login\|logout\|test]` | モデル管理（`use 1` / `test 1` のように番号でも指定可。`use 2 high` で推論の深さも指定。`list` に利用可否 ✓/✗ と表示名・推論の深さを表示） |
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
- **モデル一覧の自動取得**: Codex（アカウントで使えるモデルと推論の深さ）、Gemini / Anthropic / OpenAI 互換サーバーは、起動時と 6 時間ごとに一覧を取り直します（`alice model refresh` で今すぐ）。推論の深さは `codex-cli/gpt-6.2-sol#high` のようにモデル ID の後ろに付けて指定し、Web・TUI・iPhone のモデル選択からも選べます。

## iPhone アプリ

`ios/Alice.xcodeproj` を Xcode で開き、Signing の Team を設定して実機にインストールします。サーバー URL と `alice token` のトークンで接続します（Tailscale の `tailscale serve` などの HTTPS 推奨）。

チャットのほか、カレンダー・メール・ToDo のタブがあります。どれも Alice サーバー経由で Google Workspace スキルの接続を使うので、iPhone に Google のトークンは置きません。ToDo（Google Tasks）を使うには `alice skill groups google-workspace` で Tasks をオンにし、`alice skill login google-workspace` で再接続します。

ToDo は右上の ＋ からタイトルと期日を入れて追加し、長押しのメニューから編集（タイトル・メモ・期日）や削除ができます。チャット一覧も長押しで名前変更・削除ができます。メールは右へスワイプで既読／未読を切り替え、開いたメールは右上のボタンから返信できます。HTML メールはそのまま表示しますが、スクリプトは動かさず、外部の画像も「Load images」を押すまで読み込みません（開封の追跡を防ぐため）。Gmail の変更権限（`gmail.modify`）が要るので、以前に接続した場合は `alice skill login google-workspace` で一度再接続してください。カレンダーは左上のアイコンで週／月表示を切り替え、週表示は左右スワイプで前後の週へ、予定一覧を左右スワイプで前後の日へ移動します。

予定には Google カレンダーと同じ 11 色のタグを付けられます（追加時、または予定を開いて変更）。

**ウィジェット**: ホーム画面に「Today」（次の予定）、「Month」（1 か月のカレンダーと次の予定）、「Tasks」（期日の近いタスク）、「Today + Tasks」（予定とタスクを左右に並べたもの）を置けます。ロック画面用もあります。ウィジェットはサーバーにもトークンにも触れず、アプリが App Group（`group.local.alice.app`）に書いた最新のスナップショットを表示するだけです。アプリを開いたとき・データが変わったとき・iOS のバックグラウンド更新で書き直されます。サーバーを切り替えると消えます。実機では Signing の Team を両ターゲット（Alice / AliceWidgets）に設定してください。Bundle ID を変える場合は、App Group 名も `Alice.entitlements`・`AliceWidgets.entitlements`・`WidgetSnapshot.appGroup` の 3 か所で揃えて変えます。

**ホームと Alice のひとこと**: iPhone アプリにはホーム画面（設定でオン/オフ）があり、ホーム・チャット・カレンダー・メール・To-do の上で Alice がひとこと話します。口調は設定の「Alice のひとことの口調」で変えられ（例: 元気でかわいい、丁寧な執事）、ひとこと自体もオフにできます（オフならモデルを使いません）。アプリはライトモードで日本語、ダークモードで英語の表示になります。

**エージェント（自動実行）**: Alice に決まったタイミングで自動でお願いできます。タイミングは「繰り返し」（N 分・時間・日・週ごと、最短 5 分。保存したらすぐ 1 回目）か「時刻指定」（毎日 12:00 と 18:00 など、Mac の時刻で）。結果は指定したチャット（指定しなければ最初の実行で作るチャット）に、Alice が自分から話しかけたように届きます（お願いの文はチャットに出ません）。「新しいメールがなければ何もしない」のように書いておけば、報告することがないときはチャットに何も残さず終わります。iPhone はチャット左上のハート →「Agents」、Web は左の「エージェント」から追加・編集・削除・今すぐ実行ができます。サーバーが止まっていた間の分はまとめて実行せず、次の時刻から再開します。API は `/api/agents`。

**エージェントの実行履歴**: Web のエージェント画面で各エージェントの「履歴」を押すと、直近 10 回の実行（時刻・結果・送り先・失敗理由）が見られます。結果は「完了／報告なし／承認待ち／エラー／中止」です。

**裏で動くタスク**: チャットで「時間かかるから裏でやっておいて」のように頼むと、Alice が新しいチャットを作ってそこで作業し、終わったら通知します（同時に 3 件まで）。

**スター（ピン留め）**: チャットを長押し（Web は右クリックか「⋯」）→「スター」で、一覧の先頭に固定します。

**長い会話**: 会話が長くなって Alice に送れる分（`agent.history_chars`、既定 6 万字）を超えると、古い部分を数行の要約にまとめて残します。チャットの履歴自体は消えません。

**思い出す**: 「前に話したカフェ」のような質問には、Alice が自分でキーワードを考えて、記憶と過去の会話を検索します（Mac の中だけで検索し、外部には送りません）。

**チャットの通知**: 開いていないチャットに返事が届くと通知します（エージェントの結果も含む）。iPhone はアプリを開いている間はすぐ、閉じている間は iOS のバックグラウンド更新のときにまとめて届きます。Web はエージェント画面の「通知」をオンにすると、ブラウザの通知で知らせます（HTTPS か localhost で開いたときのみ）。

**オフライン**: サーバーにつながらないときも、最後に読み込んだ予定・メール・ToDo・最近のチャットを表示します（見るだけで、変更はつながってから）。データは iPhone 内に暗号化して保存され（ロック中は読めない、バックアップ対象外）、サーバーを切り替えると消えます。

**通知**: 予定（10 分前、終日の予定は当日 9 時）、ToDo の期日（当日 9 時）、新着メールを通知します。どれも iPhone の中で作るローカル通知で、プッシュサーバーは使いません。予定と ToDo は読み込んだデータから先に登録しておくので、アプリを閉じていても鳴ります。新着メールは iOS のバックグラウンド更新のときに確認するため、届いてから数十分〜数時間遅れることがあります。種類ごとのオン／オフはチャット左上のハートのメニュー →「Notifications」で切り替えます。

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

既定は `127.0.0.1` のみ。同じ Wi-Fi や Tailscale の端末から直接つなぐときは、この Mac の IP を足します（`127.0.0.1` は常に有効なので書かなくて OK、複数はカンマ区切り）:

```sh
alice config set server.bind 192.168.1.20          # LAN の IP
alice config set server.bind 192.168.1.20,100.x.y.z # LAN と Tailscale
alice restart && alice token                       # つなぐ URL を表示
```

その IP がない（Wi-Fi が変わった等）ときは警告して飛ばし、Alice 自体は起動します。通信は平文 HTTP なので、信頼できるネットワークだけで使ってください。外から使うなら Tailscale（例: `tailscale serve 7878`）か HTTPS リバースプロキシ経由にし、`alice config set server.public_url https://...` を設定します。

API トークンは `alice token --set` で自分で決められます（12 文字以上、入力は非表示）。`alice token --rotate` でランダムに作り直し。どちらも再起動なしですぐ有効になるので、Web / iPhone で入れ直します。間違ったトークンが 1 分に 30 回続いた接続元は 1 分間拒否します。

## 開発

```sh
cargo test                                  # Rust（LLM・各サービスはローカルの偽サーバーで代替）
(cd ios/AliceKit && swift test)             # iOS クライアント層
scripts/bench.py target/release/alice 30    # ベンチマーク
```
