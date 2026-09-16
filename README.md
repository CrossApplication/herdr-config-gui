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
| `src/main.ts` | フォーム描画と保存フロー |
| `src/resizer.ts` | サイドバーのリサイズ |

## 既知の未実装

- keys のキーバインドが実キー入力ではなくテキスト入力
- `[[keys.command]]` の行追加・削除
- `[theme.custom]` など開いた辞書への新規キー追加
- `herdr config check` は herdr 自身が解決するパスを検証するため、`HERDR_GUI_CONFIG` で
  別ファイルを編集した場合は実 config を見てしまう
