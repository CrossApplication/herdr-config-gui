---
name: release-signing
description: herdr-config-gui のリリース、バンドル生成、コード署名・公証、Artifact Attestations、依存ライセンス確認の方針と手順。リリースワークフローを作る、タグを打つ、cargo bundle の形式を選ぶ、macOS / Windows / Linux の署名や SignPath Foundation への申請を検討する、依存クレートのライセンスを見直すときに読む。
---

# リリースと署名

AGENTS.md から移した、リリース作業のときだけ必要な記述。

## 配布方針

**まだリリースは出していない。** `.github/workflows/release.yml` がタグを受けて Linux の `.deb` /
`.AppImage` と Windows の `.exe` を作り、出所を証明して下書きのリリースに付ける。公開は人が行う。
手動で実行すると下書きを作らずに成果物と証明だけ作るので、タグを打つ前に確かめられる。
タグと `Cargo.toml` のバージョンが食い違うと止まる。

費用のかかる署名手段は使わない前提で、OS ごとに扱いを変える。

cargo-bundle 0.12.0 の形式ごとの外部依存と、このリポジトリで実際に試した結果は次のとおり。

| 形式 | 外部依存 | 結果 |
| --- | --- | --- |
| `deb` `appimage` | なし（純 Rust） | CI の ubuntu で通る |
| `osx` / `dmg` | `install_name_tool` / `hdiutil` | 手元の macOS で通る（配布はしない） |
| `msi` | なし | **使えない。** ファイル名を MSI の識別子に使うが、識別子は `-` を含められず、`herdr-config-gui.exe` で止まる |
| `exe` | なし | **使えない。** 既存の PE リソースを列挙してからアイコンを足す作りで、リソースを持たないこのバイナリでは `ERROR_RESOURCE_DATA_NOT_FOUND` で止まる |
| `rpm` / `wxsmsi` | なし / `dotnet` + WiX | 試していない |

そのため Windows だけは cargo-bundle を使わず、`cargo build --release` の exe をそのまま配る。
システム標準以外の DLL を必要としないので、それ 1 つで配布物として完結する。

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
想定利用者は herdr を使っている開発者であり、Rust が入っている可能性も高い。

正規の配布には Developer ID Application 証明書が必要で、Apple Developer Program (年 $99) への
加入が前提になる。これを用意できた時点で方針を変える。

**署名は cargo-bundle に組み込まれている。** `[package.metadata.bundle]` に鍵を渡すと
`apple-codesign` で署名まで済む。

```toml
[package.metadata.bundle]
apple_signing_p12 = "certs/apple.p12"
apple_signing_password_env = "APPLE_SIGNING_PASSWORD"
apple_signing_hardened_runtime = true
```

組み込まれていないのは**公証**のほうで、これは別途かける。

```sh
xcrun notarytool submit "herdr Config.dmg" --keychain-profile ... --wait
xcrun stapler staple "herdr Config.dmg"
```

### Windows は警告付きで配る

SmartScreen はブロックではなく警告であり、利用者が自分で進める余地がある。
ただし署名がないと更新のたびに評判がリセットされ、警告は出続ける。

無償で解決する手段として [SignPath Foundation](https://signpath.org/) がある。OSS であれば
証明書による署名を無料で提供している。[利用条件](https://signpath.org/terms)は次のとおり。

- OSI 承認の OSS ライセンスで、商用デュアルライセンスでないこと（BSD-3-Clause は該当）
- プロプライエタリな構成要素を含まないこと
- 積極的にメンテナンスされていること
- **「署名したい形ですでにリリース済み」であること** — 申請の前にリリースを作る必要がある
- チーム全員が SignPath とリポジトリの両方で MFA を使うこと
- Author / Reviewer / Approver の役割を分けること
- プロジェクトのホームページに **code signing policy を掲載**すること
- バイナリのメタデータ（製品名・バージョン）が一貫していること

Azure Artifact Signing (旧 Trusted Signing) は月 $9.99 と安価だが、組織の Public Trust 検証に
3 年以上の納税履歴が必要なため、選択肢から外した。

なお cargo-bundle 自身にも Windows Authenticode 署名があるが、`windows-signing` feature を
有効にしたビルドが要る。この feature は GPL-3.0-or-later の osslsigncode を取り込むため、
cargo-bundle を自前でビルドし直すことになる（ビルドツールなので本体のライセンスには波及しない）。
SignPath を使う場合は署名が向こう側で行われるので、この feature は不要。

### Linux はそのまま配る

署名によるゲートがないため、追加の対応は不要。

ただし cargo-bundle には Sigstore による署名がある。GitHub Actions の OIDC トークンを渡すと、
成果物ごとに `*.sigstore.json` を並べて出力する。証明書も費用も要らない。

```toml
[package.metadata.bundle.linux_signing]
identity_token_env = "SIGSTORE_ID_TOKEN"
```

### ビルド成果物の検証

[GitHub Artifact Attestations](https://docs.github.com/actions/security-for-github-actions/using-artifact-attestations/using-artifact-attestations-to-establish-provenance-for-builds)
を使うと、成果物が**どのコミットからどのワークフローで生成されたか**を Sigstore の署名付きで証明できる。

```yaml
permissions:
  id-token: write
  contents: read
  attestations: write
steps:
  - uses: actions/attest-build-provenance@v4
    with:
      subject-path: dist/*
```

利用者側の検証はこうなる。

```sh
gh attestation verify herdr-config-gui_0.1.0_amd64.deb --repo CrossApplication/herdr-config-gui
```

Apple の公証が「Apple が把握している開発者が作り、マルウェアスキャンを通った」ことを示すのに対し、
attestation は「公開されたソースのこのコミットから、公開された CI で作られた」ことを示す。
OSS の文脈では後者のほうが検証可能性が高い。

リポジトリは public なので、証明は Sigstore の Public Good Instance で署名され、公開の透明性ログに
載る。誰でも `gh attestation verify` で確かめられる。手動実行で作った 3 つの成果物はすべて、作った
コミットとワークフローまで一致して検証が通った。1 バイト足したファイルは検証で拒否される。

## 依存クレートのライセンス内訳

依存する Rust クレート 570 件（全ターゲット分の合計）のライセンスを確認したところ、
選択の余地なくコピーレフトになるものは 0 件だった。内訳は MIT / Apache-2.0 系が 490 件、
`Unicode-3.0` が 27 件、`Zlib OR Apache-2.0 OR MIT` が 12 件など。`MPL-2.0` は 0 件
（Tauri の WebView 経由で入っていた 4 件は Slint 移行で消えた）。LGPL を**選べる**
ものが 2 件あるが（`r-efi`、`MIT OR Apache-2.0 OR LGPL-2.1-or-later`）、MIT を選べばよい。

## Slint の表示方法を About にした理由

表示方法は 2 択で、**アプリ内の About 画面に `AboutSlint` ウィジェットを置く**方を採った
（画面右下の「About」から開く）。バイナリと一緒に移動するので、GitHub Releases 以外の
経路で配布されても義務を満たせる。もう一方の「ダウンロードページにバッジを掲載」は、
macOS をソース配布にする方針と噛み合わず、配布経路ごとに解釈が揺れる。
