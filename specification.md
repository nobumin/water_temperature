# specification.md — 詳細仕様

DS18B20 温度センサをマイコンボードに接続し、測定温度を BLE でスマートフォンへ
提供するファームウェアの詳細仕様。

## 0. 対応ボード

本リポジトリは **2 系統のボード**に対応する。BLE の提供方式（アドバタイズ Service Data /
GATT ESS）とセンサ処理は共通で、**中核ロジック `core/` は両者で共有**している。

| | Raspberry Pi Pico W | ESP-WROOM-32D |
| --- | --- | --- |
| クレート | `firmware/` | `firmware-esp32/` |
| MCU | RP2040 (Cortex-M0+) | ESP32-D0WD (Xtensa LX6, デュアルコア) |
| BLE | CYW43439（外付けチップ） | **チップ内蔵** |
| DS18B20 データ線 | **GPIO15**（物理20番） | **GPIO4** |
| Rust ターゲット | `thumbv6m-none-eabi` | `xtensa-esp32-none-elf` |
| ツールチェーン | stable | **Espressif フォーク**（`espup`） |
| 無線ファームウェア blob | 必要（`scripts/fetch-cyw43-firmware.*`） | **不要** |
| 書き込み | `elf2uf2-rs` / `probe-rs` | `espflash` |
| ログ出力 | `defmt`（RTT） | `log`（`esp-println`, UART） |

以降、章立ての中で差異がある箇所は両方を併記する。

## 1. システム概要

```
+-----------+ 1-Wire  +----------------+   BLE(2.4GHz)   +-------------+
| DS18B20   |<------->| Raspberry Pi   |  ~~~~~~~~~~~~~>  | Smartphone  |
| (温度)    | GPIO15  | Pico W (RP2040 |                 | (nRF Connect|
|           |         |  + CYW43439)   |                 |  等)        |
+-----------+         +----------------+                 +-------------+
```

- **オンデマンド検温**: 常時測定はせず、スマートフォンからの**検温要求**（GATT Write）を
  受けてから 60 秒間だけ測定する（既定 2 秒周期）。ウィンドウ中に再度要求を受けると延長される。
  詳細は 5.4 節。
- 温度を 2 通りで提供:
  1. **非接続アドバタイズ**: アドバタイズパケットの Service Data に温度を載せる
     （待受中は最後に測定した値）。
  2. **接続型 GATT**: Environmental Sensing Service の Temperature 特性で Read / Notify。

## 2. ハードウェア構成

### 2.1 使用部品

| 部品 | 型番/仕様 |
| --- | --- |
| マイコンボード | Raspberry Pi Pico W（RP2040 + CYW43439、無線必須）<br>または ESP-WROOM-32D（ESP32-D0WD、BT 内蔵） |
| 温度センサ | DS18B20（1-Wire, -55〜+125℃, 9〜12bit） |
| プルアップ抵抗 | 4.7kΩ（データ線 ⇔ 3.3V） |

### 2.2 配線（Pico W）

| DS18B20 ピン | 接続先（Pico W） |
| --- | --- |
| GND | GND（例: 物理 38 番ピン） |
| DQ（データ） | GPIO15（物理 20 番ピン）※4.7kΩ で 3V3 へプルアップ |
| VDD | 3V3(OUT)（物理 36 番ピン） |

```
        3V3(OUT) ----+----[ 4.7kΩ ]----+
                     |                 |
   DS18B20.VDD ------+                 |
   DS18B20.DQ  ---------------------- GPIO15
   DS18B20.GND ------------------------ GND
```

> 寄生電源（VDD を GND に接続）ではなく、**通常電源（VDD=3V3）** を前提とする。

#### Pico W ピン配置図（物理ピン番号）

USB 端子を上に置いた向き。`◄──` が DS18B20 との接続に使う 3 本のピン。

