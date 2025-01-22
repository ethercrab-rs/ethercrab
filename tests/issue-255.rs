mod util;

use ethercrab::{error::Error, MainDevice, MainDeviceConfig, PduStorage, Timeouts};
use std::{
    hint::black_box,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::time::MissedTickBehavior;

const MAX_SUBDEVICES: usize = 16;
const MAX_PDU_DATA: usize = PduStorage::element_size(1100);
const MAX_FRAMES: usize = 128;
const MAX_PDI: usize = 128;

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn check_issue_255() -> Result<(), Error> {
    if option_env!("CI").is_some() {
        // This test is designed for miri and is skipped in CI because it is obscenely slow

        return Ok(());
    }

    #[cfg(not(miri))]
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    #[cfg(miri)]
    simple_logger::init_with_level(log::Level::Debug).unwrap();

    static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();

    let (tx, rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");

    let mut maindevice = Arc::new(MainDevice::new(
        pdu_loop,
        Timeouts::default(),
        MainDeviceConfig {
            dc_static_sync_iterations: 0,
            ..Default::default()
        },
    ));

    let mut cache_filename = PathBuf::from(file!());
    cache_filename.set_extension("miri-cache");

    util::spawn_tx_rx_for_miri(
        include_bytes!("./replay-issue-255.pcapng").as_slice(),
        tx,
        rx,
        #[cfg(miri)]
        Some(include_bytes!("./issue-255.miri-cache")),
        #[cfg(not(miri))]
        None,
        cache_filename,
    );

    log::info!("Begin network init");

    // Read configurations from SubDevice EEPROMs and configure devices.
    let group = Arc::get_mut(&mut maindevice)
        .unwrap()
        .init_single_group::<MAX_SUBDEVICES, MAX_PDI>(time_zero)
        .await
        .expect("Init");

    log::info!("Init done");

    let group = group.into_op(&maindevice).await.expect("Slow into OP");

    log::info!("Group is in OP");

    let mut cycle_time = tokio::time::interval(Duration::from_millis(1));
    cycle_time.set_missed_tick_behavior(MissedTickBehavior::Skip);

    // let mut cycle_time = smol::Timer::interval(Duration::from_millis(2));

    let group = Arc::new(group);

    let stop = Arc::new(AtomicBool::new(false));

    let group2 = group.clone();
    let maindevice2 = maindevice.clone();
    let stop2 = stop.clone();

    let handle = std::thread::spawn(move || {
        cassette::block_on({
            async move {
                log::info!("Start TX/RX task");

                for _ in 0..64 {
                    group2.tx_rx(&maindevice2).await.expect("TX/RX failure");

                    std::thread::sleep(Duration::from_micros(50));
                    // std::thread::yield_now();
                }

                stop2.store(true, Ordering::Release);
            }
        })
    });

    // Animate slow pattern for 8 ticks
    while !stop.load(Ordering::Acquire) && !handle.is_finished() {
        // IMPORTANT: Use a block to make sure we drop the group write guard as soon as possible
        {
            let el2889 = group.subdevice(&maindevice, 1).unwrap();

            let mut o = el2889.outputs_raw_mut();

            black_box(do_stuff(black_box(&mut o)));
        }

        // std::thread::sleep(Duration::from_millis(1));
        std::thread::sleep(Duration::from_micros(50));
        // std::thread::yield_now();

        cycle_time.tick().await;
        // cycle_time.next().await;
    }

    Ok(())
}

fn time_zero() -> u64 {
    0
}

fn do_stuff(slice: &mut [u8]) {
    slice[0] += 1;
    slice[0] -= 1;
}
