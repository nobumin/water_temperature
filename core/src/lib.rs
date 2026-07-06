//! # pico-temp-core
//!
//! DS18B20 温度センサと BLE Environmental Sensing に関する、
//! **ハードウェア非依存**の中核ロジックを提供するクレートです。
//!
//! ここには I/O やペリフェラル操作を一切含めません。純粋な値変換のみを扱うため、
//! ホスト上で `cargo test` によりユニットテストが可能です（実機不要）。
//! firmware クレートはこのクレートを呼び出して、1-Wire で読んだ生バイト列を
//! 温度へ変換し、BLE のアドバタイズ／GATT ペイロードを組み立てます。
//!
//! ## モジュール構成
//! - [`ds18b20`]: スクラッチパッド(9バイト)の解析、CRC8 検証、温度変換
//! - [`ess`]: BLE Environmental Sensing Service の温度エンコードとアドバタイズ組立て

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

pub mod ds18b20;
pub mod ess;

pub use ds18b20::{crc8, Ds18b20Error, Scratchpad, Temperature};
pub use ess::{advertisement_service_data, ESS_SERVICE_UUID, TEMPERATURE_CHAR_UUID};
