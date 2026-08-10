//! ESP-WROOM-32D (ESP32) 向け DS18B20 温度 BLE ファームウェア。
//!
//! - DS18B20 を 1-Wire(**GPIO4**)で読み、最新温度を [`ble::TEMP_CENTI`] へ格納する。
//! - ESP32 内蔵 Bluetooth を esp-radio の `BleConnector` 経由で使い、
//!   trouble-host でアドバタイズ／GATT を提供する。
//!
//! - **オンデマンド検温**: 常時測定はせず、検温要求([`ble::REQUEST`])を受けてから
//!   [`MEASURE_WINDOW_SECS`] 秒だけ測定する。詳細は `ble.rs` のモジュールコメント参照。
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
use embassy_futures::select::{select, Either};
use embassy_time::{Duration, Instant, Timer};
use esp_hal::clock::CpuClock;
use esp_hal::gpio::Flex;
use esp_hal::interrupt::software::SoftwareInterruptControl;
use esp_hal::timer::timg::TimerGroup;
use esp_radio::ble::controller::BleConnector;
use log::{info, warn};
use trouble_host::prelude::ExternalController;
use {esp_backtrace as _, esp_println as _};

use crate::ble::{NO_READING, READING, REQUEST, TEMP_CENTI};
use crate::onewire::Ds18b20;

esp_bootloader_esp_idf::esp_app_desc!();

/// 検温中の測定周期(秒)。
const SENSOR_PERIOD_SECS: u64 = 2;

/// 検温ウィンドウ(秒)。検温要求を受けてからこの時間だけ測定・送信する。
/// ウィンドウ中に再度要求を受けたら、**その時点から**この時間だけ延長する。
const MEASURE_WINDOW_SECS: u64 = 60;

/// 起動後、最初の測定までの待機時間(ms)。
/// 電源投入直後はセンサが応答せず 1 回目が必ず失敗するため、安定を待ってから始める。
const SENSOR_SETTLE_MS: u64 = 100;

/// esp-radio(BLE)が必要とするヒープサイズ。
const HEAP_SIZE: usize = 72 * 1024;

/// DS18B20 を 1 回測定し、結果を共有変数と [`READING`] へ反映する。
async fn measure_once(sensor: &mut Ds18b20<'_>) {
    match sensor.read().await {
        Ok(temp) => {
            let centi = temp.centi_celsius();
            TEMP_CENTI.store(centi, Ordering::Relaxed);
            READING.signal(centi);
            info!("DS18B20: {} centi-degC", centi);
        }
        Err(e) => {
            TEMP_CENTI.store(NO_READING, Ordering::Relaxed);
            READING.signal(NO_READING);
            warn!("DS18B20 read error: {:?}", e);
        }
    }
}

/// 検温要求を待ち、要求後 [`MEASURE_WINDOW_SECS`] 秒だけ測定を繰り返すタスク。
///
/// 状態遷移:
/// - **IDLE**: [`REQUEST`] を待つ。測定も送信もしない
/// - **MEASURING**: `deadline` まで [`SENSOR_PERIOD_SECS`] 秒ごとに測定
///   - ウィンドウ中に再度要求を受けたら `deadline` をその時点から取り直す(延長)
///   - `deadline` を過ぎたら IDLE へ戻る
///
/// [`REQUEST`] は Signal なので、測定中(最大 750ms のブロッキング)に届いた要求も
/// 取りこぼさず次の `select` で拾える。
#[embassy_executor::task]
async fn sensor_task(pin: Flex<'static>) {
    let mut sensor = Ds18b20::new(pin);
    Timer::after_millis(SENSOR_SETTLE_MS).await;

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

    let window = Duration::from_secs(MEASURE_WINDOW_SECS);
    loop {
        // --- IDLE ---
        REQUEST.wait().await;
        info!("検温開始: {} 秒間 測定します", MEASURE_WINDOW_SECS);

        // --- MEASURING ---
        let mut deadline = Instant::now() + window;
        while Instant::now() < deadline {
            measure_once(&mut sensor).await;
            match select(Timer::after_secs(SENSOR_PERIOD_SECS), REQUEST.wait()).await {
                // 次の測定タイミング。
                Either::First(_) => {}
                // 検温要求の再受信。この時点から測り直す。
                Either::Second(_) => {
                    deadline = Instant::now() + window;
                    info!("検温要求を再受信: ウィンドウを延長しました");
                }
            }
        }
        info!("検温終了: 待受へ戻ります");
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
