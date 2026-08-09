# environment.md — ローカル環境構築・実機セットアップ手順

Raspberry Pi Pico W + DS18B20 のファームウェアを、ローカルでビルド・書き込み・検証する
ための手順書。対象 OS は Windows を主とし、必要に応じ macOS/Linux も補足する。

## 0. ローカル作業ディレクトリ

本プロジェクトのクローン先は以下の配下に限定する（他のローカルファイルは触れない）。

```
D:\work\dev
```

### クローン手順（Windows PowerShell）

```powershell
# 作業フォルダへ移動（無ければ作成）
New-Item -ItemType Directory -Force "D:\work\dev"
Set-Location "D:\work\dev"

# クローン（このディレクトリ配下に water_temperature が作成される）
git clone https://github.com/nobumin/water_temperature.git
Set-Location .\water_temperature
```

## 1. 必要なもの（ハードウェア）

**ボードは 2 系統から選べます**（`firmware/` = Pico W、`firmware-esp32/` = ESP-WROOM-32D）。

| 品目 | 備考 |
| --- | --- |
| Raspberry Pi Pico W **または** ESP-WROOM-32D | Pico W は無線チップ必須（無印 Pico 不可）。ESP32 は BT 内蔵 |
| DS18B20 | 1-Wire 温度センサ |
| 4.7kΩ 抵抗 | データ線プルアップ |
| ブレッドボード・ジャンパ線 | 配線用 |
| USB ケーブル | 書き込み・給電（Pico W: micro-B / ESP32 ボード: micro-B or USB-C） |
| （任意）デバッグプローブ | Pico W 用。Raspberry Pi Debug Probe 等。probe-rs でのログ表示に使用 |

### 配線（`specification.md` 参照）

**Pico W**

| DS18B20 | Pico W |
| --- | --- |
| VDD | 3V3(OUT)（物理36） |
| DQ | GPIO15（物理20）＋ 4.7kΩ で 3V3 へ |
| GND | GND（物理38 など） |

**ESP-WROOM-32D**

| DS18B20 | ESP-WROOM-32D |
| --- | --- |
| VDD | 3V3 |
| DQ | **GPIO4** ＋ 4.7kΩ で 3V3 へ |
| GND | GND |

> **ESP32 では GPIO15 を使いません。** ストラッピングピン(MTDO)のため、DS18B20 の
> プルアップが起動時挙動に影響します（GPIO0/2/12 も同様に回避、GPIO34〜39 は入力専用で不可）。
>
> ピンの物理位置がわかる **ピン配置図**（Pico W / ESP-WROOM-32D 両方）は
> `specification.md` の「2.2 配線」「2.3 配線（ESP-WROOM-32D）」を参照してください。

## 2. ツールチェーンのセットアップ

### 2.1 Rust（Pico W 向け・stable）

```bash
# rustup 未導入なら https://rustup.rs から導入
rustup default stable
# 組込みターゲット（RP2040 = Cortex-M0+）
rustup target add thumbv6m-none-eabi
rustup component add rustfmt clippy
```

### 2.2 Rust（ESP-WROOM-32D 向け・Espressif フォーク）

ESP32（無印）は **Xtensa** アーキテクチャで、LLVM 本家が未対応のため
**stable Rust では扱えません**。Espressif の Rust フォークを `espup` で導入します。

```bash
cargo install espup
espup install --targets esp32
```

導入後、シェルに環境変数を読み込みます（`espup` が出力するパス）。

```bash
# Linux / macOS（シェル起動時に読み込むと便利）
. $HOME/export-esp.sh
```

```powershell
# Windows PowerShell
. $HOME\export-esp.ps1
```

`firmware-esp32/rust-toolchain.toml` で `channel = "esp"` を指定しているため、
このディレクトリでは自動的に esp ツールチェーンが使われます。

> `firmware-esp32/` はルートワークスペースから `exclude` されており、独立した
> `Cargo.lock` を持ちます。**必ず `firmware-esp32` ディレクトリ内で** cargo を実行してください。

### 2.3 書き込み・デバッグツール

**Pico W** — 用途に応じてどちらか（両方でも可）。

- **UF2 書き込み（プローブ不要）**: `elf2uf2-rs`
  ```bash
  cargo install elf2uf2-rs
  ```
