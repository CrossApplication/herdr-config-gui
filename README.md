# herdr-config-gui

[herdr](https://herdr.dev) の `config.toml` を GUI で編集するデスクトップアプリ。

herdr には組み込みの設定画面がないため、設定変更はエディタで `config.toml` を直接書くことになる。
このアプリは設定項目をフォームとして提供しつつ、ファイルの手書き部分を壊さない。

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

herdr の配布に合わせて macOS (x86_64 / aarch64)、Linux (x86_64 / aarch64)、Windows (x86_64)。

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
全部並べるのではなく一部を例示する。その穴を `src-tauri/src/overlay.rs` が埋める。

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

### 行末を保持する

`toml_edit` は文書を描画するとき改行をすべて LF に正規化する。CRLF のファイルを
そのまま書き戻すと触っていない行まで差分になるため、読み込み時に行末を検出して
書き込み時に復元する。Windows でメモ帳が書いた設定ファイルでも 1 項目の変更が
1 行の差分で済む。

### バイナリの探索

GUI は Finder / Explorer から起動するとシェルの PATH を継承しないため、PATH で
見つからない場合は各 OS のインストール先を探す。Windows では `.exe` 付きで
`%LOCALAPPDATA%\Programs\Herdr\bin`、`%USERPROFILE%\.local\bin`、
それと `%USERPROFILE%\.herdr\packages\standalone\releases` 配下の最新
バージョンディレクトリを見る。

## 開発

```sh
npm install
npm run tauri dev        # アプリを起動

npx tsc --noEmit         # 型チェック
npm test                 # UI ロジックのテスト
npm run build            # 本番バンドル

cd src-tauri
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test -- --test-threads=1   # HERDR_CONFIG_PATH を書き換えるテストがあるため単一スレッド
```

`HERDR_GUI_DUMP=1` を付けて起動すると、UI に渡す JSON を標準出力に吐いて終了する。
`HERDR_CONFIG_PATH` で編集対象のファイルを差し替えられる。これは herdr 自身の
環境変数なので、`herdr config check` と `herdr server reload-config` も同じ
ファイルを対象にする。

### CI

GitHub Actions が ubuntu / macOS / windows の 3 OS で上記すべてを回す。
ランナーに herdr は入っていないので、スキーマテストは
`src-tauri/fixtures/default-config.toml`（`herdr --default-config` のスナップショット）
を読む。herdr が存在する環境では `fixture_matches_installed_herdr` が
スナップショットのずれを検出し、いない環境では自分でスキップする。
スナップショットの更新は次のとおり。

```sh
herdr --default-config > src-tauri/fixtures/default-config.toml
```

## 構成

| ファイル | 役割 |
| --- | --- |
| `src-tauri/src/schema.rs` | `--default-config` の行指向パーサ |
| `src-tauri/src/overlay.rs` | 手書きのオーバーレイ（未文書のキー・カラートークン） |
| `src-tauri/src/check.rs` | `herdr config check` の実行と診断の解析 |
| `src-tauri/src/config.rs` | `config.toml` の読み書き（最小差分） |
| `src-tauri/src/herdr.rs` | herdr バイナリの解決と実行 |
| `src/state.ts` | 編集状態モデル（DOM 非依存・テスト対象） |
| `src/keys.ts` | キー構文の正規化・検証・衝突検出（DOM 非依存・テスト対象） |
| `src/capture.ts` | キー録音モーダル |
| `src/problems.ts` | 問題一覧パネル |
| `src/main.ts` | フォーム描画と保存フロー |
| `src/resizer.ts` | サイドバーのリサイズ |
| `src/color.ts` | 色値の判定とスウォッチ変換（DOM 非依存・テスト対象） |
| `src-tauri/fixtures/default-config.toml` | `herdr --default-config` のスナップショット（CI 用） |

## キーバインド

`[keys]` 系の 58 項目は実際のキー入力で設定する。prefix モードのバインドは
prefix キーを実際に押してから目的のキーを押す（ダイアログが設定中の prefix を
認識して `prefix+<chord>` に折り畳む）。Esc と Enter 自体もバインドできるよう、
録音中は全キーを飲み込み、確定後に Esc = キャンセル / Enter = 確定へ切り替わる。

`KeyboardEvent` の読み方は文字種で変える必要がある。

| 対象 | 使う値 | 理由 |
| --- | --- | --- |
| 英字・数字 | `ev.code` | shift で大文字化し、macOS では alt が合成文字に変える |
| 記号 | `ev.key` | 端末が送るのは文字そのもの。`shift+7` は `&` として届き、herdr の名前は `ampersand` |

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

## 既知の未実装

- `[[keys.command]]` エントリの並べ替え（追加・削除・編集は可能）
- `[ui.sidebar.agents.rows_by_agent]` への新規キー追加。任意のキーを受け付ける
  テーブルはここだけで（`[theme.custom]` は 19 個の固定トークン、
  `[ui.sound.agents]` はエージェント名の固定集合）、自由入力の UI が必要
- array 型 5 項目は構造エディタではなく生 TOML 入力。特に
  `ui.sidebar.agents.rows` は「行 × トークン」の二次元配列で手打ちは辛い
- Linux / Windows は CI でビルドとテストが通ることまでしか確認していない。実機での
  ウィンドウ描画、キー録音（WebView2 の `KeyboardEvent`、日本語配列の記号キー）、
  バイナリ探索のフォールバック、インストーラは未検証
- 配布まわり未着手（`.icns` / `.ico` 未生成のため `tauri build` でのバンドルは不可、
  macOS の署名・公証なし）
- `config.toml.bak-<epoch>` を毎回作るが世代管理はしていない
- 依存の `glib 0.18.5` に moderate の脆弱性報告があるが、Tauri の Linux バックエンド
  (`gtk 0.18` が `glib = "^0.18"` を要求) 経由のため当リポジトリでは上げられない