```
                    ┌─────────── USB ───────────┐
          GP0   1 ──┤ ●                       ● ├── 40  VBUS
          GP1   2 ──┤ ●                       ● ├── 39  VSYS
          GND   3 ──┤ ●                       ● ├── 38  GND       ◄── DS18B20 GND
          GP2   4 ──┤ ●                       ● ├── 37  3V3_EN
          GP3   5 ──┤ ●                       ● ├── 36  3V3(OUT)  ◄── DS18B20 VDD ＋ 4.7kΩ 一端
          GP4   6 ──┤ ●        Pico W         ● ├── 35  ADC_VREF
          GP5   7 ──┤ ●                       ● ├── 34  GP28
          GND   8 ──┤ ●                       ● ├── 33  GND
          GP6   9 ──┤ ●                       ● ├── 32  GP27
          GP7  10 ──┤ ●                       ● ├── 31  GP26
          GP8  11 ──┤ ●                       ● ├── 30  RUN
          GP9  12 ──┤ ●                       ● ├── 29  GP22
          GND  13 ──┤ ●                       ● ├── 28  GND
         GP10  14 ──┤ ●                       ● ├── 27  GP21
         GP11  15 ──┤ ●                       ● ├── 26  GP20
         GP12  16 ──┤ ●                       ● ├── 25  GP19
         GP13  17 ──┤ ●                       ● ├── 24  GP18
          GND  18 ──┤ ●                       ● ├── 23  GND
         GP14  19 ──┤ ●                       ● ├── 22  GP17
  ◄──►   GP15  20 ──┤ ●                       ● ├── 21  GP16
                    └───────────────────────────┘
  ◄──► DS18B20 DQ（データ線, GPIO15）＋ 4.7kΩ で 3V3(OUT) へプルアップ
```

まとめ:

| DS18B20 | Pico W ピン | 物理番号 | 位置の目安 |
| --- | --- | --- | --- |
| DQ | GP15 | 20 | 左側の最下段 |
| VDD | 3V3(OUT) | 36 | 右側・上から5番目 |
| GND | GND | 38 | 右側・上から3番目 |

> 4.7kΩ は DQ(GP15) と 3V3(OUT) の間に入れる（DS18B20 の 1-Wire プルアップ）。
> GND は右側 38 番以外の GND ピン（3/8/13/18/23/28/33）でも可。

### 2.3 配線（ESP-WROOM-32D）

| DS18B20 ピン | 接続先（ESP-WROOM-32D） |
| --- | --- |
| GND | GND |
| DQ（データ） | **GPIO4** ※4.7kΩ で 3V3 へプルアップ |
| VDD | 3V3 |

```
        3V3 ---------+----[ 4.7kΩ ]----+
                     |                 |
   DS18B20.VDD ------+                 |
   DS18B20.DQ  ---------------------- GPIO4
   DS18B20.GND ------------------------ GND
```

#### ESP-WROOM-32D ピン配置図

モジュールのアンテナを上に置いた向き（38 ピン版）。`◄──` が DS18B20 との接続に使う 3 本。

```
                    ┌───────── アンテナ ─────────┐
          GND   1 ──┤ ●                       ● ├── 38  GND
          3V3   2 ──┤ ●                       ● ├── 37  GPIO23
           EN   3 ──┤ ●                       ● ├── 36  GPIO22
       GPIO36   4 ──┤ ●                       ● ├── 35  GPIO1  (TXD0)
       GPIO39   5 ──┤ ●    ESP-WROOM-32D      ● ├── 34  GPIO3  (RXD0)
       GPIO34   6 ──┤ ●                       ● ├── 33  GPIO21
       GPIO35   7 ──┤ ●    (入力専用: 34-39)   ● ├── 32  GND
       GPIO32   8 ──┤ ●                       ● ├── 31  GPIO19
       GPIO33   9 ──┤ ●                       ● ├── 30  GPIO18
       GPIO25  10 ──┤ ●                       ● ├── 29  GPIO5
       GPIO26  11 ──┤ ●                       ● ├── 28  GPIO17
       GPIO27  12 ──┤ ●                       ● ├── 27  GPIO16
       GPIO14  13 ──┤ ●                       ● ├── 26  GPIO4   ◄── DS18B20 DQ
       GPIO12  14 ──┤ ●                       ● ├── 25  GPIO0   (ストラッピング)
          GND  15 ──┤ ●                       ● ├── 24  GPIO2   (ストラッピング)
       GPIO13  16 ──┤ ●                       ● ├── 23  GPIO15  (ストラッピング)
     GPIO9(SD) 17 ──┤ ●                       ● ├── 22  GPIO8 (SD)
    GPIO10(SD) 18 ──┤ ●                       ● ├── 21  GPIO7 (SD)
    GPIO11(SD) 19 ──┤ ●                       ● ├── 20  GPIO6 (SD)
                    └───────────────────────────┘
   電源: 2番 3V3 ◄── DS18B20 VDD ＋ 4.7kΩ 一端 / 1番 or 38番 GND ◄── DS18B20 GND
```

