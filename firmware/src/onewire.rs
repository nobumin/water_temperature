//! DS18B20 用の 1-Wire ビットバンギングドライバ。
//!
//! RP2040 の GPIO を [`Flex`] でオープンドレイン相当に扱い、1-Wire のタイムスロットを
//! マイクロ秒精度で生成する。タイミングが厳しいリセット／ビット送受信は
//! [`critical_section`] で割り込みを禁止して行う(スロット中の割り込み混入を防ぐ)。
//! 変換完了待ち(最大 750ms)は非同期 [`Timer`] で待機し、CPU を明け渡す。
//!
//! 外部に **4.7kΩ プルアップ抵抗(データ線⇔3.3V)** が必要(DS18B20 の仕様)。

use embassy_rp::gpio::Flex;
use embassy_time::{block_for, Duration, Timer};
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
}

impl<'d> Ds18b20<'d> {
    /// データ線に接続した GPIO(Flex)からドライバを生成する。
    pub fn new(mut pin: Flex<'d>) -> Self {
        // 初期状態はバス解放(入力=ハイインピーダンス、外部プルアップで High)。
        pin.set_as_input();
        Self { pin }
    }

    /// バスを Low に駆動する(出力=0)。
    fn drive_low(&mut self) {
        self.pin.set_low();
        self.pin.set_as_output();
    }

    /// バスを解放する(入力=ハイインピーダンス)。外部プルアップで High に戻る。
    fn release(&mut self) {
        self.pin.set_as_input();
    }

    /// 現在のバスレベルを読む(High=true)。
    fn sample(&mut self) -> bool {
        self.pin.is_high()
    }

    /// リセットパルスを送り、プレゼンスパルス(センサ応答)の有無を返す。
    fn reset(&mut self) -> bool {
        critical_section::with(|_| {
            self.drive_low();
            block_for(Duration::from_micros(480));
            self.release();
            block_for(Duration::from_micros(70));
            let present = !self.sample(); // センサ在席時はバスが Low に引かれる
            block_for(Duration::from_micros(410));
            present
        })
    }

    /// 1 ビット送信。
    fn write_bit(&mut self, bit: bool) {
        critical_section::with(|_| {
            self.drive_low();
            if bit {
                block_for(Duration::from_micros(6));
                self.release();
                block_for(Duration::from_micros(64));
            } else {
                block_for(Duration::from_micros(60));
                self.release();
                block_for(Duration::from_micros(10));
            }
        });
    }

    /// 1 ビット受信。
    fn read_bit(&mut self) -> bool {
        critical_section::with(|_| {
            self.drive_low();
            block_for(Duration::from_micros(6));
            self.release();
            block_for(Duration::from_micros(9));
            let bit = self.sample();
            block_for(Duration::from_micros(55));
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

    /// 温度を 1 回測定して返す。
    ///
    /// 手順: リセット → Skip ROM → Convert T → 変換待ち → リセット →
    /// Skip ROM → Read Scratchpad → 9 バイト読み出し → CRC 検証・変換。
    pub async fn read(&mut self) -> Result<Temperature, Ds18b20Error> {
        if !self.reset() {
            return Err(Ds18b20Error::NoResponse);
        }
        self.write_byte(CMD_SKIP_ROM);
        self.write_byte(CMD_CONVERT_T);

        // 変換完了待ち(非同期)。この間は他タスク(BLE 等)が動作できる。
        Timer::after_millis(CONVERSION_TIME_MS).await;

        if !self.reset() {
            return Err(Ds18b20Error::NoResponse);
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
