//! ホスト上で実行するユニットテスト(実機不要)。
//! DS18B20 の温度変換・CRC・ESS エンコードを検証する。

use pico_temp_core::ds18b20::{crc8, Ds18b20Error, Scratchpad, Temperature};
use pico_temp_core::ess::{advertisement_service_data, ESS_SERVICE_UUID, TEMPERATURE_CHAR_UUID};

/// 9 バイトのデータ(先頭 8 バイト)から正しい CRC を付与したスクラッチパッドを作る補助関数。
fn scratchpad_with_crc(first8: [u8; 8]) -> [u8; 9] {
    let mut b = [0u8; 9];
    b[..8].copy_from_slice(&first8);
    b[8] = crc8(&first8);
    b
}

// ---- 温度変換 ----

#[test]
fn raw_to_celsius_positive() {
    // データシート例: +25.0625℃ = 0x0191 (=401, 1/16℃単位)
    let t = Temperature::from_raw(0x0191);
    assert_eq!(t.raw(), 401);
    assert_eq!(t.centi_celsius(), 2506); // round(401*100/16)=round(2506.25)
    assert_eq!(t.milli_celsius(), 25063); // round(401*125/2)=round(25062.5)
    assert_eq!(t.to_ess_bytes(), [0xCA, 0x09]); // 2506 = 0x09CA, LE
}

#[test]
fn raw_to_celsius_zero() {
    let t = Temperature::from_raw(0);
    assert_eq!(t.centi_celsius(), 0);
    assert_eq!(t.milli_celsius(), 0);
    assert_eq!(t.to_ess_bytes(), [0x00, 0x00]);
}

#[test]
fn raw_to_celsius_negative() {
    // -10.125℃ = -162 (1/16℃単位)
    let t = Temperature::from_raw(-162);
    assert_eq!(t.centi_celsius(), -1013); // round(-16200/16)=round(-1012.5)
    assert_eq!(t.milli_celsius(), -10125); // -162*125/2 = -10125 (厳密)
}

#[test]
fn raw_to_celsius_range_limits() {
    // DS18B20 の測定範囲 -55℃ 〜 +125℃
    assert_eq!(Temperature::from_raw(-55 * 16).centi_celsius(), -5500);
    assert_eq!(Temperature::from_raw(125 * 16).centi_celsius(), 12500);
    assert_eq!(Temperature::from_raw(-55 * 16).milli_celsius(), -55000);
    assert_eq!(Temperature::from_raw(125 * 16).milli_celsius(), 125000);
}

// ---- スクラッチパッド解析 ----

#[test]
fn scratchpad_parses_valid_reading() {
    // +25.0625℃ を含む有効なスクラッチパッド(config=0x7F: 12bit)
    let bytes = scratchpad_with_crc([0x91, 0x01, 0x4B, 0x46, 0x7F, 0xFF, 0x0C, 0x10]);
    let sp = Scratchpad::new(bytes);
    assert_eq!(sp.resolution_bits(), 12);
    let t = sp.temperature().expect("valid scratchpad");
    assert_eq!(t.raw(), 0x0191);
}

#[test]
fn scratchpad_power_on_default_85c() {
    // 広く知られた DS18B20 の電源投入時スクラッチパッド。CRC=0x1C。
    let bytes = [0x50, 0x05, 0x4B, 0x46, 0x7F, 0xFF, 0x0C, 0x10, 0x1C];
    let sp = Scratchpad::new(bytes);
    let t = sp.temperature().expect("valid power-on scratchpad");
    assert_eq!(t.raw(), 0x0550);
    assert_eq!(t.milli_celsius(), 85000); // 85.0℃
}

#[test]
fn scratchpad_disconnected_all_ones() {
    let sp = Scratchpad::new([0xFF; 9]);
    assert_eq!(sp.temperature(), Err(Ds18b20Error::Disconnected));
}

#[test]
fn scratchpad_no_response_all_zeros() {
    let sp = Scratchpad::new([0x00; 9]);
    assert_eq!(sp.temperature(), Err(Ds18b20Error::NoResponse));
}

#[test]
fn scratchpad_crc_mismatch_detected() {
    let mut bytes = scratchpad_with_crc([0x91, 0x01, 0x4B, 0x46, 0x7F, 0xFF, 0x0C, 0x10]);
    let good_crc = bytes[8];
    bytes[8] ^= 0xFF; // CRC を破壊
    let sp = Scratchpad::new(bytes);
    assert_eq!(
        sp.temperature(),
        Err(Ds18b20Error::CrcMismatch {
            expected: good_crc ^ 0xFF,
            computed: good_crc,
        })
    );
}

#[test]
fn resolution_bits_from_config_register() {
    let mk = |cfg: u8| {
        Scratchpad::new([0x00, 0x00, 0x00, 0x00, cfg, 0x00, 0x00, 0x00, 0x00]).resolution_bits()
    };
    assert_eq!(mk(0x1F), 9);
    assert_eq!(mk(0x3F), 10);
    assert_eq!(mk(0x5F), 11);
    assert_eq!(mk(0x7F), 12);
}

// ---- CRC8 ----

/// 表ベースの独立実装(Dallas/Maxim, poly 0x8C 反射, init 0)。
/// ビット演算版 [`crc8`] とのクロスチェックに用いる。
fn crc8_table_reference(data: &[u8]) -> u8 {
    // 実行時に生成する 256 エントリのテーブル。
    let mut table = [0u8; 256];
    let mut i = 0usize;
    while i < 256 {
        let mut crc = i as u8;
        let mut bit = 0;
        while bit < 8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0x8C;
            } else {
                crc >>= 1;
            }
            bit += 1;
        }
        table[i] = crc;
        i += 1;
    }
    let mut crc = 0u8;
    for &b in data {
        crc = table[(crc ^ b) as usize];
    }
    crc
}

#[test]
fn crc8_known_answer_power_on_scratchpad() {
    // 電源投入時スクラッチパッドの先頭 8 バイトの CRC は 0x1C。
    let data = [0x50, 0x05, 0x4B, 0x46, 0x7F, 0xFF, 0x0C, 0x10];
    assert_eq!(crc8(&data), 0x1C);
}

#[test]
fn crc8_empty_is_zero() {
    assert_eq!(crc8(&[]), 0x00);
}

#[test]
fn crc8_matches_table_reference_over_many_inputs() {
    // 疑似乱数(線形合同法)で多数の入力を生成し、2 実装が一致することを確認。
    let mut state: u32 = 0x1234_5678;
    for len in 0..64usize {
        let mut buf = [0u8; 64];
        for slot in buf.iter_mut().take(len) {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            *slot = (state >> 24) as u8;
        }
        let data = &buf[..len];
        assert_eq!(crc8(data), crc8_table_reference(data), "len={len}");
    }
}

// ---- ESS / アドバタイズ ----

#[test]
fn ess_uuids_are_standard() {
    assert_eq!(ESS_SERVICE_UUID, 0x181A);
    assert_eq!(TEMPERATURE_CHAR_UUID, 0x2A6E);
}

#[test]
fn advertisement_service_data_layout() {
    // +25.0625℃ → センチ℃ 2506 = 0x09CA
    let ad = advertisement_service_data(Temperature::from_raw(0x0191));
    assert_eq!(ad, [0x05, 0x16, 0x1A, 0x18, 0xCA, 0x09]);
    // length フィールドは後続バイト数と一致する
    assert_eq!(ad[0] as usize, ad.len() - 1);
}
