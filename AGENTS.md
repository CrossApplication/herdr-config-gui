# AGENTS.md

このリポジトリで作業するとき最初に読むファイル。**なぜそうなっているか**を書いてある。
利用者向けの導入と使い方は [README.md](README.md) にある。

herdr 本体の設定項目は**このリポジトリに書かれていない**。起動時に `herdr --default-config`
を実行し、その出力からフォームを生成する。したがって「設定を追加する」という作業は基本的に
発生せず、herdr のバイナリが唯一のスキーマ源である。この前提が以下すべての土台になる。

## 開発

```sh
cargo run --release              # debug は描画が重いので release 推奨
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test -- --test-threads=1   # HERDR_CONFIG_PATH を書き換えるテストがあるため単一スレッド
```

`HERDR_CONFIG_PATH` で編集対象のファイルを差し替えられる。これは herdr 自身の
環境変数なので、`herdr config check` と `herdr server reload-config` も同じ
ファイルを対象にする。

ウィンドウを観察できない環境のために、確認用の抜け道を 3 つ残してある。

| 環境変数 | 動作 |
| --- | --- |
| `HERDR_GUI_DUMP=1` | ウィンドウを開かず、UI に渡すモデルの中身とカラーピッカーの往復を標準出力に吐いて終了する |
| `HERDR_GUI_TRACE=1` | 起動 1.5 秒後にウィンドウの実寸・可視状態・スケールを報告して終了する |
| `HERDR_GUI_FILTER=<語>` | 検索欄に語を入れた状態で開く。特定の設定の見栄えを確認するとき用 |

### CI

GitHub Actions が ubuntu / macOS / windows の 3 OS で `fmt` / `clippy` / `test` を回す。
ランナーに herdr は入っていないので、スキーマテストは
`fixtures/default-config.toml`（`herdr --default-config` のスナップショット）
を読む。herdr が存在する環境では `fixture_matches_installed_herdr` が
スナップショットのずれを検出し、いない環境では自分でスキップする。
スナップショットの更新は次のとおり。

```sh
herdr --default-config > fixtures/default-config.toml
```

## 設計方針

### 設定項目表を内蔵しない

起動時に `herdr --default-config` を実行し、その出力からフォームを生成する。
herdr のバイナリが唯一のスキーマ源なので、herdr が更新されて設定項目が増えても
このアプリを更新せずに新項目が現れる。

出力は TOML パーサではなく**行指向**で読む。herdr の既定設定はほぼ全項目が
コメントアウトされており、TOML としてパースすると `# key = value` の既定値と
それを説明する散文の両方が消えてしまう。`# [theme.custom]` のような
コメントアウトされたセクションヘッダも追跡する必要がある。

現行の herdr 0.9.0 では 140 項目 / 25 セクションを抽出する。

### 変更した項目しか書かない

各設定は 3 状態を持つ。

| 状態 | config.toml |
| --- | --- |
| 既定を継承 | 行なし |
| 設定済み | その行だけ書く |
| 無効 (`""`) | `key = ""` |

書き込みは `toml_edit` を通すので、コメント・空行・キー順序・行末コメントが保持され、
触っていない設定の書式も変わらない。既定に戻した設定は既定値を書き出すのではなく
ファイルから削除する。

### 保存フロー

```
差分の算出 → herdr config check（一時ファイル） → バックアップ → 書き込み → reload
```

**検証はバックアップより前にある。** 致命的な内容はディスクに届く前に止まるので、
その場合はバックアップも作られず、既存の `config.toml` は一切触られない。

| 段階 | 内容 |
| --- | --- |
| 検証 | 書き込み予定の全文を一時ファイルに置き、`HERDR_CONFIG_PATH` を向けて `herdr config check` |
| バックアップ | `config.toml.bak-<epoch>` |
| 書き込み | `toml_edit` による最小差分 |
| 反映 | `herdr server reload-config` |

## 対応プラットフォーム

herdr の配布に合わせて macOS、Linux、Windows。ただし macOS は Apple Silicon (aarch64) のみを
対象とし、Intel Mac は対象外とする。

### 設定ファイルの場所は herdr に聞く

`herdr --help` は解決済みのパスを自分で出力する。

```
Config: /Users/me/.config/herdr/config.toml
Env:    HERDR_CONFIG_PATH overrides config file path
```

これを読むので、`HERDR_CONFIG_PATH`・`XDG_CONFIG_HOME`・Windows の `%APPDATA%`
レイアウトを再実装する必要がない。herdr が見つからないときだけ自前の計算に落ちる
（`resolve_config_path` が Windows / XDG / `~/.config` の分岐を純粋関数として持ち、
どの OS 上でもテストできる）。

