//! ESP-WROOM-32D (ESP32) 向け DS18B20 温度 BLE ファームウェア。
//!
//! - DS18B20 を 1-Wire(**GPIO4**)で読み、最新温度を [`ble::TEMP_CENTI`] へ格納する。
//! - ESP32 内蔵 Bluetooth を esp-radio の `BleConnector` 経由で使い、
//!   trouble-host でアドバタイズ／GATT を提供する。
//!
//! Pico W 版と異なり **CYW43 ファームウェア blob は不要**（BT がチップ内蔵のため）。
//!
//! ## GPIO4 を使う理由
//! ESP32 の GPIO15 はストラッピングピン(MTDO)で、DS18B20 に必要な 4.7kΩ プルアップが
//! 起動時の挙動へ影響する。GPIO34〜39 は入力専用で 1-Wire に使えない。
//! そのため安全に使える GPIO4 を既定とする（詳細は specification.md）。

#![no_std]
#![no_main]

mod ble;
mod onewire;

use core::sync::atomic::Ordering;
use embassy_executor::Spawner;
use esp_hal::clock::CpuClock;
use esp_hal::gpio::Flex;
use esp_hal::interrupt::software::SoftwareInterruptControl;
use esp_hal::timer::timg::TimerGroup;
use esp_radio::ble::controller::BleConnector;
use log::{info, warn};
use trouble_host::prelude::ExternalController;
use {esp_backtrace as _, esp_println as _};

use crate::ble::{NO_READING, TEMP_CENTI};
use crate::onewire::Ds18b20;

esp_bootloader_esp_idf::esp_app_desc!();

/// DS18B20 の測定周期(秒)。
const SENSOR_PERIOD_SECS: u64 = 2;

/// 起動後、最初の測定までの待機時間(ms)。
/// 電源投入直後はセンサが応答せず 1 回目が必ず失敗するため、安定を待ってから始める。
const SENSOR_SETTLE_MS: u64 = 100;

/// esp-radio(BLE)が必要とするヒープサイズ。
const HEAP_SIZE: usize = 72 * 1024;

/// DS18B20 を定期測定し、最新温度(センチ℃)を共有変数へ書き込むタスク。
#[embassy_executor::task]
async fn sensor_task(pin: Flex<'static>) {
    let mut sensor = Ds18b20::new(pin);
    embassy_time::Timer::after_millis(SENSOR_SETTLE_MS).await;

    // 起動時セルフテスト。Read ROM が通れば「配線と 1-Wire タイミングは正常」と確定でき、
    // 以降 CRC エラーが続く場合に変換シーケンス側の問題だと切り分けられる。
    match sensor.read_rom() {
        Ok(rom) if rom[0] == onewire::FAMILY_CODE => {
            info!("DS18B20 ROM = {:02X?} (family code OK)", rom);
        }
        Ok(rom) => {
            warn!(
                "DS18B20 ROM = {:02X?} (family code {:#04X}: DS18B20 は 0x28)",
                rom, rom[0]
            );
        }
        Err(e) => warn!("DS18B20 ROM read error: {:?}", e),
    }

    loop {
        match sensor.read().await {
            Ok(temp) => {
                let centi = temp.centi_celsius();
                TEMP_CENTI.store(centi, Ordering::Relaxed);
                info!("DS18B20: {} centi-degC", centi);
            }
            Err(e) => {
                TEMP_CENTI.store(NO_READING, Ordering::Relaxed);
                warn!("DS18B20 read error: {:?}", e);
            }
        }
        embassy_time::Timer::after_secs(SENSOR_PERIOD_SECS).await;
    }
}

#[esp_rtos::main]
async fn main(spawner: Spawner) {
    esp_println::logger::init_logger_from_env();

    let peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));
    esp_alloc::heap_allocator!(size: HEAP_SIZE);

    // esp-rtos のスケジューラを起動する(embassy のタイムドライバもここで有効になる)。
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let sw_int = SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, sw_int.software_interrupt0);

    // DS18B20 データ線 = GPIO4(外部 4.7kΩ プルアップ必須)。
    let onewire_pin = Flex::new(peripherals.GPIO4);
    spawner.spawn(sensor_task(onewire_pin).unwrap());

    // --- BLE (ESP32 内蔵 Bluetooth) ---
    let connector = BleConnector::new(peripherals.BT, Default::default()).unwrap();
    let controller: ExternalController<_, 20> = ExternalController::new(connector);

    info!("Starting BLE (advertise + GATT ESS)");
    ble::run(controller).await;
}
