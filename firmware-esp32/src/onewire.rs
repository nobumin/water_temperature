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
//! Pico W 版(`firmware/src/onewire.rs`)とプロトコル手順・タイミングは同一。

use embassy_time::Timer;
use esp_hal::delay::Delay;
use esp_hal::gpio::{DriveMode, Flex, InputConfig, OutputConfig, Pull};
use pico_temp_core::ds18b20::{Ds18b20Error, Scratchpad, Temperature};

/// DS18B20 の ROM コマンド。単一センサ前提のため Skip ROM を用いる。
const CMD_SKIP_ROM: u8 = 0xCC;
/// 温度変換開始。
const CMD_CONVERT_T: u8 = 0x44;
/// スクラッチパッド読み出し。
const CMD_READ_SCRATCHPAD: u8 = 0xBE;
/// 12bit 分解能の最大変換時間(ms)。余裕を見て待機する。
const CONVERSION_TIME_MS: u64 = 750;

/// 単一の DS18B20 を 1-Wire で読むドライバ。
pub struct Ds18b20<'d> {
    pin: Flex<'d>,
    delay: Delay,
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
        Self {
            pin,
            delay: Delay::new(),
        }
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

    /// マイクロ秒のブロッキング待機(タイムスロット生成用)。
    fn delay_us(&self, us: u32) {
        self.delay.delay_micros(us);
    }

    /// リセットパルスを送り、プレゼンスパルス(センサ応答)の有無を返す。
    fn reset(&mut self) -> bool {
        critical_section::with(|_| {
            self.drive_low();
            self.delay_us(480);
            self.release();
            self.delay_us(70);
            let present = !self.sample(); // センサ在席時はバスが Low に引かれる
            self.delay_us(410);
            present
        })
    }

    /// 1 ビット送信。
    fn write_bit(&mut self, bit: bool) {
        critical_section::with(|_| {
            self.drive_low();
            if bit {
                self.delay_us(6);
                self.release();
                self.delay_us(64);
            } else {
                self.delay_us(60);
                self.release();
                self.delay_us(10);
            }
        });
    }

    /// 1 ビット受信。
    fn read_bit(&mut self) -> bool {
        critical_section::with(|_| {
            self.drive_low();
            self.delay_us(6);
            self.release();
            self.delay_us(9);
            let bit = self.sample();
            self.delay_us(55);
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

        Scratchpad::new(bytes).temperature()
    }
}