> **ピン番号は開発ボードによって異なる**（DevKitC 等ではシルク印刷の GPIO 番号に従うこと）。
> 上図はモジュール単体（ESP-WROOM-32D）のピン番号。**重要なのは GPIO 番号**。

**GPIO 選定上の注意（Pico W との違い）**

- **GPIO15 は使わない**: ESP32 では GPIO15 は**ストラッピングピン（MTDO）**で、
  DS18B20 に必要な 4.7kΩ プルアップが起動時の挙動（ブートログ抑制など）に影響する。
  同様に GPIO0 / GPIO2 / GPIO12 もストラッピングピンのため避ける。
- **GPIO34〜39 は入力専用**（出力ドライブ不可）のため、双方向の 1-Wire には使えない。
- **GPIO6〜11 は内蔵フラッシュ用**のため使用不可。
- 上記を避けた **GPIO4** を既定とする。変更する場合は `firmware-esp32/src/main.rs` の
  `Flex::new(peripherals.GPIO4)` を修正する。

### 2.4 GPIO 割り当て

**Pico W**

| 機能 | GPIO |
| --- | --- |
| DS18B20 データ（1-Wire） | GPIO15 |
| CYW43 PWR | GPIO23（ボード内部） |
| CYW43 CS | GPIO25（ボード内部） |
| CYW43 DIO | GPIO24（ボード内部） |
| CYW43 CLK | GPIO29（ボード内部） |

**ESP-WROOM-32D**

| 機能 | GPIO |
| --- | --- |
| DS18B20 データ（1-Wire） | GPIO4 |
| Bluetooth | チップ内蔵（GPIO 消費なし） |
| ログ出力 (UART0) | GPIO1 (TX) / GPIO3 (RX) |

## 3. ソフトウェア構成

Cargo ワークスペース（`firmware` / `firmware-esp32` は target・ツールチェーンが異なるため
ルートワークスペースから `exclude` し、それぞれ独立ワークスペースとして扱う）。

| クレート | 種別 | 説明 |
| --- | --- | --- |
| `core`（`pico-temp-core`） | `no_std` lib | ハードウェア非依存ロジック。ホストで単体テスト可能。**両ファームで共有**。 |
| `firmware`（`pico-temperature-firmware`） | 組込みバイナリ | Pico W 実機向け（`thumbv6m-none-eabi`）。 |
| `firmware-esp32`（`esp32-temperature-firmware`） | 組込みバイナリ | ESP-WROOM-32D 実機向け（`xtensa-esp32-none-elf`）。 |

`core` は**依存ゼロ**（`defmt` のみ optional）の純粋ロジックなので、ワークスペースが
分かれていても path 依存でバージョン競合を起こさずに共有できる。

### 3.1 `core` クレート

