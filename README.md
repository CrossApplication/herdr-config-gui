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
設定ファイルの場所は Linux/macOS が `~/.config/herdr/config.toml`、
Windows が `%APPDATA%\herdr\config.toml`。

## 開発

```sh
npm install
npm run tauri dev        # アプリを起動
npm test                 # UI ロジックのテスト
cd src-tauri && cargo test -- --test-threads=1   # スキーマ/書き込みのテスト
```

`HERDR_GUI_DUMP=1` を付けて起動すると、UI に渡す JSON を標準出力に吐いて終了する。
`HERDR_GUI_CONFIG` で編集対象のファイルを差し替えられる（テスト用）。

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
- Linux / Windows 未検証。`herdr` バイナリ解決のフォールバック（PATH を継承しない
  GUI 起動時に `~/.local/bin` や Homebrew を探す経路）も未実行
- 配布まわり未着手（`.icns` / `.ico` 未生成、release ビルド未実施、CI なし）
- `herdr config check` は herdr 自身が解決するパスを検証するため、`HERDR_GUI_CONFIG` で
  別ファイルを編集した場合は実 config を見てしまう
- `config.toml.bak-<epoch>` を毎回作るが世代管理はしていない
- 依存の `glib 0.18.5` に moderate の脆弱性報告があるが、Tauri の Linux バックエンド
  (`gtk 0.18` が `glib = "^0.18"` を要求) 経由のため当リポジトリでは上げられない
