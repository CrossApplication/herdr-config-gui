# herdr-config-gui

[herdr](https://herdr.dev) の `config.toml` を GUI で編集するデスクトップアプリ。

herdr には組み込みの設定画面がないため、設定変更はエディタで `config.toml` を直接書くことになる。
このアプリは設定項目をフォームとして提供しつつ、ファイルの手書き部分を壊さない。

## インストール

### 共通の前提

必要なのは Rust だけ。UI は Slint の DSL から Rust にコンパイルされるので、
Node.js もシステムの WebView も要らない。Slint 1.18 が Rust **1.92 以上**を要求する。

| OS | 追加で必要なもの |
| --- | --- |
| macOS | Xcode Command Line Tools |
| Linux | `libxkbcommon-dev` `libfontconfig-dev` `libxcb-shape0-dev` `libxcb-xfixes0-dev` `libgl1-mesa-dev` `build-essential` |
| Windows | Visual Studio Build Tools (MSVC) |

### ソースからビルドする

```sh
git clone https://github.com/CrossApplication/herdr-config-gui.git
cd herdr-config-gui
cargo build --release
```

成果物は単一の実行ファイル `target/release/herdr-config-gui`
（Windows は `target\release\herdr-config-gui.exe`）。依存する動的ライブラリは
OS 標準のものだけなので、パスの通った場所に置けばそのまま動く。

アイコンとアプリ名の付いた形式が欲しい場合は [cargo-bundle](https://github.com/burtonageo/cargo-bundle)
を使う。バンドル定義は `Cargo.toml` の `[package.metadata.bundle]` にある。

```sh
cargo install cargo-bundle
cargo bundle --release                 # その OS で作れるものを全部
cargo bundle --release --format osx    # 形式を指定する場合
```

macOS では `target/release/bundle/osx/herdr Config.app` と `dmg/herdr Config.dmg` が出る。
cargo-bundle は `deb` `rpm` `appimage` `msi` にも対応しているが、**このリポジトリで確認したのは
macOS の 2 形式だけ**。

### macOS

**ビルド済みファイルは配布していない。上記の手順でソースからビルドすること。**

理由は[配布方針](#配布方針)に書いた。ローカルでビルドしたものには quarantine 属性が付かないので、
そのまま `/Applications` にコピーして使える。

対応は Apple Silicon (aarch64) のみ。Intel Mac は対象外。

### Linux

Releases の `.deb` または `.AppImage` を使う。署名によるゲートはないのでそのまま動く。

### Windows

Releases の `.msi` を使う。

初回実行時に **SmartScreen の「Windows によって PC が保護されました」** という警告が出る。
これは署名がないため。「詳細情報」→「実行」で進められる。ブロックではなく警告である。

> 現時点ではリリースワークフローが未整備で、Releases にビルド済みファイルは添付されていない。
> ビルドしたい場合は上記のソースビルドを使うこと。

## 配布方針

費用のかかる署名手段は使わない方針のため、OS ごとに扱いを変えている。

### macOS でバイナリを配らない理由

macOS Sequoia (15) で、Control+クリックによる Gatekeeper 回避が削除された。署名も公証もない
アプリをダウンロードした場合、システム設定 → プライバシーとセキュリティ から明示的に許可するか、
次を実行する必要がある。

```sh
xattr -dr com.apple.quarantine "/Applications/herdr Config.app"
```

問題は手間ではなく、**「警告が出たら quarantine を外す」という習慣を配布者が広めることになる**点にある。
この操作は OS のマルウェア対策を無効化するもので、覚えた習慣は他のアプリにも転用される。
Apple がこの回避手段を削ったのは、まさに署名のないアプリを装うマルウェアへの対策だった。

`xattr` の手順を README に書いて済ませるより、ソースからビルドしてもらうほうが誠実だと判断した。
想定利用者は herdr を使っている開発者であり、Rust と Node が入っている可能性も高い。

正規の配布には Developer ID Application 証明書が必要で、Apple Developer Program (年 $99) への
加入が前提になる。これを用意できた時点で方針を変える。cargo-bundle には Tauri のような署名機能が
ないため、`.app` を作ったあとに自分で署名と公証をかけることになる。

```sh
codesign --deep --force --options runtime \
  --sign "Developer ID Application: ..." "herdr Config.app"
xcrun notarytool submit "herdr Config.dmg" --keychain-profile ... --wait
xcrun stapler staple "herdr Config.dmg"
```

### Windows は警告付きで配る

SmartScreen はブロックではなく警告であり、利用者が自分で進める余地がある。
ただし署名がないと更新のたびに評判がリセットされ、警告は出続ける。

無償で解決する手段として [SignPath Foundation](https://signpath.org/) がある。OSS であれば
OV 相当の証明書による署名を無料で提供している。public 化したあとに申請する。

Azure Artifact Signing (旧 Trusted Signing) は月 $9.99 と安価だが、組織の Public Trust 検証に
3 年以上の納税履歴が必要なため、選択肢から外した。

### Linux はそのまま配る

署名によるゲートがないため、追加の対応は不要。

### ビルド成果物の検証 (public 化後)

[GitHub Artifact Attestations](https://docs.github.com/actions/security-for-github-actions/using-artifact-attestations/using-artifact-attestations-to-establish-provenance-for-builds)
を使うと、成果物が**どのコミットからどのワークフローで生成されたか**を Sigstore の署名付きで証明できる。

```yaml
permissions:
  id-token: write
  contents: read
  attestations: write
steps:
  - uses: actions/attest-build-provenance@v3
    with:
      subject-path: target/release/bundle/**/*
```

利用者側の検証はこうなる。

```sh
gh attestation verify "herdr Config_0.1.0_aarch64.dmg" --repo CrossApplication/herdr-config-gui
```

Apple の公証が「Apple が把握している開発者が作り、マルウェアスキャンを通った」ことを示すのに対し、
attestation は「公開されたソースのこのコミットから、公開された CI で作られた」ことを示す。
OSS の文脈では後者のほうが検証可能性が高い。

ただし **GitHub Free / Pro / Team では public リポジトリでのみ利用できる**。
private の間は使えないため、public 化とセットで導入する。

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

バックアップ (`config.toml.bak-<epoch>`) → 最小差分書き込み →
`herdr config check` → `herdr server reload-config`

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
- herdr が受理するのに文書化されていない設定 **6 個**
  （`keys.swap_pane_*`、`keys.copy_mode`、`ui.agent_panel_scope`）

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

ウィンドウを観察できない環境のために、確認用の抜け道を 2 つ残してある。

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

## 構成

単一の Rust クレート。UI は `.slint` で書き、`build.rs` が Rust にコンパイルする。

| ファイル | 役割 |
| --- | --- |
| `ui/app.slint` | 画面の定義（フォーム、行エディタ、カラーピッカー、キー録音、各モーダル） |
| `src/main.rs` | 状態モデル、Slint のモデル構築、全コールバック、保存フロー |
| `src/schema.rs` | `--default-config` の行指向パーサ |
| `src/overlay.rs` | 手書きのオーバーレイ（未文書のキー・カラートークン） |
| `src/check.rs` | `herdr config check` の実行と診断の解析 |
| `src/config.rs` | `config.toml` の読み書き（最小差分） |
| `src/herdr.rs` | herdr バイナリの解決と実行 |
| `src/keys.rs` | キー構文の正規化・検証・衝突検出 |
| `src/physical.rs` | winit の物理キーと入れ替え前の修飾キーの記録 |
| `src/color.rs` | 色値の判定と HSV 変換 |
| `src/rows.rs` | サイドバー行のパース・生成・検証 |
| `src/tabbar.rs` | タブバー右端の項目のパース・生成・検証 |
| `fixtures/default-config.toml` | `herdr --default-config` のスナップショット（CI 用） |

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

## ライセンス

本体は **BSD-3-Clause**（`LICENSE`）。著作権者は Members Co., Ltd。

依存する Rust クレート 570 件（全ターゲット分の合計）のライセンスを確認したところ、
選択の余地なくコピーレフトになるものは 0 件だった。内訳は MIT / Apache-2.0 系が 490 件、
`Unicode-3.0` が 27 件、`Zlib OR Apache-2.0 OR MIT` が 12 件など。`MPL-2.0` は 0 件
（Tauri の WebView 経由で入っていた 4 件は Slint 移行で消えた）。LGPL を**選べる**
ものが 2 件あるが（`r-efi`、`MIT OR Apache-2.0 OR LGPL-2.1-or-later`）、MIT を選べばよい。

### Slint のアトリビューション

Slint の 14 クレートは 3 択ライセンス
（`GPL-3.0-only` / `LicenseRef-Slint-Royalty-free-2.0` / `LicenseRef-Slint-Software-3.0`）で、
このうち **Royalty-free 2.0** を選んでいる。本体のライセンスを縛らないため
BSD-3-Clause のままでよいが、アトリビューションの表示が要る。

表示方法は 2 択で、**アプリ内の About 画面に `AboutSlint` ウィジェットを置く**方を採った
（画面右下の「About」から開く）。バイナリと一緒に移動するので、GitHub Releases 以外の
経路で配布されても義務を満たせる。もう一方の「ダウンロードページにバッジを掲載」は、
macOS をソース配布にする方針と噛み合わず、配布経路ごとに解釈が揺れる。

なお Royalty-free 2.0 は、Slint 単体の配布、組込み機器での使用、Slint の API を
外部に公開するアプリを禁じているが、いずれも本アプリには該当しない。

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
- リリースワークフローが未整備。タグを打っても Releases にビルド済みファイルは付かない
- 署名は一切していない。macOS の `.app` はリンカによる ad-hoc 署名のみで、
  `spctl` は通らない（ローカルビルドは quarantine が付かないため動作する）
- `config.toml.bak-<epoch>` を毎回作るが世代管理はしていない
- 共有モジュールに残る `#[derive(Serialize)]` は Tauri の JSON IPC 用だったもので、
  Slint 版では使っていない。外部に出す形式を持たないので消してよい