- **probe-rs（プローブ使用、ログ表示・高速書き込み）**:
  ```bash
  cargo install probe-rs-tools    # `probe-rs` コマンドを提供
  ```

> Windows で probe-rs を使う場合、プローブの USB ドライバ設定が必要なことがある
> （[probe.rs のドキュメント](https://probe.rs/docs/) 参照）。

**ESP-WROOM-32D** — `espflash`（USB シリアル経由で書き込み・ログ表示）。

```bash
cargo install espflash
```

> Windows では USB-シリアル変換チップ（CP2102 / CH340 等）のドライバが必要な場合があります。
> Linux では書き込み用にシリアルポートへのアクセス権（`dialout` グループ等）が必要です。

## 3. CYW43 ファームウェア blob の取得（**Pico W のみ**）

> **ESP-WROOM-32D では不要です。** Bluetooth がチップに内蔵されているため、
> blob の取得なしにそのままビルド・書き込みできます。この章は読み飛ばしてください。

Pico W の無線を動かすには Infineon 提供の blob が必要（リポジトリには非同梱）。

**macOS / Linux（bash）**:

```bash
# リポジトリルートで実行
bash scripts/fetch-cyw43-firmware.sh
```

**Windows（PowerShell）**:

```powershell
# リポジトリのどこからでも可（PowerShell 5.1 / 7 いずれも）
pwsh -File scripts/fetch-cyw43-firmware.ps1
# もしくは
powershell -ExecutionPolicy Bypass -File scripts\fetch-cyw43-firmware.ps1
```

`firmware/cyw43-firmware/` に以下が配置される:
`43439A0.bin` / `43439A0_clm.bin` / `43439A0_btfw.bin` / `nvram_rp2040.bin`

> blob 無しでビルド確認だけしたい場合は、後述の各コマンドに
> `--features skip-cyw43-firmware` を付ける（実機では動作しない）。

## 4. ビルド

### 4.1 中核ロジック（ホスト、実機不要）

```bash
cargo test -p pico-temp-core
```

### 4.2 ファームウェア（Pico W / `thumbv6m-none-eabi`）

```bash
cd firmware
cargo build --release            # blob 取得済みの場合
# もしくは
cargo build --release --features skip-cyw43-firmware   # ビルド確認のみ
```

成果物: `firmware/target/thumbv6m-none-eabi/release/pico-temperature-firmware`

### 4.3 ファームウェア（ESP-WROOM-32D / `xtensa-esp32-none-elf`）

esp ツールチェーンの環境変数を読み込んだ状態で（2.2 参照）:

```bash
cd firmware-esp32
cargo build --release
```

成果物: `firmware-esp32/target/xtensa-esp32-none-elf/release/esp32-temperature-firmware`

> blob 不要のため、Pico W のような `skip-*` フィーチャはありません。そのままビルドできます。

## 5. 実機への書き込み

### 5.0 ESP-WROOM-32D（espflash）

USB シリアルで接続した状態で、`firmware-esp32` ディレクトリから:

```bash
cd firmware-esp32
cargo run --release
```

`.cargo/config.toml` の `runner = "espflash flash --monitor"` により、
**書き込み後そのままシリアルモニタが開き**、`log` の出力（`DS18B20: 2350 centi-degC` など）が
表示されます。

- ポートを明示したい場合: `espflash flash --monitor --port /dev/ttyUSB0 <ELF パス>`
  （Windows は `--port COM3` など）
- 書き込みが始まらないボードでは、**BOOT ボタンを押しながら EN(RST) を一度押す**と
  ダウンロードモードに入ります。

以降 5.1 / 5.2 は **Pico W 向け**の手順です。

### 5.1 UF2 で書き込む（Pico W、プローブ不要）

1. Pico W の **BOOTSEL ボタンを押しながら** USB 接続 → `RPI-RP2` ドライブとして認識。
2. ELF を UF2 に変換して書き込み:
   ```bash
   cd firmware
   elf2uf2-rs -d target/thumbv6m-none-eabi/release/pico-temperature-firmware
   ```
   （`-d` で認識中の RPI-RP2 ドライブへ自動コピー）
3. 書き込み後、Pico W が自動リセットして起動。

### 5.2 probe-rs で書き込む（Pico W、プローブ使用、ログ表示）

デバッグプローブを SWD（SWCLK/SWDIO/GND）に接続した状態で:

```bash
cd firmware
cargo run --release    # .cargo/config.toml の runner = "probe-rs run --chip RP2040"
```

defmt ログ（`info!` など）がホスト側に表示される。

## 6. 動作確認（概要）

詳細は `test_procedure.md` を参照。

1. スマホに **nRF Connect**（iOS/Android）等の BLE スキャナを入れる。
2. スキャンで `PicoTemp` を探す。
3. アドバタイズの Service Data（UUID 0x181A）に温度（センチ℃, LE）が載る。
4. 接続して Environmental Sensing → Temperature(0x2A6E) を Read / Notify。

## 7. トラブルシュート

**共通**

| 症状 | 対処 |
| --- | --- |
| `PicoTemp` が見つからない | 給電・書き込み確認。ログ（probe-rs / espflash モニタ）で BLE 起動を確認。 |
| 温度が 0 / エラーログ | まず**ログに `DS18B20` の行が出ているか**で切り分ける。<br>・`DS18B20 read error` が定期的に出る → 配線・4.7kΩ プルアップ・データ線 GPIO を確認（Pico W は GPIO15、ESP32 は GPIO4）。<br>・`DS18B20` の行が**一切出ない** → センサではなくタスク側の問題。ESP32 の項を参照。 |

**Pico W**

| 症状 | 対処 |
| --- | --- |
| ビルドで blob エラー | `scripts/fetch-cyw43-firmware.sh`（Windows は `.ps1`）を実行、または `--features skip-cyw43-firmware`。 |
| probe-rs がプローブを認識しない | ドライバ設定・接続（SWD）を確認。 |
| `can't find crate for core` | `rustup target add thumbv6m-none-eabi` を実行。 |

**ESP-WROOM-32D**

| 症状 | 対処 |
| --- | --- |
| `toolchain 'esp' is not installed` | `espup install --targets esp32` 実行後、`export-esp.sh`（Windows は `export-esp.ps1`）を読み込む。 |
| `can't find crate for core` | esp ツールチェーンの環境変数が未読み込み。上記 `export-esp.*` を読み込む。 |
| 書き込みが始まらない / タイムアウト | **BOOT を押しながら EN(RST) を一度押して**ダウンロードモードへ。ポート指定（`--port`）も確認。 |
| シリアルポートが開けない（Linux） | ユーザーを `dialout` グループへ追加、または `sudo` で実行。 |
| ログが文字化けする | モニタのボーレートを 115200 に設定。 |
| 温度の読み取りが不安定 | 1-Wire はソフトタイミング。配線を短く、プルアップを確実に。GPIO15/0/2/12（ストラッピング）を使っていないか確認。 |
| `DS18B20 ROM read error` が出る／ファミリコードが 0x28 でない | 起動時セルフテストの失敗。1-Wire 通信が成立していない。配線・プルアップ・GPIO 番号を確認する。バス上にセンサが 2 個以上あると Read ROM は必ず失敗する（その場合はセルフテストの失敗を無視してよい）。 |
| `CrcMismatch` が続く | 直前の `[1-Wire] scratchpad raw = [..]` の生バイト列で切り分ける。<br>・**1 に偏る（0xFF が多い）** → 読み取りサンプル点が遅い。`onewire.rs` の `read_bit` の待機値を短くする。<br>・**毎回ランダムに散る** → タイミング擾乱。ビットバンギングを free 関数化して `#[esp_hal::ram]` で IRAM 配置を検討。<br>・**全 0x00** → 給電またはプルアップの物理的な問題。 |
| 温度が 0.00 で固定され、`DS18B20` のログが**1 行も出ない**（BLE は正常に広告・接続できる） | embassy のタイマが動いていない。`Cargo.lock` で embassy クレートが crates.io 版と git 版に**分裂**していないか確認する。<br>`grep -c 'name = "embassy-executor-timer-queue"' firmware-esp32/Cargo.lock` が **1** でなければ該当。<br>対処: `firmware-esp32/Cargo.toml` の `[patch.crates-io]` で embassy ファミリ（`embassy-time` / `embassy-time-driver` / `embassy-time-queue-utils` / `embassy-executor-timer-queue` 等）を**すべて同一 rev**へ固定し、lock を再生成する。<br>詳細は `specification.md` の「ESP32 版の依存方針」を参照。 |
