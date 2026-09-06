# water_temperature
DIY デジタル水温計

Raspberry Pi Pico W または ESP-WROOM-32D に DS18B20 温度センサーを接続して水温を計測し、
Bluetooth Low Energy (BLE) 経由で Android の Chrome ブラウザから水温を確認できます。
釣り場での水温チェックなど、間接的な釣具として利用することを想定しています。

## 構成

| ディレクトリ | 内容 |
| --- | --- |
| `firmware/pico_w/main.py` | Raspberry Pi Pico W 用 MicroPython ファームウェア |
| `firmware/esp32/esp32_ds18b20_ble/` | ESP-WROOM-32D 用 Arduino スケッチ |
| `docs/index.html` | Web Bluetooth API を使った水温表示ページ (Android Chrome 対応) |

両ファームウェアとも同じ BLE GATT UUID (Environmental Sensing サービス `0x181A` /
Temperature キャラクタリスティック `0x2A6E`) を使用しているため、`docs/index.html` は
どちらのボードにもそのまま利用できます。

## 配線 (DS18B20)

- VDD → 3.3V
- GND → GND
- DQ  → データピン (下記) + DQ-VDD 間に 4.7kΩ のプルアップ抵抗

| ボード | DQ 接続ピン |
| --- | --- |
| Raspberry Pi Pico W | GP16 |
| ESP-WROOM-32D | GPIO4 |

## Raspberry Pi Pico W のセットアップ

1. [Raspberry Pi Pico W 用 MicroPython](https://micropython.org/download/RPI_PICO_W/) ファームウェアを書き込みます。
   (`onewire`/`ds18x20` モジュールが同梱されています)
2. `firmware/pico_w/main.py` を Thonny などで Pico W に転送し、`main.py` として保存します。
3. 電源を入れると `WaterTemp` という名前で BLE アドバタイズを開始します。

## ESP-WROOM-32D のセットアップ

1. Arduino IDE に [ESP32 ボードサポート](https://github.com/espressif/arduino-esp32) を追加します。
2. ライブラリマネージャーから `OneWire` と `DallasTemperature` をインストールします
   (ESP32 BLE ライブラリはボードサポートに同梱されています)。
3. `firmware/esp32/esp32_ds18b20_ble/esp32_ds18b20_ble.ino` を開いて書き込みます。
4. 電源を入れると `WaterTemp` という名前で BLE アドバタイズを開始します。

## Android Chrome での確認方法

1. `docs/index.html` を HTTPS (GitHub Pages など) 経由、または `localhost` で配信します。
   (Web Bluetooth はセキュアコンテキストが必須です)
2. Android 端末の Chrome でページを開き、「水温計に接続 (Connect)」ボタンをタップします。
3. 表示されたデバイス一覧から `WaterTemp` を選択すると、水温がリアルタイムに表示されます。
