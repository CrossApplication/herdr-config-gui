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
cargo run --release              # 描画が軽いので release 推奨
cargo test -- --test-threads=1   # 共有モジュールが単一スレッドを要求する
PROTO_DUMP=1 cargo run           # ウィンドウを開かずデータだけ確認
```

## 検証済みの論点

| 論点 | 結果 |
| --- | --- |
| 現行のカラーパレット | 値をそのまま移せる |
| 3 状態の行（バー・チップ） | 再現できた |
| セクションサイドバーと件数 | 再現できた |
| **入れ子リスト（行 × トークン）** | Slint の struct は配列フィールドを持てる。`struct RowEntry { tokens: [TokenCell] }` と `for r in rows: for t in r.tokens:` で表現できた |
| ホバー | CSS の `:hover` はなく、`TouchArea` + `has-hover` を明示的に書く |
| カラーピッカー | 標準ウィジェットはないが**自作できた**（下記） |

## カラーピッカー

Slint に標準ウィジェットがないため自作した。構成は次のとおり。

- **SV 平面** — 色相色の矩形に、左から白、下から黒を `@linear-gradient` で重ねる。
  2 枚のグラデーションで彩度・明度平面になる
- **色相帯** — 7 ストップの線形グラデーション
- **十字カーソル / つまみ** — 現在値の位置に描画
- **hex 入力** — 名前付き色や `rgb()` や `reset` も打てるよう自由入力のまま
- **プリセット** — アプリ自身のパレット 12 色

`TouchArea` の `mouse-x` / `mouse-y` を 0〜1 に正規化して Rust へ渡し、
HSV ↔ RGB の変換は Rust 側（`src/color.rs`）で行う。Slint 側に計算を持たせていない。

herdr は `[theme.custom]` の色を一切検証しない（`accent = "notacolor"` は
`config check` を通って黙って無視される）一方、行スタイルの `fg` は
`#rgb` / `#rrggbb` しか受け付けない。ピッカーは常に hex を生成するのでどちらでも有効で、
文字入力で他の記法も使えるようにしてある。

行エディタの `fg` スウォッチからも同じピッカーを開く。書き戻し先は
`PickerTarget::Item` と `PickerTarget::RowFg` で切り替える。

## キー録音

実機の打鍵をログに記録して確認した結果を `src/keys.rs` のテストに固定してある。

### Slint が渡すもの

**論理キー（レイアウトが生成する文字）と修飾キーのフラグのみ。物理キーコードはない。**
Slint 自身のドキュメントが "bindings are based on logical keys ... not the physical
position of a key" と明記している。ブラウザの `KeyboardEvent` は `ev.key`（論理）と
`ev.code`（物理）の両方を渡すので、**この一点だけはブラウザのほうが情報量が多い**。

実測結果（macOS）:

| 押したキー | Slint が渡す内容 | 生成されるチョード |
| --- | --- | --- |
| ctrl+a | `text="a"` ctrl=true | `ctrl+a` |
| cmd+a | `text="a"` meta=true | `cmd+a` |
| shift+7 | `text="&"` shift=true | `ampersand` |
| **option+a** | **`text="å"` alt=true** | **`alt+å`** |
| esc / f12 | `Key.Escape` / `Key.F12` | `esc` / `f12` |

修飾キーは正しく届く。ctrl と cmd が入れ替わるようなことはない。

### 唯一の制約

macOS の option は文字を合成する。`option+a` は `å` として届き、**物理キーがないため
`a` に戻せない**。現行版は `ev.code = "KeyA"` から `alt+a` を復元しているが、Slint では
できず、合成後の文字で記録される。

herdr 自身が「alt は端末や tmux の設定次第で届かない」と警告している領域なので実害は
限定的だが、macOS で alt を含むバインドを録音する場合は現行版のほうが正確。

### 全キーを奪えるか

奪える。ログに `cmd+q` が記録されており、**アプリは終了しなかった**。
`FocusScope` が `accept` を返す限り、OS のショートカットも含めて飲み込む。

### 2 フェーズ

Esc と Enter 自体をバインドできるよう、録音中は全キーを飲み込み、確定後に
Esc = キャンセル / Enter = 確定へ切り替わる。確定後は打鍵を無視するため、
**「録音中 / 録音停止中」を大きく表示する**。これがないと、停止中に押したキーが
反映されず「間違ったキーが出た」ように見える。

## 行エディタについて

`src/rows.ts`（TypeScript・223 行）に相当するものを `src/rows.rs` として書いた。
`toml_edit` が TOML を解釈してくれるため、**手書きトークナイザ約 90 行が不要になり短くなった**。

既定値の往復が TypeScript 版と一致することをテストで固定している。

```
[["state_icon", "machine", "workspace", "tab"], ["agent"]]
  -> 2 行 / 5 トークン -> 同一文字列に再生成
```

スタイルのフラグは OFF のときフィールドごと削除する（herdr にとって省略は
「文脈の既定を維持」であり `false` とは別の値であるため）。現行版と同じ扱い。