- `ds18b20` モジュール
  - `crc8(&[u8]) -> u8`: Dallas/Maxim 1-Wire CRC8（多項式 0x8C 反射, 初期値 0）。
  - `Scratchpad`: 9 バイトのスクラッチパッド。`temperature()` で CRC 検証と温度変換。
  - `Temperature`: 1/16 ℃ 生値を保持。`centi_celsius()` / `milli_celsius()` / `to_ess_bytes()`。
  - `Ds18b20Error`: `Disconnected`(全 0xFF) / `NoResponse`(全 0x00) / `CrcMismatch`。
- `ess` モジュール
  - `ESS_SERVICE_UUID = 0x181A`, `TEMPERATURE_CHAR_UUID = 0x2A6E`。
  - `advertisement_service_data(Temperature) -> [u8; 6]`: Service Data AD 構造。

### 3.2 `firmware` クレート

- `main.rs`: embassy executor 起動、DS18B20 タスク（検温ウィンドウの状態機械を所有）、
  CYW43 初期化、BLE 起動。
- `onewire.rs`: 1-Wire ビットバンギング DS18B20 ドライバ（`critical_section` でスロット保護）。
- `ble.rs`: `trouble-host` によるアドバタイズ + GATT ESS + 検温制御サービス。
  タスク間連携は `REQUEST`（検温要求）と `READING`（測定完了）の 2 本の `Signal` のみ。

依存の要点（`firmware/Cargo.toml`、Cargo.lock で固定）:
- embassy 各クレートは git rev `1d3c3de` に固定（trouble 公式 rp-pico-w 例と同一）。
- `cyw43` 0.7.0 / `cyw43-pio` 0.10.0（`bluetooth` フィーチャ）。
- `trouble-host`（git, Cargo.lock で rev 固定）。

### 3.3 `firmware-esp32` クレート

- `main.rs`: `#[esp_rtos::main]` で起動 → ヒープ確保 → `esp_rtos::start`（TIMG0 + ソフト割り込み）
  → DS18B20 タスク（GPIO4）→ `BleConnector` → `ExternalController` → BLE 起動。
- `onewire.rs`: 1-Wire ドライバ。ESP32 では GPIO を**オープンドレイン出力＋入力有効**で使い、
  `set_low()` で駆動 / `set_high()` で解放する。手順・タイミングは Pico W 版と同一。
- `ble.rs`: Pico W 版と同一ロジック（GATT ESS・アドバタイズ・Notify）。
  ログのみ `defmt` ではなく `log` を使う。

依存の要点（`firmware-esp32/Cargo.toml`、Cargo.lock で固定）:
- `esp-hal` 1.1 / `esp-radio` 0.18（`ble`）/ `esp-rtos` 0.3（`embassy`）/ `esp-alloc`。
- `trouble-host` は Pico W 版と**同じ git rev `07bf6c0`** に固定し、両ファームで
  同一の BLE API を使う。
- **`[patch.crates-io]` で esp-* を `embassy-rs/esp-hal` rev `b7eec0f` へ差し替える。**
  crates.io 版 `esp-radio` 0.18 は `bt-hci` 0.8 を使うが `trouble-host` は 0.9 を要求するため、
  そのままでは `Controller` トレイトが実装されない。この差し替えで `bt-hci` を 0.9 に揃える
  （trouble 公式 esp32 例と同じ対処）。

#### ESP32 版の依存方針: embassy ファミリは単一 rev に揃える（必須）

`[patch.crates-io]` では **embassy 系クレートを 1 つ残らず同一 rev（`1d3c3de`）へ固定する**。
`embassy-executor` だけを git 化して `embassy-time` 系を crates.io のまま残す、といった
混在をしてはならない。

理由は `embassy-executor-timer-queue` が **2 つに分裂**するため。

- `embassy-executor`（git）→ git 版 `embassy-executor-timer-queue`
- `embassy-time-queue-utils`（crates.io、`esp-rtos` のタイムドライバが使用）
  → crates.io 版 `embassy-executor-timer-queue`

