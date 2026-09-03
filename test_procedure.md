# test_procedure.md — テスト手順

本プロジェクトの検証は **2 層構成**。
実機非依存部分は自動テスト（ホスト/CI）で完結し、無線・センサ依存部分は実機手順で検証する。

| 層 | 対象 | 実行方法 | 自動化 |
| --- | --- | --- | --- |
| L1 ホスト単体テスト | 温度変換・CRC・ESS エンコード（`core`） | `cargo test` | CI で自動 |
| L2 クロスビルド | firmware が `thumbv6m` / `xtensa-esp32` で通ること | `cargo build` | CI で自動 |
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

**Pico W（`thumbv6m-none-eabi`）**

```bash
cd firmware
cargo fmt --check
cargo clippy --features skip-cyw43-firmware -- -D warnings
cargo build --release --features skip-cyw43-firmware
```

**ESP-WROOM-32D（`xtensa-esp32-none-elf`）**

esp ツールチェーン導入済み・環境変数読み込み済みであること（`environment.md` 2.2）。

```bash
cd firmware-esp32
cargo fmt --check
cargo clippy --release -- -D warnings
cargo build --release
```

### 期待結果

fmt / clippy 警告なし、各ターゲット向けビルド成功。

---

## CI（自動実行）

`.github/workflows/ci.yml` が push / PR で以下を実行する。

- `core` ジョブ: `cargo fmt --all --check` / `cargo clippy -p pico-temp-core --all-targets -- -D warnings`
  / `cargo test -p pico-temp-core`
- `firmware` ジョブ: `cargo fmt --check` /
  `cargo clippy --features skip-cyw43-firmware -- -D warnings` /
  `cargo build --release --features skip-cyw43-firmware`（`thumbv6m-none-eabi`）
- `firmware-esp32` ジョブ: Espressif ツールチェーン導入 → `cargo fmt --check` /
  `cargo clippy --release -- -D warnings` / `cargo build --release`（`xtensa-esp32-none-elf`）

PR が緑になることを Copilot レビュー前の前提とする。

---

## L3. 実機 HIL テスト（手動）

前提: `environment.md` に従い書き込み済み。ログを見ながら実施する。

- **Pico W**: probe-rs 使用時は `cd firmware && cargo run --release`（defmt ログ）
- **ESP-WROOM-32D**: `cd firmware-esp32 && cargo run --release`
  （`espflash flash --monitor` により書き込み後そのままログ表示）

**ボード差分の要点**: BLE の見え方（デバイス名 `PicoTemp`、Service Data、GATT）は両ボードで
同一。異なるのは **DS18B20 のデータ線（Pico W = GPIO15 / ESP32 = GPIO4）** とログ出力方法のみ。

> **本ファームはオンデマンド検温方式**です。検温要求を出すまで測定は始まりません。
> 「起動しただけでは温度ログが出ない」のは**正常動作**です（仕様は `specification.md` 5.4 節）。

### L3-1. 起動・待受状態の確認

1. ボードを給電（上記コマンドでログ表示）。
2. **ESP32 のみ**: 起動直後に `DS18B20 ROM = [28, ..] (family code OK)` が 1 回出ることを確認。
   - Read ROM セルフテスト。これが通れば配線と 1-Wire タイミングは正常と確定できる。
   - 失敗する場合は先へ進まず、配線・プルアップを見直す。
3. **待受中は `DS18B20: <n> centi-degC` が出ないこと**を確認（これが正常）。
4. `BLE address = ...` が出て BLE が起動していることを確認。

### L3-2. 検温要求 → 60 秒間の測定（本機能の中核）

> nRF Connect の代わりに **`webapp/index.html`（Web UI）** でも同じ操作ができます。
> その場合は手順 1〜3 をまとめて「温度を取得」ボタン 1 つで実行できます（`environment.md` 6.1）。

1. nRF Connect で `PicoTemp` に **Connect**。
2. **Environmental Sensing Service (0x181A)** → **Temperature (0x2A6E)** の **Notify を有効化**。
3. **検温制御サービス `4d454153-0001-...`** → **検温要求 `4d454153-0002-...`** に
   **任意の 1 バイトを Write**（例: `0x01`。値は問わない）。
4. **確認**:
   - ログに `[gatt] 検温要求を受信` → `検温開始: 60 秒間 測定します` が出る
   - `DS18B20: <n> centi-degC` が約 2 秒周期で出始める
   - nRF Connect の Temperature が同じ周期で更新される
   - 値が室温付近（例 2000〜3000 = 20〜30℃）で妥当か
5. センサを指でつまむ等で温めると値が上昇することを確認。

### L3-3. 60 秒での自動停止

1. L3-2 の状態から**何もせず 60 秒待つ**。
2. **確認**:
   - ログに `検温終了: 待受へ戻ります` が出る
   - `DS18B20:` の行が止まる
   - Notify が来なくなる（接続は維持されたまま）

### L3-4. 検温ウィンドウの延長

1. 再度 Write して検温を開始する。
2. **60 秒経過前**（例: 30 秒後）にもう一度 Write する。
3. **確認**:
   - ログに `検温要求を再受信: ウィンドウを延長しました` が出る
   - **2 回目の Write から 60 秒間**測定が続く（合計で約 90 秒になる）
   - 最初の Write から 60 秒の時点では止まらない

### L3-5. 異常系（センサ）

1. 検温中に DS18B20 のデータ線を抜く → `DS18B20 read error` ログ
   （Disconnected/NoResponse）と `[1-Wire] scratchpad raw = [..]` を確認。
2. 再接続で正常値に復帰することを確認。

### L3-6. BLE アドバタイズ（非接続）

1. 切断後、nRF Connect でスキャンし `PicoTemp` を発見できることを確認。
2. アドバタイズ詳細の **Service Data / UUID 0x181A** に 2 バイトの温度が載ることを確認。
   - リトルエンディアン sint16 / センチ℃。例: `0x09CA` = 2506 = 25.06℃。
   - **待受中の値は「最後に測定した値」**であり、ライブの値ではない。
     一度も測定していない状態では 0 が載る。

### 合否基準

| 項目 | 合格条件 |
| --- | --- |
| セルフテスト（ESP32） | 起動時の Read ROM でファミリコード `0x28` を取得できる |
| 待受 | 起動しただけでは測定ログ・Notify が出ない |
| 検温開始 | Write を受けて `検温開始` ログが出て、約 2 秒周期で測定・Notify が始まる |
| センサ | 妥当な室温を示し、加温で上昇 |
| 自動停止 | 約 60 秒で `検温終了` ログが出て、測定・Notify が止まる |
| 延長 | ウィンドウ中の Write でその時点から 60 秒へ延長される |
| 異常系 | 線を抜くとエラー、再接続で復帰 |
| アドバタイズ | `PicoTemp` を発見、Service Data に最終測定値が載る |

### 記録

実施日・ボード個体・気温・観測値・スクリーンショット（nRF Connect）を PR またはテスト記録に残す。

---

## 将来の自動化（HIL）

- セルフホスト Runner に Pico W + プローブを常設し、`probe-rs run` の defmt ログを
  パースして L3-1/L3-2 を自動判定する構成を想定。
- BLE 側は Runner にホスト BLE アダプタを付け、スキャンで `PicoTemp` の Service Data を
  自動検証する拡張を想定（本 PR では手順化のみ）。
