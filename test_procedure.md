# test_procedure.md — テスト手順

本プロジェクトの検証は **2 層構成**。
実機非依存部分は自動テスト（ホスト/CI）で完結し、無線・センサ依存部分は実機手順で検証する。

| 層 | 対象 | 実行方法 | 自動化 |
| --- | --- | --- | --- |
| L1 ホスト単体テスト | 温度変換・CRC・ESS エンコード（`core`） | `cargo test` | CI で自動 |
| L2 クロスビルド | firmware が `thumbv6m` で通ること | `cargo build`（skip-cyw43-firmware） | CI で自動 |
| L3 実機 HIL | センサ読み取り・BLE 提供 | 実機書き込み + スマホ確認 | 手動（将来セルフホスト化） |

---

## L1. ホスト単体テスト（実機不要）

### 実行

```bash
cargo test -p pico-temp-core
```

### テスト項目（`core/tests/conversion.rs`）

| 分類 | 内容 |
| --- | --- |
| 温度変換 | +25.0625℃ / 0℃ / -10.125℃ / 範囲端(-55, +125℃) の生値→センチ℃・ミリ℃ |
| スクラッチパッド解析 | 有効読み取り、電源投入時 85℃、分解能ビット |
| エラー検出 | 全 0xFF（Disconnected）/ 全 0x00（NoResponse）/ CRC 不一致 |
| CRC8 | 既知ベクタ（0x1C）、空入力、表ベース参照実装との多数入力クロスチェック |
| ESS/アドバタイズ | UUID 定数、Service Data AD 構造レイアウト |

### 期待結果

全テストが `ok`（現状 15 ケース）。

---

## L2. クロスビルド確認（実機不要）

```bash
cd firmware
cargo fmt --check
cargo clippy --features skip-cyw43-firmware -- -D warnings
cargo build --release --features skip-cyw43-firmware
```

### 期待結果

fmt / clippy 警告なし、`thumbv6m-none-eabi` 向けビルド成功。

---

## CI（自動実行）

`.github/workflows/ci.yml` が push / PR で以下を実行する。

- `core` ジョブ: `cargo fmt --check` / `clippy(core)` / `cargo test(core)`
- `firmware` ジョブ: `cargo fmt --check` / `clippy(firmware)` / `cargo build(firmware)`

PR が緑になることを Copilot レビュー前の前提とする。

---

## L3. 実機 HIL テスト（手動）

前提: `environment.md` に従い書き込み済み。可能なら probe-rs でログを見ながら実施。

### L3-1. 起動・センサ読み取り

1. Pico W を給電。probe-rs 使用時は `cargo run --release` でログ表示。
2. `DS18B20: <n> centi-degC` のログが約 2 秒周期で出ることを確認。
3. **確認**: 室温付近（例 2000〜3000 = 20〜30℃）の妥当な値か。
4. センサを指でつまむ等で温めると値が上昇することを確認。

### L3-2. 異常系（センサ）

1. DS18B20 のデータ線を抜く → `DS18B20 read error` ログ（Disconnected/NoResponse）を確認。
2. 再接続で正常値に復帰することを確認。

### L3-3. BLE アドバタイズ（非接続）

1. スマホの **nRF Connect** 等でスキャン。
2. `PicoTemp` を発見できることを確認。
3. アドバタイズ詳細の **Service Data / UUID 0x181A** に 2 バイトの温度が載ることを確認。
   - リトルエンディアン sint16 / センチ℃。例: `0x09CA` = 2506 = 25.06℃。
4. センサを温めると、次のアドバタイズ更新（約 10 秒以内）で値が変化することを確認。

### L3-4. BLE GATT（接続）

1. nRF Connect で `PicoTemp` に **Connect**。
2. **Environmental Sensing Service (0x181A)** → **Temperature (0x2A6E)** を確認。
3. **Read** で現在温度（センチ℃, LE）を取得。
4. **Notify を有効化**し、約 2 秒周期で値が更新されることを確認。
5. センサ温度を変化させ、Notify 値が追従することを確認。
6. 切断後、再度アドバタイズに戻ることを確認。

### 合否基準

| 項目 | 合格条件 |
| --- | --- |
| センサ | 妥当な室温を示し、加温で上昇 |
| 異常系 | 切断でエラー、再接続で復帰 |
| アドバタイズ | `PicoTemp` を発見、Service Data に温度、更新される |
| GATT | Read/Notify で温度取得、追従、切断後再広告 |

### 記録

実施日・ボード個体・気温・観測値・スクリーンショット（nRF Connect）を PR またはテスト記録に残す。

---

## 将来の自動化（HIL）

- セルフホスト Runner に Pico W + プローブを常設し、`probe-rs run` の defmt ログを
  パースして L3-1/L3-2 を自動判定する構成を想定。
- BLE 側は Runner にホスト BLE アダプタを付け、スキャンで `PicoTemp` の Service Data を
  自動検証する拡張を想定（本 PR では手順化のみ）。
