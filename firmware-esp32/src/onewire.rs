//! DS18B20 用の 1-Wire ビットバンギングドライバ（ESP32 版）。
//!
//! ESP32 の GPIO を [`Flex`] の**オープンドレイン出力＋入力有効**として扱い、
//! 1-Wire のタイムスロットをマイクロ秒精度で生成する。
//! オープンドレインなので `set_low()` でバスを Low に駆動し、`set_high()` で解放
//! （外部プルアップで High に戻る）できる。
//!
//! タイミングが厳しいリセット／ビット送受信は [`critical_section`] で割り込みを禁止して行う。
//! ESP32 は Wi-Fi/BT スタックが割り込みを多用するため、この保護が特に重要。
//! 変換完了待ち(最大 750ms)は非同期 [`Timer`] で待機し、CPU を明け渡す。
//!
//! 外部に **4.7kΩ プルアップ抵抗(データ線⇔3.3V)** が必要(DS18B20 の仕様)。
//!
//! プロトコル手順は Pico W 版(`firmware/src/onewire.rs`)と同一だが、
//! **µs 待機の実装だけはボード固有**（本モジュールの `delay_us` のコメント参照）。

use embassy_time::Timer;
use esp_hal::gpio::{DriveMode, Flex, InputConfig, OutputConfig, Pull};
use esp_hal::rom::ets_delay_us;
use log::warn;
use pico_temp_core::ds18b20::{crc8, Ds18b20Error, Scratchpad, Temperature};

/// DS18B20 の ROM コマンド。単一センサ前提のため Skip ROM を用いる。
const CMD_SKIP_ROM: u8 = 0xCC;
/// ROM コード(64bit)読み出し。バス上にセンサが 1 個のときのみ使える。
const CMD_READ_ROM: u8 = 0x33;
/// 温度変換開始。
const CMD_CONVERT_T: u8 = 0x44;
/// スクラッチパッド読み出し。
const CMD_READ_SCRATCHPAD: u8 = 0xBE;
/// 12bit 分解能の最大変換時間(ms)。余裕を見て待機する。
const CONVERSION_TIME_MS: u64 = 750;
/// DS18B20 のファミリコード。Read ROM の 1 バイト目がこの値になる。
pub const FAMILY_CODE: u8 = 0x28;

/// マイクロ秒のブロッキング待機(タイムスロット生成用)。
///
/// **`esp_hal::delay::Delay` を使ってはならない。** ESP32 の `Delay` は内部で
/// `Instant::now()` をループで呼ぶが、ESP32 の `Instant::now()` は TIMG0 の LACT
/// タイマを読む実装で、「更新完了ビットが無いため下位 32bit が変化するまで
/// ポーリングする」という重い手順を踏む。このため 1 回の呼び出しに µs オーダーの
/// オーバーヘッドが乗り、`delay_us(6)` が実際には十数 µs かかってしまう。
///
/// 1-Wire の読み取りは**立ち下がりから 15µs 以内**にサンプルする必要があるため、
/// このオーバーヘッドがあるとサンプル点が規定を超え、センサが出した 0 を 1 として
/// 読んでしまう(実機で `CrcMismatch` / 全 0xFF の `Disconnected` として観測された)。
///
/// ROM の `ets_delay_us` は CCOUNT ベースの busy-loop でバスアクセスを伴わない。
/// CPU 周波数変更時に esp-hal が `ets_update_cpu_frequency_rom()` を呼ぶため、
/// `CpuClock::max()` を設定した後でも精度が保たれる。
#[inline(always)]
fn delay_us(us: u32) {
    ets_delay_us(us);
}

/// 単一の DS18B20 を 1-Wire で読むドライバ。
pub struct Ds18b20<'d> {
    pin: Flex<'d>,
}

impl<'d> Ds18b20<'d> {
    /// データ線に接続した GPIO(Flex)からドライバを生成する。
    pub fn new(mut pin: Flex<'d>) -> Self {
        // 内部プルアップは使わず、外部 4.7kΩ に任せる(DS18B20 の推奨)。
        pin.apply_input_config(&InputConfig::default().with_pull(Pull::None));
        pin.apply_output_config(&OutputConfig::default().with_drive_mode(DriveMode::OpenDrain));
        // 出力(オープンドレイン)と入力を同時に有効化し、駆動と読み取りを両立する。
        pin.set_input_enable(true);
        pin.set_output_enable(true);
        // 初期状態はバス解放(High = ハイインピーダンス)。
        pin.set_high();
        Self { pin }
    }

    /// バスを Low に駆動する。
    fn drive_low(&mut self) {
        self.pin.set_low();
    }

    /// バスを解放する(オープンドレインのため外部プルアップで High に戻る)。
    fn release(&mut self) {
        self.pin.set_high();
    }

    /// 現在のバスレベルを読む(High=true)。
    fn sample(&mut self) -> bool {
        self.pin.is_high()
    }

    /// リセットパルスを送り、プレゼンスパルス(センサ応答)の有無を返す。
    fn reset(&mut self) -> bool {
        critical_section::with(|_| {
            self.drive_low();
            delay_us(480);
            self.release();
            delay_us(70);
            let present = !self.sample(); // センサ在席時はバスが Low に引かれる
            delay_us(410);
            present
        })
    }

