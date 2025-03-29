//! Distributed clocks initialisation test.
//!
//! Required hardware:
//!
//! - EK1100
//! - EL2828
//! - EL2889

mod util;

use env_logger::Env;
use ethercrab::{
    DcSync, MainDevice, MainDeviceConfig, PduStorage, RegisterAddress, Timeouts, TxRxResponse,
    error::Error,
    subdevice_group::{CycleInfo, DcConfiguration},
};
use std::{path::PathBuf, time::Duration};

const MAX_SUBDEVICES: usize = 16;
const MAX_PDU_DATA: usize = PduStorage::element_size(1100);
const MAX_FRAMES: usize = 128;
const PDI_LEN: usize = 64;

const TICK_INTERVAL: Duration = Duration::from_millis(5);

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn replay_dc() -> Result<(), Error> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();

    let (tx, rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");

    let maindevice = MainDevice::new(
        pdu_loop,
        Timeouts::default(),
        MainDeviceConfig {
            dc_static_sync_iterations: 100,
            ..Default::default()
        },
    );

    let test_name = PathBuf::from(file!())
        .file_stem()
        .unwrap()
        .to_string_lossy()
        .to_string();

    util::spawn_tx_rx(&format!("tests/{test_name}.pcapng"), tx, rx);

    let mut tick_interval = tokio::time::interval(TICK_INTERVAL);

    let mut group = maindevice
        // EtherCAT time is always 0 for this test
        .init_single_group::<MAX_SUBDEVICES, PDI_LEN>(|| 0)
        .await
        .expect("Init");

    for mut subdevice in group.iter_mut(&maindevice) {
        subdevice.set_dc_sync(DcSync::Sync0);
    }

    let group = group.into_pre_op_pdi(&maindevice).await?;

    log::info!("Group in PREOP");

    // Repeatedly send group PDI and sync frame to align all SubDevice clocks
    loop {
        group
            .tx_rx_sync_system_time(&maindevice)
            .await
            .expect("TX/RX");

        let mut max_deviation = 0;

        for sd in group.iter(&maindevice) {
            let diff = match sd
                .register_read::<u32>(RegisterAddress::DcSystemTimeDifference)
                .await
            {
                Ok(value) =>
                // The returned value is NOT in two's compliment, rather the upper bit specifies
                // whether the number in the remaining bits is odd or even, so we convert the
                // value to `i32` using that logic here.
                {
                    let flag = 0b1u32 << 31;

                    if value >= flag {
                        // Strip off negative flag bit and negate value as normal
                        -((value & !flag) as i32)
                    } else {
                        value as i32
                    }
                }
                Err(Error::WorkingCounter { .. }) => 0,
                Err(e) => return Err(e),
            };

            max_deviation = max_deviation.max(diff as u32);
        }

        // 100k us
        if max_deviation < 100_000 {
            break;
        }

        tick_interval.tick().await;
    }

    log::info!("Clocks aligned");

    // SubDevice clocks are aligned. We can turn DC on now.
    let group = group
        .configure_dc_sync(
            &maindevice,
            DcConfiguration {
                // Start SYNC0 100ms in the future
                start_delay: Duration::from_millis(100),
                // SYNC0 period should be the same as the process data loop in most cases
                sync0_period: TICK_INTERVAL,
                // Send process data half way through cycle
                sync0_shift: TICK_INTERVAL / 2,
            },
        )
        .await?;

    let group = group
        .into_safe_op(&maindevice)
        .await
        .expect("PRE-OP -> SAFE-OP");

    log::info!("SAFE-OP");

    let group = group
        .request_into_op(&maindevice)
        .await
        .expect("SAFE-OP -> OP");

    // Wait for all OP while sending PDI and DC sync frames
    loop {
        let response @ TxRxResponse {
            working_counter: _wkc,
            extra: CycleInfo {
                next_cycle_wait, ..
            },
            ..
        } = group.tx_rx_dc(&maindevice).await.expect("TX/RX");

        if response.all_op() {
            break;
        }

        tokio::time::sleep(next_cycle_wait).await;
    }

    log::info!("All SubDevices entered OP");

    // Main application process data cycle
    for i in 0..u8::MAX {
        let TxRxResponse {
            working_counter: _wkc,
            extra: CycleInfo {
                next_cycle_wait, ..
            },
            ..
        } = group.tx_rx_dc(&maindevice).await.expect("TX/RX");

        for subdevice in group.iter(&maindevice) {
            let mut o = subdevice.outputs_raw_mut();

            for byte in o.iter_mut() {
                *byte = i;
            }
        }

        tokio::time::sleep(next_cycle_wait).await;
    }

    let group = group
        .into_safe_op(&maindevice)
        .await
        .expect("OP -> SAFE-OP");

    log::info!("OP -> SAFE-OP");

    let group = group
        .into_pre_op(&maindevice)
        .await
        .expect("SAFE-OP -> PRE-OP");

    log::info!("SAFE-OP -> PRE-OP");

    let _group = group.into_init(&maindevice).await.expect("PRE-OP -> INIT");

    log::info!("PRE-OP -> INIT, shutdown complete");

    Ok(())
}
