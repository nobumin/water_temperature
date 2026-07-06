//! BLE Environmental Sensing に関するエンコード。
//!
//! - Environmental Sensing Service (ESS): UUID `0x181A`
//! - Temperature 特性: UUID `0x2A6E`、型 `sint16`、単位 0.01 ℃(センチ℃)
//!
//! スマートフォンは 2 通りで温度を取得できる:
//! 1. **非接続アドバタイズ**: 温度を Service Data (AD type `0x16`) に載せ、
//!    スキャンのみで取得(本モジュールの [`advertisement_service_data`])。
//! 2. **接続型 GATT**: ESS の Temperature 特性を Read/Notify で取得
//!    (値のバイト列は [`Temperature::to_ess_bytes`])。

use crate::ds18b20::Temperature;

/// Environmental Sensing Service の 16bit UUID。
pub const ESS_SERVICE_UUID: u16 = 0x181A;

/// Temperature 特性の 16bit UUID。
pub const TEMPERATURE_CHAR_UUID: u16 = 0x2A6E;

/// アドバタイズパケットに埋め込む「Service Data - 16bit UUID」AD 構造の型。
const AD_TYPE_SERVICE_DATA_16BIT: u8 = 0x16;

/// アドバタイズ用 Service Data AD 構造(6 バイト)を組み立てる。
///
/// レイアウト(BLE Core: AD structure = `length` + `data`):
///
/// | Offset | 値            | 意味 |
/// | ------ | ------------- | ---- |
/// | 0      | `0x05`        | 後続バイト数(type1 + UUID2 + 温度2 = 5) |
/// | 1      | `0x16`        | AD type: Service Data - 16bit UUID |
/// | 2-3    | `0x1A 0x18`   | ESS UUID (0x181A, リトルエンディアン) |
/// | 4-5    | 温度 2 バイト | sint16・センチ℃・リトルエンディアン |
///
/// この 6 バイトをアドバタイズ／スキャンレスポンスのペイロードへ連結する。
pub const fn advertisement_service_data(temp: Temperature) -> [u8; 6] {
    let uuid = ESS_SERVICE_UUID.to_le_bytes();
    let value = temp.to_ess_bytes();
    [
        0x05,
        AD_TYPE_SERVICE_DATA_16BIT,
        uuid[0],
        uuid[1],
        value[0],
        value[1],
    ]
}