    /// 1 ビット送信。
    fn write_bit(&mut self, bit: bool) {
        critical_section::with(|_| {
            self.drive_low();
            if bit {
                delay_us(6);
                self.release();
                delay_us(64);
            } else {
                delay_us(60);
                self.release();
                delay_us(10);
            }
        });
    }

    /// 1 ビット受信。
    ///
    /// サンプル点は `3 + 8` = **立ち下がりから約 11µs**。DS18B20 の規定は「15µs 以内」だが、
    /// 規定値ちょうどの `6 + 9` = 15µs にすると余裕がゼロになり、わずかなオーバーヘッドで
    /// センサの 0 を取りこぼす。センサは立ち下がりから 1µs 以内に 0 を駆動し、1 の場合も
    /// 外部プルアップによる立ち上がりは数 µs で完了するため、11µs は「遅すぎず早すぎない」
    /// 安全域になる。スロット全体は 60µs 以上を維持する。
    fn read_bit(&mut self) -> bool {
        critical_section::with(|_| {
            self.drive_low();
            delay_us(3);
            self.release();
            delay_us(8);
            let bit = self.sample();
            delay_us(50);
            bit
        })
    }

    /// 1 バイト送信(LSB ファースト)。
    fn write_byte(&mut self, mut byte: u8) {
        for _ in 0..8 {
            self.write_bit(byte & 0x01 == 0x01);
            byte >>= 1;
        }
    }

    /// 1 バイト受信(LSB ファースト)。
    fn read_byte(&mut self) -> u8 {
        let mut byte = 0u8;
        for i in 0..8 {
            if self.read_bit() {
                byte |= 1 << i;
            }
        }
        byte
    }

    /// reset でプレゼンスパルスが得られなかったときのエラー種別を、バスレベルから判定する。
    ///
    /// - バスが High（外部プルアップで釣られている＝バス開放）: [`Ds18b20Error::Disconnected`]
    /// - バスが Low に張り付いている（ショート/電源不良等）: [`Ds18b20Error::NoResponse`]
    fn absence_error(&mut self) -> Ds18b20Error {
        if self.sample() {
            Ds18b20Error::Disconnected
        } else {
            Ds18b20Error::NoResponse
        }
    }

    /// ROM コード(64bit)を読み出す。**バス上にセンサが 1 個のときのみ**有効。
    ///
    /// 起動時のセルフテスト用。1 バイト目が [`FAMILY_CODE`] (0x28) で CRC が合えば、
    /// 配線と 1-Wire のタイミングが正常であることを確定できる。温度変換を伴わないため、
    /// 「通信の問題」と「変換シーケンスの問題」を切り分けられる。
    pub fn read_rom(&mut self) -> Result<[u8; 8], Ds18b20Error> {
        if !self.reset() {
            return Err(self.absence_error());
        }
        self.write_byte(CMD_READ_ROM);

        let mut rom = [0u8; 8];
        for b in rom.iter_mut() {
            *b = self.read_byte();
        }

        if rom.iter().all(|&b| b == 0xFF) {
            return Err(Ds18b20Error::Disconnected);
        }
        if rom.iter().all(|&b| b == 0x00) {
            return Err(Ds18b20Error::NoResponse);
        }
        // ROM は 先頭 7 バイト(family + シリアル)に対する CRC が末尾に入る。
        let computed = crc8(&rom[0..7]);
        if computed != rom[7] {
            warn!("[1-Wire] ROM raw = {:02X?}", rom);
            return Err(Ds18b20Error::CrcMismatch {
                expected: rom[7],
                computed,
            });
        }
        Ok(rom)
    }

    /// 温度を 1 回測定して返す。
    ///
    /// 手順: リセット → Skip ROM → Convert T → 変換待ち → リセット →
    /// Skip ROM → Read Scratchpad → 9 バイト読み出し → CRC 検証・変換。
    pub async fn read(&mut self) -> Result<Temperature, Ds18b20Error> {
        if !self.reset() {
            return Err(self.absence_error());
        }
        self.write_byte(CMD_SKIP_ROM);
        self.write_byte(CMD_CONVERT_T);

        // 変換完了待ち(非同期)。この間は他タスク(BLE 等)が動作できる。
        Timer::after_millis(CONVERSION_TIME_MS).await;

        if !self.reset() {
            return Err(self.absence_error());
        }
        self.write_byte(CMD_SKIP_ROM);
        self.write_byte(CMD_READ_SCRATCHPAD);

        let mut bytes = [0u8; 9];
        for b in bytes.iter_mut() {
            *b = self.read_byte();
        }

        let result = Scratchpad::new(bytes).temperature();
        if result.is_err() {
            // エラー種別だけでは原因を絞れないため、生バイト列も残す。
            // 1 に偏る→サンプルが遅い / ランダム→タイミング擾乱 / 全 0x00→給電・プルアップ。
            warn!("[1-Wire] scratchpad raw = {:02X?}", bytes);
        }
        result
    }
}
