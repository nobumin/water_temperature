//! BLE (trouble-host) による温度提供。
//!
//! 2 通りでスマートフォンへ温度を提供する:
//! 1. **非接続アドバタイズ**: アドバタイズパケットの Service Data(ESS 0x181A)に
//!    最新温度(sint16・センチ℃)を載せる。スキャンのみで取得可能。
//! 2. **接続型 GATT**: Environmental Sensing Service の Temperature 特性(0x2A6E)を
//!    Read / Notify で提供。
//!
//! 最新温度は [`TEMP_CENTI`] を介してセンサタスクから受け取る。

use defmt::{info, warn};
use embassy_futures::join::join;
use embassy_futures::select::{select, Either};
use embassy_time::Timer;
use pico_temp_core::ess::ESS_SERVICE_UUID;
use portable_atomic::{AtomicI16, Ordering};
use trouble_host::prelude::*;

/// 「未測定」を表す番兵値。センサ未応答時はアドバタイズに温度を載せない。
pub const NO_READING: i16 = i16::MIN;

/// センサタスクが書き込み、BLE タスクが読み出す最新温度(センチ℃)。
pub static TEMP_CENTI: AtomicI16 = AtomicI16::new(NO_READING);

/// 同時接続数の上限。
const CONNECTIONS_MAX: usize = 1;
/// L2CAP チャネル数(Signal + ATT)。
const L2CAP_CHANNELS_MAX: usize = 2;
/// アドバタイズ時のデバイス名。
const DEVICE_NAME: &str = "PicoTemp";
/// 未接続時にアドバタイズデータを最新温度へ更新する周期(秒)。
const ADV_REFRESH_SECS: u64 = 10;
/// 接続中に温度を Notify する周期(秒)。
const NOTIFY_PERIOD_SECS: u64 = 2;

// GATT サーバ定義。
#[gatt_server]
struct Server {
    ess: EnvironmentalSensingService,
}

/// Environmental Sensing Service。Temperature 特性は sint16・センチ℃。
#[gatt_service(uuid = service::ENVIRONMENTAL_SENSING)]
struct EnvironmentalSensingService {
    #[characteristic(uuid = characteristic::TEMPERATURE, read, notify, value = 0i16)]
    temperature: i16,
}

/// BLE スタックを起動し、アドバタイズ／GATT を永続的に実行する。
pub async fn run<C: Controller>(controller: C) {
    // テスト用途に固定のランダムアドレスを用いる。
    let address: Address = Address::random([0x42, 0x6d, 0x75, 0x70, 0x69, 0x63]);
    info!("BLE address = {:?}", address);

    let mut resources: HostResources<_, DefaultPacketPool, CONNECTIONS_MAX, L2CAP_CHANNELS_MAX> =
        HostResources::new();
    let stack = trouble_host::new(controller, &mut resources)
        .set_random_address(address)
        .build();
    let runner = stack.runner();
    let mut peripheral = stack.peripheral();

    let server = Server::new_with_config(GapConfig::Peripheral(PeripheralConfig {
        name: DEVICE_NAME,
        appearance: &appearance::sensor::GENERIC_SENSOR,
    }))
    .unwrap();

    // BLE ランナーとアプリループを並行実行する。
    let _ = join(ble_task(runner), async {
        loop {
            match advertise(&mut peripheral, &server).await {
                Some(conn) => {
                    // 接続確立時のみ GATT イベント処理と Notify タスクを走らせる。
                    let events = gatt_events_task(&conn);
                    let notify = notify_task(&server, &conn);
                    select(events, notify).await;
                }
                None => {
                    // タイムアウト(未接続)。ループ先頭で最新温度を載せ直して再広告する。
                }
            }
        }
    })
    .await;
}

/// BLE ホストランナー。他の BLE タスクと並行して常時動かす必要がある。
async fn ble_task<C: Controller, P: PacketPool>(mut runner: Runner<'_, C, P>) {
    loop {
        if let Err(e) = runner.run().await {
            let e = defmt::Debug2Format(&e);
            warn!("[ble_task] error: {:?}", e);
        }
    }
}

/// 最新温度をアドバタイズし、接続を待つ。`ADV_REFRESH_SECS` 経過で `None` を返し、
/// 呼び出し側が最新温度で再広告できるようにする。
async fn advertise<'values, 'server, C: Controller>(
    peripheral: &mut Peripheral<'values, C, DefaultPacketPool>,
    server: &'server Server<'values>,
) -> Option<GattConnection<'values, 'server, DefaultPacketPool>> {
    // Service Data に載せる温度(未測定時は 0 を載せる)。
    let centi = TEMP_CENTI.load(Ordering::Relaxed);
    let temp_bytes = if centi == NO_READING {
        [0u8, 0u8]
    } else {
        centi.to_le_bytes()
    };

    let mut adv_data = [0u8; 31];
    let len = AdStructure::encode_slice(
        &[
            AdStructure::Flags(LE_GENERAL_DISCOVERABLE | BR_EDR_NOT_SUPPORTED),
            AdStructure::ServiceData16 {
                uuid: ESS_SERVICE_UUID.to_le_bytes(),
                data: &temp_bytes,
            },
            AdStructure::CompleteLocalName(DEVICE_NAME.as_bytes()),
        ],
        &mut adv_data[..],
    )
    .ok()?;

    let advertiser = peripheral
        .advertise(
            &Default::default(),
            Advertisement::ConnectableScannableUndirected {
                adv_data: &adv_data[..len],
                scan_data: &[],
            },
        )
        .await
        .ok()?;

    match select(advertiser.accept(), Timer::after_secs(ADV_REFRESH_SECS)).await {
        Either::First(Ok(conn)) => {
            info!("[adv] connection established");
            conn.with_attribute_server(server).ok()
        }
        Either::First(Err(e)) => {
            let e = defmt::Debug2Format(&e);
            warn!("[adv] accept error: {:?}", e);
            None
        }
        // タイムアウト: advertiser を drop してアドバタイズを停止し、再広告へ。
        Either::Second(_) => None,
    }
}

/// GATT イベント(Read/Write)を接続が閉じるまで処理する。
async fn gatt_events_task<P: PacketPool>(conn: &GattConnection<'_, '_, P>) -> Result<(), Error> {
    loop {
        match conn.next().await {
            GattConnectionEvent::Disconnected { reason } => {
                info!("[gatt] disconnected: {:?}", reason);
                break;
            }
            GattConnectionEvent::Gatt { event } => {
                // Temperature は Read のみ想定。いずれのイベントも受理して応答する。
                match event.accept() {
                    Ok(reply) => reply.send().await,
                    Err(e) => warn!("[gatt] response error: {:?}", e),
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// 接続中、最新温度を GATT テーブルへ反映しつつ定期 Notify する。
async fn notify_task<P: PacketPool>(
    server: &Server<'_>,
    conn: &GattConnection<'_, '_, P>,
) -> Result<(), Error> {
    let temperature = server.ess.temperature;
    loop {
        let centi = TEMP_CENTI.load(Ordering::Relaxed);
        if centi != NO_READING {
            // Read 用に値を更新(未購読でも最新値を返せるように)。
            let _ = server.set(&temperature, &centi);
            // 購読中の Central へ通知(store=true でテーブルにも反映)。
            if let Err(e) = temperature.notify(conn, &centi, true).await {
                warn!("[notify] error: {:?}", e);
            }
        }
        Timer::after_secs(NOTIFY_PERIOD_SECS).await;
    }
}