このクレートは integrated timer queue の状態を**タスクヘッダ内**に保持する。分裂すると
タイムドライバが `schedule_wake()` する先と、エグゼキュータが実際にタスクを管理する先が
**別々の状態**になり、**`Timer::after(..).await` が永久に復帰しない**。

厄介なのは、シンボル衝突が起きないため**ビルドもリンクも成功してしまう**点。症状は実機でのみ
現れ、「BLE の広告・接続は正常だが、温度が 0.00 のまま更新されず、`DS18B20` のログが 1 行も
出ない」という形になる（センサ読み取りは変換待ちの `Timer` で停止する）。

再発防止として、CI（`.github/workflows/ci.yml` の `firmware-esp32` ジョブ）で
`Cargo.lock` を検査し、対象クレートが 1 エントリかつ git 由来であることを検証している。

> ESP32 は Wi-Fi/BT スタックが割り込みを多用するため、1-Wire のタイムスロットを
> `critical_section` で保護することが Pico W 以上に重要。それでも読み取りが不安定な場合は、
> RMT ペリフェラルによるハードウェア生成へ置き換える余地がある（「今後の拡張候補」参照）。

#### µs 待機の実装はボード固有（ESP32 では `esp_hal::delay::Delay` を使わない）

1-Wire のプロトコル手順とタイミング定数は両ボードで共通だが、**µs 待機の実装だけは
ボードごとに異なる**。

| ボード | 待機の実装 | 理由 |
| --- | --- | --- |
| Pico W | `embassy_time::block_for` | RP2040 の TIMER は 1µs 分解能で単純に読めるため軽い |
| ESP32 | **`esp_hal::rom::ets_delay_us`** | 下記の通り `Delay` ではオーバーヘッドが大きすぎる |

ESP32 の `esp_hal::delay::Delay` は内部で `Instant::now()` をループで呼ぶが、ESP32 の
`Instant::now()` は **TIMG0 の LACT タイマ**を読む実装で、「更新完了を示すビットが無いため
下位 32bit の値が変化するまでポーリングする」という重い手順を踏む。このため 1 回の呼び出しに
µs オーダーのオーバーヘッドが乗り、`delay_micros(6)` が実際には十数 µs かかる。

1-Wire の読み取りスロットは**立ち下がりから 15µs 以内**にサンプルする必要があるため、
このオーバーヘッドがあるとサンプル点が規定を超え、**センサが出した 0 を 1 として読む**。
実機では `CrcMismatch` の多発と、全ビットが 1 に化けた `Disconnected`（全 0xFF）として現れた。

ROM の `ets_delay_us` は CCOUNT ベースの busy-loop でバスアクセスを伴わない。CPU 周波数変更時に
esp-hal が `ets_update_cpu_frequency_rom()` を呼ぶため、`CpuClock::max()` 設定後も精度が保たれる。

あわせて ESP32 版の読み取りサンプル点は、規定どおりの 15µs（6+9µs）ではなく
**約 11µs（3+8µs）**とし、4µs の余裕を持たせている。

## 4. DS18B20 プロトコル

### 4.1 測定シーケンス

> ESP32 版は起動時に一度だけ **Read ROM（0x33）** を実行し、ROM コード 8 バイトをログへ出力する
> （セルフテスト）。1 バイト目のファミリコードが `0x28` で CRC が合えば、配線と 1-Wire の
> タイミングが正常であることを確定でき、以降のエラーを「変換シーケンス側の問題」と切り分けられる。
> Read ROM はバス上のセンサが 1 個のときのみ有効。

1. リセット → プレゼンス検出
2. Skip ROM（0xCC）
3. Convert T（0x44）
4. 変換待ち（12bit で最大 750ms、非同期待機）
5. リセット → Skip ROM（0xCC）
6. Read Scratchpad（0xBE）
7. 9 バイト読み出し
8. CRC8 検証 → 温度変換

### 4.2 スクラッチパッド構造（9 バイト）