### herdr に検証させる

`herdr config check` は合否だけでなく、認識できないセクションとキー、期待する型、
enum のメンバーまで報告する。`HERDR_CONFIG_PATH` を一時ファイルに向けて実行すれば、
まだ書いていない内容を検証できるので、保存は「書く前に検証」する。

判定で重要なのは出力末尾の `; using defaults` の有無である。未知のキーは無視されて
他の設定は有効になるが、型や構文のエラーは **設定ファイル全体が破棄されて既定値に
戻る**。後者は保存を中止する。

この仕組みは別名の検出にも使える。名前とその別名を同時に書くと herdr は
`duplicate field` を報告するため、「未文書の設定」と「既存設定の別名」を区別できる
（`keys.fullscreen` は `zoom` の、`advanced.scrollback_lines` は
`scrollback_limit_bytes` の別名だった）。

### 手書きのオーバーレイ層

`--default-config` はドキュメントであってスキーマではない。テーブルのメンバーを
全部並べるのではなく一部を例示する。その穴を `src/overlay.rs` が埋める。

- `[theme.custom]` と light/dark が受け付けるカラートークン **19 個**
  （`--default-config` の記載は 7 個と 2 個ずつ）
- herdr が受理するのに文書化されていない設定 **7 個**
  （`keys.swap_pane_*`、`keys.copy_mode`、`ui.agent_panel_scope`、
  `[[keys.command]]` の `description`）

このファイルのテストは全ての名前を実際の herdr に問い合わせ、でっち上げの名前が
拒否されることも確認する。enum のメンバーは herdr が報告した値と一致するか比較する。
つまりオーバーレイは黙って現実からずれない。

さらに `schema` のテストが全設定を一括で書き出して herdr に検証させる。これは実際に
`accent` の誤配属を検出した（コメントアウトされたセクションヘッダの有効範囲が
無限に続き、`[ui]` の `accent` が `ui.sidebar.spaces.accent` になっていた）。

### 色は自前で検証する

herdr は色を一切検証しない。`accent = "notacolor"` は `config check` を通り、その後
黙って無視される。そのため GUI 側で 16 進 / `rgb()` / 名前付き色 / `reset` を判定する。
ただし herdr が受け付ける「名前」の一覧は文書化されていないため、知らない名前は
警告せず受け入れる。動くかもしれない値を警告するほうが害が大きい。

### 配列テーブルは一覧として扱う

`[[keys.command]]` はエントリの並びであって設定の集合ではない。スキーマ上の項目は
1 エントリの形を表すテンプレートで、フォームはエントリごとに展開して
`keys.command[0].key` のような添字付きパスに書き込む。単一の `[keys.command]`
テーブルとして書くと **herdr は設定ファイル全体を破棄する**ので、この区別は必須である。

新しいエントリは末尾への追加しかできない（TOML に「隙間」を表す手段がないため）。
削除は添字の大きいものから適用する。順序を逆にすると、削除でずれた添字のせいで
ユーザーが選んだのとは別の行が消える。

popup の `width` / `height` は 1 つのフィールドに 2 つの TOML 型が入る。
パーセントは文字列 (`"80%"`, 1〜100%)、セル数はクォートなしの整数。
herdr は `width = "120"` を拒否するため、入力に応じてクォートを変える。

### サイドバーの行は構造として編集する

`ui.sidebar.agents.rows` などは「行 × トークン」の二次元配列で、トークンは組み込み名、
`$name` 形式のメタデータ値、または `{ token = "workspace", fg = "#89b4fa", bold = true }`
というインラインスタイルのいずれか。生の TOML で手打ちするには構造が深すぎるため、
行とトークンを追加・削除・並べ替えできるエディタを用意している。

トークンの集合は行の種類ごとに異なり、互換性はない。`agent` は spaces の行では拒否され、
`branch` は agents の行では拒否される。

| 種類 | トークン |
| --- | --- |
| agents (9) | `state_icon` `state_text` `machine` `workspace` `tab` `pane` `agent` `terminal_title` `terminal_title_stripped` |
| spaces (5) | `state_icon` `state_text` `workspace` `branch` `git_status` |

スタイルが受け付けるのは `token` `fg` `bold` `dim` のみ。`bg` や `italic` は拒否される。
`fg` は **`#rgb` / `#rrggbb` だけ**で、名前付き色も `rgb()` も通らない（`[theme.custom]`
とは規則が違う）。

パースは意図的に狭く作ってある。エディタが表現できない書き方だった場合は構造を推測せず、
生の TOML 入力にフォールバックする。半分だけ理解した値を書き換えるより安全なため。

### タブバーの項目は herdr に形を聞いて組み立てる

