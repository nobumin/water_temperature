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

| 品目 | 備考 |
| --- | --- |
| Raspberry Pi Pico W | 無線チップ必須（無印 Pico 不可） |
| DS18B20 | 1-Wire 温度センサ |
| 4.7kΩ 抵抗 | データ線プルアップ |
| ブレッドボード・ジャンパ線 | 配線用 |
| USB ケーブル（micro-B） | 書き込み・給電 |
| （任意）デバッグプローブ | Raspberry Pi Debug Probe 等。probe-rs でのログ表示に使用 |

### 配線（`specification.md` 参照）

| DS18B20 | Pico W |
| --- | --- |
| VDD | 3V3(OUT)（物理36） |
| DQ | GPIO15（物理20）＋ 4.7kΩ で 3V3 へ |
| GND | GND（物理38 など） |

## 2. ツールチェーンのセットアップ

### 2.1 Rust

```bash
# rustup 未導入なら https://rustup.rs から導入
rustup default stable
# 組込みターゲット（RP2040 = Cortex-M0+）
rustup target add thumbv6m-none-eabi
rustup component add rustfmt clippy
```

### 2.2 書き込み・デバッグツール

用途に応じてどちらか（両方でも可）。

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

## 3. CYW43 ファームウェア blob の取得

Pico W の無線を動かすには Infineon 提供の blob が必要（リポジトリには非同梱）。

```bash
# リポジトリルートで実行
bash scripts/fetch-cyw43-firmware.sh
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

### 4.2 ファームウェア（組込みターゲット）

```bash
cd firmware
cargo build --release            # blob 取得済みの場合
# もしくは
cargo build --release --features skip-cyw43-firmware   # ビルド確認のみ
```

成果物: `firmware/target/thumbv6m-none-eabi/release/pico-temperature-firmware`

## 5. 実機への書き込み

### 5.1 UF2 で書き込む（プローブ不要）

1. Pico W の **BOOTSEL ボタンを押しながら** USB 接続 → `RPI-RP2` ドライブとして認識。
2. ELF を UF2 に変換して書き込み:
   ```bash
   cd firmware
   elf2uf2-rs -d target/thumbv6m-none-eabi/release/pico-temperature-firmware
   ```
   （`-d` で認識中の RPI-RP2 ドライブへ自動コピー）
3. 書き込み後、Pico W が自動リセットして起動。

### 5.2 probe-rs で書き込む（プローブ使用、ログ表示）

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

| 症状 | 対処 |
| --- | --- |
| `PicoTemp` が見つからない | 給電・書き込み確認。probe-rs のログで BLE 起動を確認。 |
| 温度が 0 / エラーログ | 配線・4.7kΩ プルアップ・GPIO15 を確認（`DS18B20 read error`）。 |
| ビルドで blob エラー | `scripts/fetch-cyw43-firmware.sh` 実行、または `--features skip-cyw43-firmware`。 |
| probe-rs がプローブを認識しない | ドライバ設定・接続（SWD）を確認。 |
