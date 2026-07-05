//! DS18B20 スクラッチパッドの解析・CRC 検証・温度変換。
//!
//! DS18B20 はスクラッチパッドを 9 バイトで返す:
//!
//! | Byte | 内容 |
//! | ---- | ---- |
//! | 0    | 温度 LSB |
//! | 1    | 温度 MSB |
//! | 2    | TH レジスタ / ユーザーバイト1 |
//! | 3    | TL レジスタ / ユーザーバイト2 |
//! | 4    | Configuration レジスタ (分解能) |
//! | 5-7  | 予約 |
//! | 8    | CRC8 (Dallas/Maxim) |
//!
//! 温度は 16bit 符号付きで、単位は 1/16 ℃ (12bit 分解能時)。

/// DS18B20 の温度値。内部表現は DS18B20 本来の単位である **1/16 ℃** の
/// 符号付き 16bit 生値。派生値(ミリ℃・センチ℃・ESS バイト列)を提供する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Temperature {
    /// 1/16 ℃ 単位の符号付き生値 (温度 MSB:LSB をそのまま結合したもの)。
    raw: i16,
}

impl Temperature {
    /// DS18B20 の 1/16 ℃ 生値から生成する。
    pub const fn from_raw(raw: i16) -> Self {
        Self { raw }
    }

    /// 1/16 ℃ 単位の生値を返す。
    pub const fn raw(self) -> i16 {
        self.raw
    }

    /// ミリ℃ (1/1000 ℃) を四捨五入(0から遠い側へ)して返す。
    ///
    /// 1LSB = 1/16 ℃ = 62.5 ミリ℃ のため、`raw * 125 / 2` を丸める。
    pub const fn milli_celsius(self) -> i32 {
        div_round(self.raw as i32 * 125, 2)
    }

    /// センチ℃ (1/100 ℃) を四捨五入して返す。BLE ESS Temperature 特性の単位。
    ///
    /// 1LSB = 1/16 ℃ = 6.25 センチ℃ のため、`raw * 100 / 16` を丸める。
    /// DS18B20 の測定範囲(-55〜+125℃)は i16 に収まるが、防御的に飽和させる。
    pub const fn centi_celsius(self) -> i16 {
        let centi = div_round(self.raw as i32 * 100, 16);
        if centi > i16::MAX as i32 {
            i16::MAX
        } else if centi < i16::MIN as i32 {
            i16::MIN
        } else {
            centi as i16
        }
    }

    /// BLE ESS Temperature 特性(0x2A6E)のペイロード。
    /// sint16・センチ℃・リトルエンディアン 2 バイト。
    pub const fn to_ess_bytes(self) -> [u8; 2] {
        self.centi_celsius().to_le_bytes()
    }
}

/// DS18B20 のスクラッチパッド(9 バイト)を解析するためのラッパ。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scratchpad {
    /// 生の 9 バイト。
    pub bytes: [u8; 9],
}

impl Scratchpad {
    /// 9 バイトの生データから生成する。
    pub const fn new(bytes: [u8; 9]) -> Self {
        Self { bytes }
    }

    /// Configuration レジスタ(byte 4)を返す。
    pub const fn config_register(self) -> u8 {
        self.bytes[4]
    }

    /// 分解能ビット数(9〜12)を Configuration レジスタから求める。
    pub const fn resolution_bits(self) -> u8 {
        // R1:R0 = bit6:bit5。00→9, 01→10, 10→11, 11→12
        9 + ((self.bytes[4] >> 5) & 0b11)
    }

    /// CRC を検証し、温度へ変換する。
    ///
    /// # エラー
    /// - [`Ds18b20Error::Disconnected`]: 全バイト 0xFF (バス開放=センサ未接続)
    /// - [`Ds18b20Error::NoResponse`]: 全バイト 0x00 (応答なし)
    /// - [`Ds18b20Error::CrcMismatch`]: CRC 不一致(通信ノイズ等)
    pub fn temperature(self) -> Result<Temperature, Ds18b20Error> {
        if self.bytes.iter().all(|&b| b == 0xFF) {
            return Err(Ds18b20Error::Disconnected);
        }
        if self.bytes.iter().all(|&b| b == 0x00) {
            return Err(Ds18b20Error::NoResponse);
        }
        let computed = crc8(&self.bytes[0..8]);
        let expected = self.bytes[8];
        if computed != expected {
            return Err(Ds18b20Error::CrcMismatch { expected, computed });
        }
        let raw = i16::from_le_bytes([self.bytes[0], self.bytes[1]]);
        Ok(Temperature::from_raw(raw))
    }
}

/// DS18B20 の読み取りエラー。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Ds18b20Error {
    /// バスが常に High (全 0xFF)。プルアップのみでセンサが応答していない状態。
    Disconnected,
    /// 全 0x00。バスショートや電源不良で応答がない状態。
    NoResponse,
    /// CRC8 不一致。`expected` はスクラッチパッド末尾の CRC、`computed` は計算値。
    CrcMismatch {
        /// スクラッチパッド byte8 に格納されていた CRC。
        expected: u8,
        /// byte0..8 から計算した CRC。
        computed: u8,
    },
}

/// Dallas/Maxim 1-Wire CRC8 を計算する。
///
/// 多項式 x^8 + x^5 + x^4 + 1 (反射表現 0x8C)。初期値 0。
pub const fn crc8(data: &[u8]) -> u8 {
    let mut crc: u8 = 0;
    let mut i = 0;
    while i < data.len() {
        let mut byte = data[i];
        let mut bit = 0;
        while bit < 8 {
            let mix = (crc ^ byte) & 0x01;
            crc >>= 1;
            if mix != 0 {
                crc ^= 0x8C;
            }
            byte >>= 1;
            bit += 1;
        }
        i += 1;
    }
    crc
}

/// `n / d` を 0 から遠い側へ丸める(四捨五入)。`d > 0` を前提とする。
const fn div_round(n: i32, d: i32) -> i32 {
    let half = d / 2;
    if n >= 0 {
        (n + half) / d
    } else {
        (n - half) / d
    }
}
