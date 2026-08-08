//! BLE (trouble-host) による温度提供。
//!
//! 2 通りでスマートフォンへ温度を提供する:
//! 1. **非接続アドバタイズ**: アドバタイズパケットの Service Data(ESS 0x181A)に
//!    最新温度(sint16・センチ℃)を載せる。スキャンのみで取得可能。
//! 2. **接続型 GATT**: Environmental Sensing Service の Temperature 特性(0x2A6E)を
//!    Read / Notify で提供。
//!
//! 最新温度は [`TEMP_CENTI`] を介してセンサタスクから受け取る。
//!
//! Pico W 版(`firmware/src/ble.rs`)と同じロジックだが、ESP32 側はログに
//! `log` クレートを用いる(esp-radio / esp-println が log ベースのため)。

use core::sync::atomic::{AtomicI16, Ordering};
use embassy_futures::join::join;
use embassy_futures::select::{select, Either};
use embassy_time::Timer;
use log::{info, warn};
use pico_temp_core::ess::ESS_SERVICE_UUID;
use trouble_host::prelude::*;

/// 「未測定」を表す番兵値。この値のときはアドバタイズの Service Data に温度 0 を載せる
/// （Service Data 自体は常に付与する）。
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
    // テスト用途に固定の静的ランダムアドレスを用いる。
    // BLE 仕様上、静的ランダムアドレスは最上位バイト(配列末尾)の上位 2bit が 11 である必要がある。
    let address: Address = Address::random([0x42, 0x6d, 0x75, 0x70, 0x69, 0xE3]);
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
    let len = match AdStructure::encode_slice(
        &[
            AdStructure::Flags(LE_GENERAL_DISCOVERABLE | BR_EDR_NOT_SUPPORTED),
            AdStructure::ServiceData16 {
                uuid: ESS_SERVICE_UUID.to_le_bytes(),
                data: &temp_bytes,
            },
            AdStructure::CompleteLocalName(DEVICE_NAME.as_bytes()),
        ],
        &mut adv_data[..],
    ) {
        Ok(len) => len,
        Err(e) => {
            // 例: AD 構造が 31 バイトを超えた等。原因追跡のためログを残す。
            warn!("[adv] encode error: {:?}", e);
            return None;
        }
    };

    let advertiser = match peripheral
        .advertise(
            &Default::default(),
            Advertisement::ConnectableScannableUndirected {
                adv_data: &adv_data[..len],
                scan_data: &[],
            },
        )
        .await
    {
        Ok(advertiser) => advertiser,
        Err(e) => {
            // 無線初期化/設定ミス等の切り分けのためログを残す。
            warn!("[adv] advertise error: {:?}", e);
            return None;
        }
    };

    match select(advertiser.accept(), Timer::after_secs(ADV_REFRESH_SECS)).await {
        Either::First(Ok(conn)) => {
            info!("[adv] connection established");
            match conn.with_attribute_server(server) {
                Ok(gatt_conn) => Some(gatt_conn),
                Err(e) => {
                    // GATT サーバ確立失敗。原因追跡のためログを残す。
                    warn!("[adv] attribute server error: {:?}", e);
                    None
                }
            }
        }
        Either::First(Err(e)) => {
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
        // 未測定時は 0 を格納し、Read が常に最新状態(古い値を返し続けない)になるようにする。
        // アドバタイズの Service Data と同じ扱い。
        let value = if centi == NO_READING { 0 } else { centi };
        // Read 用に値を更新(未購読でも最新値を返せるように)。
        let _ = server.set(&temperature, &value);
        // 購読中の Central へ通知(store=true でテーブルにも反映)。
        if let Err(e) = temperature.notify(conn, &value, true).await {
            warn!("[notify] error: {:?}", e);
        }
        Timer::after_secs(NOTIFY_PERIOD_SECS).await;
    }
}