| Byte | 内容 |
| --- | --- |
| 0 | 温度 LSB |
| 1 | 温度 MSB |
| 2-3 | TH / TL |
| 4 | Configuration（分解能: bit6:5） |
| 5-7 | 予約 |
| 8 | CRC8 |

### 4.3 温度変換

- 生値 `raw`（16bit 符号付き）は **1/16 ℃** 単位。`温度[℃] = raw / 16`。
- BLE ESS Temperature（0x2A6E）は `sint16`・**0.01℃（センチ℃）** 単位・リトルエンディアン。
  - `centi = round(raw × 100 / 16)`。
- 1-Wire バス開放時は全 0xFF（`Disconnected`）、無応答時は全 0x00（`NoResponse`）として扱う。

## 5. BLE 仕様

### 5.1 デバイス

- デバイス名: `PicoTemp`
- アドレス: ランダム静的アドレス（テスト用固定値）
- Appearance: Generic Sensor

### 5.2 アドバタイズ（非接続で温度取得）

`ConnectableScannableUndirected` で以下の AD 構造を広告する。

| AD Type | 内容 |
| --- | --- |
| Flags(0x01) | LE General Discoverable, BR/EDR Not Supported |
| Service Data - 16bit UUID(0x16) | UUID=0x181A + 温度2バイト（sint16, センチ℃, LE） |
| Complete Local Name(0x09) | `PicoTemp` |

- アドバタイズ間隔は `ADV_INTERVAL_MS`（既定 1000ms）。trouble-host のデフォルト 160ms は
  発見の速さ優先で電力的に不利なため、待受時間が支配的な本用途では長めにしている。
- 未接続時は `ADV_REFRESH_SECS`（既定 60 秒）ごとにアドバタイズを貼り直す（最新温度を反映）。
- **待受中の Service Data は「最後に測定した値」**であり、ライブの値ではない。
  一度も測定していない場合は 0 を載せる。

### 5.3 GATT（接続で温度取得）

| Service | Characteristic | UUID | プロパティ | 値 |
| --- | --- | --- | --- | --- |
| Environmental Sensing (0x181A) | Temperature | 0x2A6E | Read / Notify | sint16, センチ℃, LE |
| 検温制御（カスタム）<br>`4d454153-0001-4a70-9c2f-3b1d5e7a9c00` | 検温要求<br>`4d454153-0002-4a70-9c2f-3b1d5e7a9c00` | 上記 | Read / Write | uint8（値は不問） |

- Temperature の Notify は**検温中のみ**。測定が完了するたびに送る（固定周期ではない）。
- Read 時は最新の測定値を返す。
- 検温要求 characteristic に**何か書き込むと検温が始まる**（値は問わない）。詳細は 5.4 節。
- ESS の Temperature（0x2A6E）は仕様上 Read/Notify のみのため、そこに `write` を足さず
  別サービスとして制御用 characteristic を設けている。UUID の先頭 4 バイトは ASCII `MEAS`。

### 5.4 オンデマンド検温（動作モデル）

常時測定はしない。ユーザからの**検温要求**を受けてから一定時間だけ測定・送信する。

```
IDLE（待受）── 検温要求 ──► MEASURING（deadline = now + 60s）
  ▲                            │  ・SENSOR_PERIOD_SECS ごとに測定して Notify
  │                            │  ・要求を再受信 → deadline = now + 60s（延長）
  └──── now >= deadline ───────┘
```

| 状態 | 測定 | Notify | アドバタイズ |
| --- | --- | --- | --- |
| IDLE（待受） | しない | しない | する（最終測定値を Service Data に載せる） |
| MEASURING（検温中） | する | する | 接続中のため停止 |

- 延長は**要求を受けた時点から `MEASURE_WINDOW_SECS` 秒**へ取り直す
  （残り時間への加算ではなくスライディング方式）。
- 検温要求は `Signal` で受けるため、測定中（最大 750ms のブロッキング）に届いた要求も
  取りこぼさない。**受信時刻は GATT 受信側で採って `Signal` に載せる**。
  センサタスク側で `Instant::now()` を取ると、測定処理のぶん起点が後ろ倒しになるため。
