# Slint 試作（比較用・使い捨て）

現行の Tauri + TypeScript 版と**見た目と手触りを比べるためだけ**のビルド。
`src-tauri/` と `src/` には一切手を触れていない。

## 既存コードの扱い

Rust の中核 5 モジュールは **コピーせず、同じファイルをそのまま読み込んでいる**
（`#[path = "../../src-tauri/src/..."]`）。いずれも Tauri に依存していないため、
そのまま Slint 側でも動く。二重管理は発生しない。

| モジュール | 役割 |
| --- | --- |
| `schema.rs` | `herdr --default-config` の行指向パース |
| `overlay.rs` | 未文書キー、カラートークン、行トークン集合 |
| `check.rs` | `herdr config check` の実行と診断の解析 |
| `config.rs` | `config.toml` の読み書き（最小差分・行末保持） |
| `herdr.rs` | herdr バイナリの探索と実行 |

移植対象は TypeScript の 2,211 行と CSS の 448 行のみ。

## 安全策

**書き込みを一切行わない。** 編集はメモリ上だけで、保存ボタンは差分の表示までしか
しない。試作が実 `config.toml` を触る事故を避けるため。

```sh
cd prototype-slint
cargo run
```
