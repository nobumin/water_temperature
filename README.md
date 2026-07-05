# water_temperature

Raspberry Pi Pico W と DS18B20 温度センサを用いて、測定温度を **BLE（Bluetooth Low Energy）**
でスマートフォンへ提供するファームウェアです。

- **ハードウェア**: Raspberry Pi Pico W (RP2040 + CYW43439)
- **センサ**: DS18B20（1-Wire デジタル温度センサ）
- **言語**: Rust（[embassy](https://embassy.dev/) / `cyw43` / `trouble-host`）
- **BLE 提供方式**:
  1. 非接続アドバタイズ（Service Data: Environmental Sensing 0x181A）
  2. 接続型 GATT（Environmental Sensing Service + Temperature 特性 0x2A6E）

## ブランチ運用

| ブランチ | 役割 |
| --- | --- |
| `main` | 本番 |
| `develop` | 開発統合 |
| 作業ブランチ | 機能実装 → `develop` へ PR |

## ドキュメント

| ファイル | 内容 |
| --- | --- |
| `specification.md` | 詳細仕様 |
| `environment.md` | ローカル環境構築・実機セットアップ手順 |
| `test_procedure.md` | テスト手順 |
| `CLAUDE.md` | 開発運用ルール |

詳細は開発ブランチ（`develop`）を参照してください。