`ui.tab_bar_right` はインラインテーブルの配列で、`--default-config` は
`tab_bar_right = []` と型の名前だけを書き、各型が取るフィールドには触れていない。
そこで `config check` に未知のフィールドを渡して名前を吐かせた。

```
unknown field `zzz`, expected one of `command`, `interval_seconds`, `timeout_seconds`
```

| type | フィールド | 必須 |
| --- | --- | --- |
| `zoom` / `hostname` | なし | |
| `datetime` | `format` (string) | |
| `text` | `text` (string) | ✓ |
| `command` | `command` (string) | ✓ |
| | `interval_seconds` / `timeout_seconds` (u64、1 以上) | |

型ごとの必須フィールドが欠けていると **herdr は設定ファイル全体を破棄する**
（`missing field \`text\``）。一方、値が空だったり `interval_seconds = 0` だったりする場合は
その項目だけを隠して残りは有効にする。そのため編集器は入力途中の項目でも
`text = ""` の形で書き出し、破棄される状態を作らない。隠される条件は警告として画面に出す。

`zoom` と `hostname` は余計なフィールドを黙って無視するが、編集器にはそれを置く場所がない。
保存時に黙って消すよりは生の TOML 入力に落ちるほうが安全なので、パースを拒否している。

### 行末を保持する

`toml_edit` は文書を描画するとき改行をすべて LF に正規化する。CRLF のファイルを
そのまま書き戻すと触っていない行まで差分になるため、読み込み時に行末を検出して
書き込み時に復元する。Windows でメモ帳が書いた設定ファイルでも 1 項目の変更が
1 行の差分で済む。

### バイナリの探索

GUI は Finder / Explorer から起動するとシェルの PATH を継承しないため、PATH で
見つからない場合は各 OS のインストール先を探す。この経路は macOS のバンドル済み
`.app` を PATH なしで起動して検証済み（`~/.local/bin/herdr` を発見する）。
Windows と Linux では未検証。Windows では `.exe` 付きで
`%LOCALAPPDATA%\Programs\Herdr\bin`、`%USERPROFILE%\.local\bin`、
それと `%USERPROFILE%\.herdr\packages\standalone\releases` 配下の最新
バージョンディレクトリを見る。

## キーバインド

`[keys]` 系の 58 項目は実際のキー入力で設定する。prefix モードのバインドは
prefix キーを実際に押してから目的のキーを押す（ダイアログが設定中の prefix を
認識して `prefix+<chord>` に折り畳む）。Esc と Enter 自体もバインドできるよう、
録音中は全キーを飲み込み、確定後に Esc = キャンセル / Enter = 確定へ切り替わる。

### 物理キーと論理キーを使い分ける

打鍵の読み方は文字種で変える必要がある。

| 対象 | 使う値 | 理由 |
| --- | --- | --- |
| 英字・数字・F キー | 物理キー | shift で大文字化し、macOS では alt が合成文字に変える（`option+a` は `å`） |
| 記号 | 論理キー | 端末が送るのは文字そのもの。`shift+7` は `&` として届き、herdr の名前は `ampersand` |

Slint の公開 API が渡すのは**論理キーと修飾フラグだけ**で、物理キーは捨てられている。
ただし winit は持っており、Slint の winit バックエンドは横取りを許してくれる。

```rust
slint::BackendSelector::new()
    .with_winit_custom_application_handler(PhysicalKeyRecorder)
    .select()?;
```

`CustomApplicationHandler` は「Slint が見る前に呼ばれる」と明記されている。押下のたびに
`winit::event::KeyEvent::physical_key` を記録し、Slint が同じ押下を届けた時点で読み出す。
`EventResult::Propagate` を返すので Slint 側の処理は変わらない。これに `slint` の
`unstable-winit-030` feature が要る（unstable の名のとおり、winit のメジャー更新で
module 名が変わりうる）。

### Control と Command の入れ替えを打ち消す

Slint の winit バックエンドは **Apple プラットフォームで Control と Command を
意図的に入れ替える**。

```rust
// i-slint-backend-winit/winitwindowadapter.rs
// For now: Match Qt's behavior of mapping command to control and control to meta (LWin/RWin).
let swap_cmd_ctrl = i_slint_core::is_apple_platform();
```

自前のショートカットを `Ctrl+C` と書けばどの OS でも動く、という一般的なアプリには妥当な
既定値だが、**キーバインドを記録する用途では有害**。config.toml に書くべきは端末が実際に
受け取る修飾キーであって、慣習で読み替えた名前ではない。放置すると `ctrl+a` の設定が
`cmd+a` として保存され、herdr では永久に発火しない。

