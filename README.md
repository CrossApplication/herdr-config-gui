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
| `src-tauri/src/config.rs` | `config.toml` の読み書き（最小差分） |
| `src-tauri/src/herdr.rs` | herdr バイナリの解決と実行 |
| `src/state.ts` | 編集状態モデル（DOM 非依存・テスト対象） |
| `src/keys.ts` | キー構文の正規化・検証・衝突検出（DOM 非依存・テスト対象） |
| `src/capture.ts` | キー録音モーダル |
| `src/problems.ts` | 問題一覧パネル |
| `src/main.ts` | フォーム描画と保存フロー |
| `src/resizer.ts` | サイドバーのリサイズ |
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

- `[[keys.command]]` の行追加・削除（単一エントリの項目列挙までは可能）
- `[theme.custom]` / `[ui.sound.agents]` など開いた辞書への新規キー追加
- array 型 5 項目はカラーピッカーや構造エディタではなく生 TOML 入力
- Linux / Windows は CI でビルドとテストが通ることまでしか確認していない。実機での
  ウィンドウ描画、キー録音（WebView2 の `KeyboardEvent`、日本語配列の記号キー）、
  バイナリ探索のフォールバック、インストーラは未検証
- 配布まわり未着手（`.icns` / `.ico` 未生成のため `tauri build` でのバンドルは不可、
  macOS の署名・公証なし）
- `config.toml.bak-<epoch>` を毎回作るが世代管理はしていない
- 依存の `glib 0.18.5` に moderate の脆弱性報告があるが、Tauri の Linux バックエンド
  (`gtk 0.18` が `glib = "^0.18"` を要求) 経由のため当リポジトリでは上げられない