- `deadline` は測定周期と同時に `select3` で待つ。これにより期限到達から
  測定周期ぶん遅れて停止することがない。
- **1 回の要求につき最低 1 回は測定する**（ウィンドウ判定より前に測定するため）。
- 将来ボタン等のトリガを追加する場合も、`ble::REQUEST` へ合流させれば BLE 側の変更は不要。

#### 接続が切れた場合の振る舞い（意図的な設計）

検温ウィンドウ中に BLE が切断されても、**センサタスクはウィンドウ終了まで測定を継続する**。

- Notify は接続が無いため送られなくなる（`notify_task` が終了する）
- 一方で `TEMP_CENTI` は更新され続けるため、**アドバタイズの Service Data は
  残りウィンドウの間フレッシュな値を保つ**

切断で即キャンセルしない理由は、一瞬の BLE 切断で検温ウィンドウが失われるのを避けるため。
再接続のたびに Write をやり直す必要がなくなる。測定継続のコスト（DS18B20 の変換で約 1mA）は
無線のアイドル電流に比べて無視できる。

> 「待受中は測定しない」という状態モデルは**ウィンドウの有無**で定義しており、
> 接続の有無では定義していない。厳密に「切断＝即 IDLE」としたい場合は、
> BLE 側の `Disconnected` 検出からセンサ側へ中断シグナルを渡す経路の追加が必要。

> **なぜ deep sleep にしないのか**: BLE の原理上、無線を落とすと検温要求を受信できない。
> そのため待受中もコネクタブルアドバタイズを継続する。真の deep sleep（µA）には
> 物理ボタン等の GPIO 起床が必要になる。
>
> 本方式で削減できるのは「測定・Notify の停止」と「アドバタイズ間隔の延長」の分のみで、
> 消費電流の支配要因である CPU と無線のアイドル電流は下がらない。
> **本方式の主目的は電力削減ではなく、要求されたときだけ測るという動作そのもの**である。

## 6. 動作パラメータ（既定値）

| パラメータ | 既定 | 定義箇所 |
| --- | --- | --- |
| 検温ウィンドウ | 60 秒 | `main.rs: MEASURE_WINDOW_SECS` |
| 検温中の測定周期 | 2 秒 | `main.rs: SENSOR_PERIOD_SECS` |
| アドバタイズ間隔 | 1000ms | `ble.rs: ADV_INTERVAL_MS` |
| アドバタイズ貼り直し周期 | 60 秒 | `ble.rs: ADV_REFRESH_SECS` |
| Notify | 測定完了ごと | `ble.rs: notify_task`（`READING` 駆動） |
| 変換待ち | 750ms | `onewire.rs: CONVERSION_TIME_MS` |

## 7. エラー処理

- センサ読み取り失敗時は共有温度を「未測定」（番兵値）にし、アドバタイズには温度 0 を載せる。
- BLE ランナー / Notify のエラーはログ出力し、致命的でない限り継続する。
- CRC 不一致は 1 サイクルを破棄し、次周期で再測定する。

## 8. 今後の拡張候補

- 電池駆動向けの低消費電力化（アドバタイズ間隔延長、スリープ）。
- 複数センサ対応（ROM 検索）。
- 湿度など他の環境センシング特性の追加。
- **`ble.rs` の共通クレート化**: Pico W 版と ESP32 版で BLE ロジックがほぼ同一のため、
  `firmware-common` として抽出し重複を解消する（ログ出力の抽象化が必要）。
- **ESP32 の 1-Wire を RMT ペリフェラルへ移行**: 現在はソフトウェアのビットバンギング。
  RMT を使うとタイムスロットがハードウェア生成となり、無線スタックの割り込みに対して
  より堅牢になる。
- ESP32-C3/C6（RISC-V）対応: stable Rust で扱えるため、ツールチェーン導入が容易になる。
