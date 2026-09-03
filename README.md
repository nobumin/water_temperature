# water_temperature

DS18B20 温度センサの測定値を **BLE（Bluetooth Low Energy）** でスマートフォンへ提供する
ファームウェアです。**Raspberry Pi Pico W と ESP-WROOM-32D の 2 ボードに対応**しています。

- **対応ハードウェア**:
  - Raspberry Pi Pico W (RP2040 + CYW43439) … `firmware/`
  - ESP-WROOM-32D (ESP32-D0WD, BT 内蔵) … `firmware-esp32/`
- **センサ**: DS18B20（1-Wire デジタル温度センサ）
- **言語**: Rust（[embassy](https://embassy.dev/) / `trouble-host` / `cyw43` / `esp-hal`）
- **動作モデル**: **オンデマンド検温**。常時測定はせず、スマホからの検温要求（GATT Write）を
  受けてから 60 秒間だけ測定・送信します。ウィンドウ中に再要求すると、その時点から延長されます。
- **BLE 提供方式**（両ボード共通）:
  1. 非接続アドバタイズ（Service Data: Environmental Sensing 0x181A、待受中は最終測定値）
  2. 接続型 GATT（Environmental Sensing Service + Temperature 特性 0x2A6E）
  3. 検温制御サービス（カスタム UUID、Write で検温を開始／延長）

| | Pico W | ESP-WROOM-32D |
| --- | --- | --- |
| DS18B20 データ線 | GPIO15 | **GPIO4** |
| ツールチェーン | stable | Espressif フォーク（`espup`） |
| 無線 blob | 必要 | **不要**（BT 内蔵） |
| 書き込み | `elf2uf2-rs` / `probe-rs` | `espflash` |

## ブランチ運用

| ブランチ | 役割 |
| --- | --- |
| `main` | 本番 |
| `develop` | 開発統合 |
| 作業ブランチ | 機能実装 → `develop` へ PR |

## リポジトリ構成

| パス | 内容 |
| --- | --- |
| `core/` | ハードウェア非依存ロジック（温度変換・CRC・ESS エンコード, `no_std`, ホストでテスト可）。**両ファームで共有** |
| `firmware/` | Pico W 向け組込みバイナリ（embassy + cyw43 + trouble-host, `thumbv6m-none-eabi`） |
| `firmware-esp32/` | ESP-WROOM-32D 向け組込みバイナリ（esp-hal + esp-radio + trouble-host, `xtensa-esp32-none-elf`） |
| `scripts/` | CYW43 ファームウェア blob 取得スクリプト等（Pico W 用） |
| `.github/workflows/` | CI |

## クイックスタート

```bash
# 中核ロジックのユニットテスト（実機不要・どちらのボードでも共通）
cargo test -p pico-temp-core
```

**Pico W**

```bash
cd firmware && cargo build --release --features skip-cyw43-firmware   # blob 無しでコンパイル確認
```

**ESP-WROOM-32D**（事前に `cargo install espup && espup install --targets esp32`。
インストール後、シェルごとに `export-esp.sh`（Windows は `export-esp.ps1`）の読み込みが必要。
詳細は `environment.md` 2.2 参照）

```bash
cd firmware-esp32 && cargo build --release
```

実機での書き込み・動作確認は `environment.md` と `test_procedure.md` を参照してください。
電池駆動を検討する場合は `power_supply.md` を参照してください。

## ドキュメント

| ファイル | 内容 |
| --- | --- |
| `specification.md` | 詳細仕様 |
| `environment.md` | ローカル環境構築・実機セットアップ手順 |
| `test_procedure.md` | テスト手順 |
| `power_supply.md` | 電源設計メモ（電池選定・接続図・省電力・製品化の検討） |
| `CLAUDE.md` | 開発運用ルール |
