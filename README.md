# herdr-config-gui

[herdr](https://herdr.dev) の設定を GUI で編集するアプリです。

herdr に設定画面はありません。何か変えたければ `config.toml` をエディタで開いて手で書くことになります。

`herdr --default-config` が吐く既定の設定は 374 行あります。そのうち値が書かれている行は
`pane_history = false` の 1 行だけで、残りはコメントアウトされた設定と、その説明文。
どれが今効いているのかを拾うところから始まります。

そこをフォームにしました。手で書いた部分には触りません。

## できること

起動すると herdr に設定項目を聞きに行き、その場でフォームを組み立てます。

- キーバインドは実際にキーを押して設定できます
- 色はカラーピッカーで選べます
- サイドバーの行やタブバーの項目は、生の TOML ではなく構造として編集できます
- 保存する前に、書き込まれる差分と herdr の検証結果を確認できます
- 触っていない設定、コメント、空行、改行コードはそのまま残ります

herdr 0.9.0 で 25 セクション / 193 項目。

### 設定項目はアプリに持っていません

起動のたびに `herdr --default-config` を実行して、その出力からフォームを作ります。
herdr を新しくすれば、このアプリを更新しなくても新しい項目が増えます。
裏を返すと、herdr が見つからなければ何も表示できません。

画面の上に、今読んでいる herdr のバージョンと編集対象のファイルが出ます。

### 設定には 3 つの状態があります

ここが少し独特で、慣れるまで戸惑うかもしれません。

| 画面の表示 | `config.toml` |
| --- | --- |
| `既定` | 行なし |
| `設定済み` | その行だけ書かれる |
| `無効` | `key = ""` |

「無効」は herdr が用意している off スイッチで、既定に戻すのとは別物です。
既定に戻せば行そのものが消えます。無効にすれば `key = ""` が書かれます。
off スイッチを持つ設定にだけ「無効にする」ボタンが出ます。

保存前の変更は `未保存` になり、行の左端に色が付きます。

### キーバインドは打鍵で設定します

「キーを録音」を押して、割り当てたいキーをそのまま押してください。prefix モードのバインドなら、
prefix キーを押してから目的のキーを押せば `prefix+<キー>` にまとまります。

Esc と Enter 自体も割り当てられるように、録音中はすべてのキーを飲み込みます。
確定すると Esc がキャンセル、Enter が確定に戻ります。この切り替わりが見えるよう
「● 録音中」「■ 録音停止中」を大きく出しています。打鍵が入らないときは、たいていここです。

構文エラー、他のキーとの衝突、端末が届けにくいキーは、画面下部の「キー設定の問題」に集まります。
そこから該当の設定へ飛んで、録音し直せます。

### 保存の前に herdr へ確かめます

herdr は型や構文のエラーを見つけると、設定ファイルを丸ごと捨てて既定値に戻します。
一部が無視されるのではなく、全部です。これが怖いので、書く前に確かめています。

```
差分の算出 → herdr config check（一時ファイル） → バックアップ → 書き込み → 再読込
```

致命的な内容なら保存を中止します。このときディスク上の `config.toml` には指一本触れていません。
書き込むときは `config.toml.bak-<エポック秒>` を残します。

## 動作環境

| | |
| --- | --- |
| herdr | 0.9.0 で確認 |
| macOS | Apple Silicon (aarch64) のみ。Intel Mac は対象外 |
| Linux | CI でビルドとテストが通るところまで。実機では未確認 |
| Windows | 同上 |

先に断っておくと、開発は macOS だけでやっています。Linux と Windows は GitHub Actions で
ビルドとテストが通るのを見ただけで、実機でウィンドウが開くかどうかも試していません。

herdr が入っていることが前提です。設定項目の取得にも保存時の検証にも herdr を呼びます。

## インストール

ビルド済みのファイルは配っていません。署名のないアプリを配ると「警告が出たら回避する」という
習慣を広めることになるので、やめておきました。判断の経緯は
[AGENTS.md の配布方針](AGENTS.md#配布方針)に書いてあります。

### 必要なもの

Rust だけです。UI も Slint の DSL から Rust にコンパイルされるので、Node.js もシステムの
WebView も要りません。ただし Slint 1.18 が Rust 1.92 以上を要求します。

| OS | 追加で必要なもの |
| --- | --- |
| macOS | Xcode Command Line Tools |
| Linux | `libxkbcommon-dev` `libfontconfig-dev` `libxcb-shape0-dev` `libxcb-xfixes0-dev` `libgl1-mesa-dev` `build-essential` |
| Windows | Visual Studio Build Tools (MSVC) |

### ビルドする

```sh
git clone https://github.com/CrossApplication/herdr-config-gui.git
cd herdr-config-gui
cargo build --release
```

出てくるのは実行ファイル 1 つ、`target/release/herdr-config-gui` です
（Windows なら `target\release\herdr-config-gui.exe`）。リンクしている動的ライブラリは
OS 標準のものだけなので、`~/.local/bin` あたりに置けばそのまま動きます。

```sh
./target/release/herdr-config-gui
```

### アイコンとアプリ名を付ける

[cargo-bundle](https://github.com/burtonageo/cargo-bundle) を使います。

```sh
cargo install cargo-bundle
cargo bundle --release
```

macOS なら `target/release/bundle/` の下に `osx/herdr Config.app` と
`dmg/herdr Config.dmg` が出ます。`.app` は `/Applications` にコピーするだけです。
手元でビルドしたものに quarantine 属性は付かないので、Gatekeeper の警告も出ません。

`deb` `rpm` `appimage` `msi` も作れることになっていますが、こちらで通したのは macOS の
2 形式だけです。

## 使い方

### 編集されるファイル

herdr 自身が解決したパスを使います。`herdr --help` が出力するものをそのまま読むので、
`HERDR_CONFIG_PATH` や `XDG_CONFIG_HOME` の設定もそのまま効きます。

| OS | 既定の場所 |
| --- | --- |
| macOS / Linux | `~/.config/herdr/config.toml` |
| Windows | `%APPDATA%\herdr\config.toml` |

別のファイルを触りたいときは `HERDR_CONFIG_PATH` を指定して起動してください。herdr 自身の
環境変数なので、検証も再読込も同じファイルに対して行われます。

```sh
HERDR_CONFIG_PATH=/path/to/config.toml herdr-config-gui
```

### 画面の流れ

1. 左のサイドバーでセクションを選びます。名前の横に項目数が出ます。
   設定済みのものがあれば「設定済み / 全体」の形になります
2. 上の検索欄で、キー名と説明文をまたいで絞り込めます
3. 値を変えると行に色が付き、下の保存ボタンが押せるようになります
4. 「差分を確認」で、書き込まれる内容と herdr の検証結果を見られます
5. 「保存して反映」で書き込み、動いている herdr サーバーに再読込をかけます

変更がなければ保存ボタンは押せません。サイドバーの幅は境界をドラッグして変えられます。

### うまく動かないとき

| 症状 | 見るところ |
| --- | --- |
| 設定項目が出ない | herdr が PATH にあるか確認してください。画面上部にバージョンが出ていれば見つかっています |
| 保存が中止される | 差分モーダルに herdr の指摘が出ています。中身はディスクに届いていません |
| 押したキーが記録されない | 「録音中」になっているか確認してください。確定後の打鍵は無視されます |

ウィンドウを開かずに状態だけ見たいときは、環境変数で済ませられます。

| 環境変数 | 動作 |
| --- | --- |
| `HERDR_GUI_DUMP=1` | 読み込んだ設定の中身を標準出力に吐いて終了 |
| `HERDR_GUI_TRACE=1` | ウィンドウの実寸と可視状態を報告して終了 |
| `HERDR_GUI_FILTER=<語>` | 検索欄に語を入れた状態で開く |

## まだできないこと

Linux と Windows を実機で動かしていないのが一番大きいところです。ウィンドウの描画、キー録音、
herdr バイナリの探索、バンドル生成、どれも試せていません。

リリースワークフローも用意していないので、タグを打っても Releases にファイルは付きません。

設定のうち次のものは、まだ生の TOML 入力のままです。

- `experimental.cjk_ime_agents`
- `[ui.sidebar.agents.rows_by_agent]` と `[ui.sound.agents]` への新規キー追加
  （すでにあるキーは編集できます）

`[[keys.command]]` のエントリは追加も削除も編集もできますが、並べ替えだけできません。
バックアップ `config.toml.bak-<エポック秒>` は毎回残りますが、古いものを消す仕組みはありません。

### 手を貸してもらえると助かります

一番ありがたいのは Linux か Windows の実機での動作報告です。開発環境が macOS しかなく、
この 2 つは CI が緑になるのを見ているだけで、起動したら何が起きるのか分かっていません。

報告の材料として、ウィンドウを開かずに状態を吐く環境変数を用意してあります。

```sh
HERDR_GUI_DUMP=1 herdr-config-gui    # 設定が読めているか
HERDR_GUI_TRACE=1 herdr-config-gui   # ウィンドウが実際に開いたか
```

知りたいのはこのあたりです。

- ウィンドウが開くか、文字が潰れていないか
- キーを録音したとき、押したキーがそのまま記録されるか
  （日本語配列の記号キーと、Windows の Alt まわりが特に怪しいと思っています）
- herdr を PATH の外に置いても見つけられるか
- `cargo bundle` が通るか、できた `.deb` や `.msi` が実際に入るか

うまくいかなかった報告のほうが価値があります。「動きませんでした」の一行でも助かります。
Issue を立ててください。

設定項目まわりに手を入れる場合は、先に [AGENTS.md](AGENTS.md) を読んでもらえると早いです。
herdr のどの仕様をどう調べて、なぜその形になっているかを書いてあります。

## ライセンス

本体は BSD-3-Clause（[LICENSE](LICENSE)）です。著作権者は Members Co., Ltd。

UI に [Slint](https://slint.dev) を Royalty-free 2.0 で使っています。表示義務があるので、
画面右下の「About」に Slint のアトリビューションを置いています。

依存ライブラリの内訳は [AGENTS.md](AGENTS.md#ライセンス) にあります。

## 開発者向け

設計の理由、herdr の仕様をどう調べたか、CI、配布と署名の方針は
[AGENTS.md](AGENTS.md) にまとめてあります。