```
winit  : ModifiersState(CONTROL)  physical=ControlLeft  logical=Control
Slint  : ctrl=false  meta=true                          ← ここで入れ替わる
```

修飾キーも物理キーと同じ `CustomApplicationHandler` から取る。こちらは入れ替え前の値を
受け取る。

herdr は種別ごとに違う構文規則を持つため、`schema.rs` が各項目を prefix /
action / navigate / indexed / command に分類し、`1..9` レンジを取るかを記録する。
navigate モードのキーは `prefix+` / `esc` / `enter` / `tab` / 左右矢印 /
修飾なし 1〜9 を使えない。

衝突検出は既定値を含む実効設定を横断する。レンジは 9 チョードに展開するので
`prefix+1..9` は手書きの `prefix+3` と衝突する。修飾キーの順序は無視する
（herdr 自身のドキュメントが `ctrl+shift+alt+left` と `alt+shift+left` の
両方を書いている）。navigate モードは別スコープとして扱う。これをしないと
同梱デフォルトだけで 6 件の誤検出が出る。

構文エラー・衝突・端末が届けにくいキーは「キー設定の問題」パネルに深刻な順で
並び、各項目から該当行へのジャンプ・再録音・無効化ができる。

## 配布方針

まだリリースは出していない。`.github/workflows/release.yml` がタグを受けて Linux の `.deb` / `.AppImage`
と Windows の `.exe` を作り、出所を証明して下書きのリリースに付ける。方針は「macOS はビルド済みを
配らない、Windows は SignPath Foundation で署名、Linux はそのまま、成果物は Artifact Attestations で
出所を検証可能にする」。
その根拠、cargo-bundle の形式ごとの外部依存、各 OS の署名手順は
[`.claude/skills/release-signing/SKILL.md`](.claude/skills/release-signing/SKILL.md) にある。
リリース、バンドル生成、署名に触れるときは先にそちらを読むこと。

## ライセンス

本体は **BSD-3-Clause**（`LICENSE`）。著作権者は Members Co., Ltd。

UI の Slint は 3 択ライセンスのうち **Royalty-free 2.0** を選んでいる。本体のライセンスは
縛らないが、アトリビューションの表示が要る。

- **About 画面の `AboutSlint` ウィジェットは表示義務そのものなので、消さない・隠さない**
- Royalty-free 2.0 は、Slint 単体の配布、組込み機器での使用、Slint の API を外部に公開する
  アプリを禁じている。機能を足すときにこれらへ当たらないようにする

依存クレートのライセンス内訳と、表示方法を About に決めた理由は
[`.claude/skills/release-signing/SKILL.md`](.claude/skills/release-signing/SKILL.md) にある。

## 既知の未実装

- `[[keys.command]]` エントリの並べ替え（追加・削除・編集は可能）
- `[ui.sidebar.agents.rows_by_agent]` と `[ui.sound.agents]` への新規キー追加。
  どちらも自由なキーではなく **19 個の正規エージェント ID の集合**で、herdr は
  未知の名前を拒否する。ID の一覧は取得済みで、テストが実機の herdr に問い合わせて
  固定している（`overlay::ROWS_BY_AGENT_IDS` / `SOUND_AGENT_IDS`）が、
  追加する UI はまだない。
  なお herdr は同じエージェントを表に応じて別綴りで呼ぶ
  （rows_by_agent は `opencode` / `copilot`、sound は `open_code` / `github_copilot`）
- `experimental.cjk_ime_agents` は生 TOML 入力。herdr 側に検証がなく任意の文字列が通る
- Linux / Windows は実機で一度も動かしていない。ウィンドウ描画、キー録音
  （winit がどの物理キーを報告するか、日本語配列の記号キー）、バイナリ探索のフォールバック、
  バンドル生成はいずれも未検証。CI でビルドとテストが通ることまでが現状の確認範囲
- まだリリースを出していない。ワークフローは手動実行で、3 つの成果物とその証明まで確認済み
- Windows の exe はコンソールサブシステムでビルドされる（`windows_subsystem = "windows"` がない）ので、
  起動するとアプリの横にコンソールウィンドウが開く。アイコンも既定のまま。どちらもリリース前に直す
- 署名は一切していない。macOS の `.app` はリンカによる ad-hoc 署名のみで、
  `spctl` は通らない（ローカルビルドは quarantine が付かないため動作する）
- `config.toml.bak-<epoch>` を毎回作るが世代管理はしていない
- `src/` の `#[derive(Serialize)]` と `#[derive(Deserialize)]` はどこからも使われていない
  （旧 Tauri 版が JSON でやり取りしていた名残）。外部に出す形式はないので、`serde` 依存ごと外してよい
