//! Raspberry Pi Pico W 向け DS18B20 温度 BLE ファームウェア。
//!
//! - DS18B20 を 1-Wire(GPIO15)で読み、最新温度を [`ble::TEMP_CENTI`] へ格納する。
//! - CYW43439 を初期化し、trouble-host でアドバタイズ／GATT を提供する。
//! - **オンデマンド検温**: 常時測定はせず、検温要求([`ble::REQUEST`])を受けてから
//!   [`MEASURE_WINDOW_SECS`] 秒だけ測定する。詳細は `ble.rs` のモジュールコメント参照。
//!
//! CYW43 ファームウェア blob は `firmware/cyw43-firmware/` に配置する
//! (取得手順は environment.md / scripts/fetch-cyw43-firmware.sh)。
//! blob 無しでコンパイル確認したい場合は `--features skip-cyw43-firmware` を付ける。

#![no_std]
#![no_main]

mod ble;
mod onewire;

#[cfg(not(feature = "skip-cyw43-firmware"))]
use cyw43::aligned_bytes;
use cyw43::Cyw43439;
use cyw43_pio::PioSpi;
use defmt::{info, warn};
use embassy_executor::Spawner;
use embassy_futures::select::{select3, Either3};
use embassy_rp::gpio::{Flex, Level, Output};
use embassy_rp::peripherals::{DMA_CH0, DMA_CH1, PIO0};
use embassy_rp::pio::{InterruptHandler, Pio};
use embassy_rp::{bind_interrupts, dma};
use embassy_time::{Duration, Timer};
use portable_atomic::Ordering;
use static_cell::StaticCell;
use trouble_host::prelude::ExternalController;
use {defmt_rtt as _, panic_probe as _};

use crate::ble::{NO_READING, READING, REQUEST, TEMP_CENTI};
use crate::onewire::Ds18b20;

/// 検温中の測定周期(秒)。
const SENSOR_PERIOD_SECS: u64 = 2;

/// 検温ウィンドウ(秒)。検温要求を受けてからこの時間だけ測定・送信する。
/// ウィンドウ中に再度要求を受けたら、**その時点から**この時間だけ延長する。
const MEASURE_WINDOW_SECS: u64 = 60;

bind_interrupts!(struct Irqs {
    PIO0_IRQ_0 => InterruptHandler<PIO0>;
    DMA_IRQ_0 => dma::InterruptHandler<DMA_CH0>, dma::InterruptHandler<DMA_CH1>;
});

#[embassy_executor::task]
async fn cyw43_task(
    runner: cyw43::Runner<
        'static,
        cyw43::SpiBus<Output<'static>, PioSpi<'static, PIO0, 0>>,
        Cyw43439,
    >,
) -> ! {
    runner.run().await
}

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
///   - ウィンドウ中に再度要求を受けたら、**その要求の受信時刻**を起点に取り直す(延長)
///   - `deadline` に達した時点で即座に IDLE へ戻る
///
/// `deadline` は測定周期と同時に `select3` で待つため、期限到達から測定周期ぶん
/// 遅れて停止することはない。
///
/// [`REQUEST`] は Signal なので、測定中(最大 750ms のブロッキング)に届いた要求も
/// 取りこぼさず次の `select3` で拾える。起点のずれを避けるため、受信時刻は
/// GATT 側で採って Signal に載せている。
///
/// 1 回の要求につき**最低 1 回は測定する**(ウィンドウ判定より前に測定するため)。
#[embassy_executor::task]
async fn sensor_task(pin: Flex<'static>) {
    let mut sensor = Ds18b20::new(pin);
    let window = Duration::from_secs(MEASURE_WINDOW_SECS);
    loop {
        // --- IDLE ---
        let received_at = REQUEST.wait().await;
        info!("検温開始: {} 秒間 測定します", MEASURE_WINDOW_SECS);

        // --- MEASURING ---
        let mut deadline = received_at + window;
        loop {
            measure_once(&mut sensor).await;
            match select3(
                Timer::at(deadline),
                Timer::after_secs(SENSOR_PERIOD_SECS),
                REQUEST.wait(),
            )
            .await
            {
                // ウィンドウ終了。
                Either3::First(_) => break,
                // 次の測定タイミング。
                Either3::Second(_) => {}
                // 検温要求の再受信。受信時刻を起点に測り直す。
                Either3::Third(received_at) => {
                    deadline = received_at + window;
                    info!("検温要求を再受信: ウィンドウを延長しました");
                }
            }
        }
        info!("検温終了: 待受へ戻ります");
    }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());

    // DS18B20 データ線 = GPIO15(外部 4.7kΩ プルアップ必須)。
    let onewire_pin = Flex::new(p.PIN_15);
    spawner.spawn(defmt::unwrap!(sensor_task(onewire_pin)));

    // --- CYW43439 (Wi-Fi/BT) 初期化 ---
    #[cfg(feature = "skip-cyw43-firmware")]
    let (fw, clm, btfw, nvram) = {
        static EMPTY: &cyw43::Aligned<cyw43::A4, [u8]> = &cyw43::Aligned([0u8; 0]);
        (EMPTY, &[] as &[u8], EMPTY, EMPTY)
    };

    #[cfg(not(feature = "skip-cyw43-firmware"))]
    let (fw, clm, btfw, nvram) = {
        // firmware/cyw43-firmware/ に配置した blob を埋め込む。
        let fw = aligned_bytes!("../cyw43-firmware/43439A0.bin");
        let clm = aligned_bytes!("../cyw43-firmware/43439A0_clm.bin");
        let btfw = aligned_bytes!("../cyw43-firmware/43439A0_btfw.bin");
        let nvram = aligned_bytes!("../cyw43-firmware/nvram_rp2040.bin");
        (fw, clm, btfw, nvram)
    };

    let pwr = Output::new(p.PIN_23, Level::Low);
    let cs = Output::new(p.PIN_25, Level::High);
    let mut pio = Pio::new(p.PIO0, Irqs);
    let spi = PioSpi::new(
        &mut pio.common,
        pio.sm0,
        cyw43_pio::DEFAULT_CLOCK_DIVIDER,
        pio.irq0,
        cs,
        p.PIN_24,
        p.PIN_29,
        dma::Channel::new(p.DMA_CH0, Irqs),
        dma::Channel::new(p.DMA_CH1, Irqs),
    );

    static STATE: StaticCell<cyw43::State> = StaticCell::new();
    let state = STATE.init(cyw43::State::new());
    let (_net_device, bt_device, mut control, runner) =
        cyw43::new_with_bluetooth(state, pwr, spi, fw, btfw, nvram).await;
    spawner.spawn(defmt::unwrap!(cyw43_task(runner)));
    control.init(clm).await;

    let controller: ExternalController<_, 10> = ExternalController::new(bt_device);

    info!("Starting BLE (advertise + GATT ESS)");
    ble::run(controller).await;
}
