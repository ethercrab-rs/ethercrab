//! Configure Distributed Clocks (DC) for EK1100 and a couple of other modules.
//!
//! Please note this example uses experimental features and should not be used as a reference for
//! other code. It is here (currently) primarily to help develop EtherCrab.

use env_logger::Env;
use ethercrab::{
    DcSync, EtherCrabWireRead, EtherCrabWireWrite, MainDevice, MainDeviceConfig, PduStorage,
    RegisterAddress, Timeouts,
    ds402::{self, Ds402, OpMode, PdoMapping, StatusWord, SyncManagerAssignment},
    error::Error,
    std::ethercat_now,
    subdevice_group::{
        CycleInfo, DcConfiguration, MappingConfig, PdiMappingBikeshedName, TxRxResponse,
    },
};
use futures_lite::StreamExt;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
use ta::Next;
use ta::indicators::ExponentialMovingAverage;

/// Maximum number of SubDevices that can be stored. This must be a power of 2 greater than 1.
const MAX_SUBDEVICES: usize = 16;
const MAX_PDU_DATA: usize = PduStorage::element_size(1100);
const MAX_FRAMES: usize = 32;
const PDI_LEN: usize = 64;

static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();

// This must remain at 1ms to get the drive into OP. The ESI file specifies this value.
const TICK_INTERVAL: Duration = Duration::from_millis(1);

fn main() -> Result<(), Error> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let interface = std::env::args()
        .nth(1)
        .expect("Provide network interface as first argument.");

    log::info!("Starting DS402 demo...");
    log::info!("Run with RUST_LOG=ethercrab=debug or =trace for debug information");

    let (tx, rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");

    let maindevice = Arc::new(MainDevice::new(
        pdu_loop,
        Timeouts {
            wait_loop_delay: Duration::from_millis(5),
            state_transition: Duration::from_secs(10),
            pdu: Duration::from_millis(2000),
            ..Timeouts::default()
        },
        MainDeviceConfig {
            dc_static_sync_iterations: 1000,
            ..MainDeviceConfig::default()
        },
    ));

    let mut tick_interval = smol::Timer::interval(TICK_INTERVAL);

    #[cfg(target_os = "windows")]
    std::thread::spawn(move || {
        ethercrab::std::tx_rx_task_blocking(
            &interface,
            tx,
            rx,
            ethercrab::std::TxRxTaskConfig { spinloop: false },
        )
        .expect("TX/RX task")
    });
    #[cfg(not(target_os = "windows"))]
    smol::spawn(ethercrab::std::tx_rx_task(&interface, tx, rx).expect("spawn TX/RX task")).detach();

    // Wait for TX/RX loop to start
    thread::sleep(Duration::from_millis(200));

    #[cfg(target_os = "linux")]
    thread_priority::set_current_thread_priority(thread_priority::ThreadPriority::Crossplatform(
        thread_priority::ThreadPriorityValue::try_from(48u8).unwrap(),
    ))
    .expect("Main thread prio");

    smol::block_on(async {
        let mut group = maindevice
            .init_single_group::<MAX_SUBDEVICES, PDI_LEN>(ethercat_now)
            .await
            .expect("Init");

        // The group will be in PRE-OP at this point

        // let mut servo = None;

        // for mut subdevice in group.iter_mut(&maindevice) {
        //     if subdevice.name() == "PD4-EB59CD-E-65-1" {
        //         log::info!("Found servo {:?}", subdevice.identity());

        //         // subdevice
        //         //     .sdo_write_array(
        //         //         0x1600,
        //         //         [
        //         //             0x6040_0010u32, // Control word, 16 bits
        //         //             0x6060_0008,    // Op mode, 8 bits
        //         //         ],
        //         //     )
        //         //     .await?;

        //         // subdevice
        //         //     .sdo_write_array(
        //         //         0x1a00,
        //         //         [
        //         //             0x6041_0010u32, // Status word, 16 bits
        //         //             0x6061_0008,    // Op mode reported
        //         //         ],
        //         //     )
        //         //     .await?;

        //         // // Outputs to SubDevice
        //         // subdevice.sdo_write_array(0x1c12, [0x1600u16]).await?;

        //         // // Inputs back to MainDevice
        //         // subdevice.sdo_write_array(0x1c13, [0x1a00u16]).await?;

        //         // // let (inputs_mapping, outputs_mapping) =
        //         // subdevice.set_config(&inputs_config, &outputs_config);

        //         // // Enable SYNC0 DC
        //         // subdevice.set_dc_sync(DcSync::Sync0);
        //     }
        // }

        log::info!("Group has {} SubDevices", group.len());

        let mut averages = Vec::new();

        for _ in 0..group.len() {
            averages.push(ExponentialMovingAverage::new(64).unwrap());
        }

        log::info!("Moving into PRE-OP with PDI");

        // Let's tackle the FMMU/SM config override thing later. Gonna focus on PDI mapping for now.
        // enum SmUsage {
        //     // Direct values from ESI file.
        //     MBoxOut,
        //     MBoxIn,
        //     Outputs,
        //     Inputs,
        // }

        // struct SmConfig {
        //     usage: SmUsage,
        //     size: Range<usize>,
        //     start_addr: u16,
        //     // TODO: A nice way of mapping `ControlByte`. Could I just ignore it and map based on SmUsage?
        //     // NOTE: Ignoring enable flag and just assuming always enabled
        // }

        // // A subset of the information in an ESI file
        // struct SubDeviceConfig<'a> {
        //     sync_managers: &'a [SmUsage],
        //     fmmus: &'a [FmmuConfig],
        //     io: MappingConfig<'a>,
        //     // TODO: Some way of assigning a default SM/FMMU for a `SyncManagerAssignment` of I or O
        //     // based on what the spec says should be the default.
        // }

        let config = MappingConfig::new(
            const {
                &[SyncManagerAssignment::new(
                    const {
                        &[PdoMapping::new(
                            0x1a00,
                            &[ds402::ReadObject::STATUS_WORD, ds402::ReadObject::OP_MODE],
                        )]
                    },
                )]
            },
            const {
                &[SyncManagerAssignment::new(
                    const {
                        &[PdoMapping::new(
                            0x1600,
                            &[
                                ds402::WriteObject::CONTROL_WORD,
                                ds402::WriteObject::OP_MODE,
                            ],
                        )]
                    },
                )]
            },
        );

        let mut servo_mapping = None;

        let group = group
            .into_pre_op_pdi_with_config(&maindevice, async |mut subdevice, idx| {
                if subdevice.name() == "PD4-EB59CD-E-65-1" {
                    log::info!("Found servo {:?}", subdevice.identity());

                    // This is required as the drive won't go into SAFE-OP without the SDOs
                    // configured.
                    config.configure_sdos(&subdevice).await?;

                    subdevice.set_dc_sync(DcSync::Sync0);

                    // Copy config and assign it to the subdevice by configured address. The rest of
                    // the subdevice isn't used here as it doesn't have a configured PDI yet
                    servo_mapping = Some(config.pdi_mapping(&subdevice));

                    // Let EtherCrab configure the SubDevice automatically based on the SDOs we
                    // wrote just above. The SM and FMMU config is read from a well-formed EEPROM.
                    // TODO: Need a way to tell EtherCrab to completely ignore the EEPROM for
                    // SM/FMMU assignment.
                    // TODO: Add a flag or something to tell EtherCrab to write SDO config or not.
                    // This data isn't defined in the ESI files AFAICS so maybe some heuristic like
                    // "has mailbox SM"?
                    Ok(Some(config))
                } else {
                    Ok(None)
                }
            })
            .await?;

        // Now that we have a PDI and the SD has its configured offsets, we can wrap it in the PDI
        // mapping. This mapping is now bound to the subdevice we configured in
        // `into_pre_op_pdi_with_config`. Max 16 I and O mappings
        let servo_mapping: PdiMappingBikeshedName<16, 16> = servo_mapping.expect("No servo");

        let servo = servo_mapping
            .with_subdevice(&maindevice, &group)
            .expect("Didn't find SD in group");

        let servo = Ds402::new(servo).expect("Could not create DS402");

        {
            loop {
                // We can call OG methods on the DS402 device using `.inner()`
                // TODO: Decide if we want to do more deref chaining from `Ds402` ->
                // `PdiMappingBikeshedName` -> `SubDeviceRef` -> `SubDevice` (lol)
                dbg!(servo.inner().configured_address());

                // We can drop down to reading/writing specific IO fields using `inner()`
                // TODO: inner_mut()
                let status: StatusWord = servo
                    .inner()
                    .input(ds402::ReadObject::STATUS_WORD)
                    .expect("No mapping");

                // OR the preferred way: use higher level methods on the DS402 object. This doesn't
                // panic because we made sure the status word mapping was present earlier. It may
                // panic if the PDI isn't configured correctly though (too short, wrong range, etc).
                let status: StatusWord = servo.status_word();

                break;
            }
        }

        let group = group
            .into_safe_op(&maindevice)
            .await
            .expect("OP -> SAFE-OP");

        // This shouldn't be possible because we moved the group!
        // dbg!(servo.configured_address());

        log::info!("OP -> SAFE-OP");

        let group = group
            .into_pre_op(&maindevice)
            .await
            .expect("SAFE-OP -> PRE-OP");

        log::info!("SAFE-OP -> PRE-OP");

        let _group = group.into_init(&maindevice).await.expect("PRE-OP -> INIT");

        log::info!("PRE-OP -> INIT, shutdown complete");

        Ok(())

        // for sd in group.iter(&maindevice) {
        //     log::info!(
        //         "--> {:#06x} PDI {} input bytes, {} output bytes",
        //         sd.configured_address(),
        //         sd.inputs_raw().len(),
        //         sd.outputs_raw().len()
        //     );
        // }

        // log::info!("Done. PDI available. Waiting for SubDevices to align");

        // let mut now = Instant::now();
        // let start = Instant::now();

        // // Repeatedly send group PDI and sync frame to align all SubDevice clocks. We use an
        // // exponential moving average of each SubDevice's deviation from the EtherCAT System Time
        // // (the time in the DC reference SubDevice) and take the maximum deviation. When that is
        // // below 100us (arbitraily chosen value for this demo), we call the sync good enough and
        // // exit the loop.
        // loop {
        //     group
        //         .tx_rx_sync_system_time(&maindevice)
        //         .await
        //         .expect("TX/RX");

        //     if now.elapsed() >= Duration::from_millis(25) {
        //         now = Instant::now();

        //         let mut max_deviation = 0;

        //         for (s1, ema) in group.iter(&maindevice).zip(averages.iter_mut()) {
        //             let diff = match s1
        //                 .register_read::<u32>(RegisterAddress::DcSystemTimeDifference)
        //                 .await
        //             {
        //                 Ok(value) =>
        //                 // The returned value is NOT in two's compliment, rather the upper bit specifies
        //                 // whether the number in the remaining bits is odd or even, so we convert the
        //                 // value to `i32` using that logic here.
        //                 {
        //                     let flag = 0b1u32 << 31;

        //                     if value >= flag {
        //                         // Strip off negative flag bit and negate value as normal
        //                         -((value & !flag) as i32)
        //                     } else {
        //                         value as i32
        //                     }
        //                 }
        //                 Err(Error::WorkingCounter { .. }) => 0,
        //                 Err(e) => return Err(e),
        //             };

        //             let ema_next = ema.next(diff as f64);

        //             max_deviation = max_deviation.max(ema_next.abs() as u32);
        //         }

        //         log::debug!("--> Max deviation {} ns", max_deviation);

        //         // Less than 100us max deviation
        //         if max_deviation < 100_000 {
        //             log::info!("Clocks settled after {} ms", start.elapsed().as_millis());

        //             break;
        //         }
        //     }

        //     tick_interval.next().await;
        // }

        // log::info!("Alignment done");

        // // SubDevice clocks are aligned. We can turn DC on now.
        // let group = group
        //     .configure_dc_sync(
        //         &maindevice,
        //         DcConfiguration {
        //             // Start SYNC0 100ms in the future
        //             start_delay: Duration::from_millis(100),
        //             // SYNC0 period should be the same as the process data loop in most cases
        //             sync0_period: TICK_INTERVAL,
        //             // Taken from ESI file
        //             sync0_shift: Duration::from_nanos(250_000),
        //         },
        //     )
        //     .await?;

        // let group = group
        //     .into_safe_op(&maindevice)
        //     .await
        //     .expect("PRE-OP -> SAFE-OP");

        // log::info!("SAFE-OP");

        // // Request OP state without waiting for all SubDevices to reach it. Allows the immediate
        // // start of the process data cycle, which is required when DC sync is used, otherwise
        // // SubDevices never reach OP, most often timing out with a SyncManagerWatchdog error.
        // let group = group
        //     .request_into_op(&maindevice)
        //     .await
        //     .expect("SAFE-OP -> OP");

        // log::info!("OP requested");

        // let op_request = Instant::now();

        // // Send PDI and check group state until all SubDevices enter OP state. At this point, we can
        // // exit this loop and enter the main process data loop that does not have the state check
        // // overhead present here.
        // loop {
        //     let now = Instant::now();

        //     let response @ TxRxResponse {
        //         working_counter: _wkc,
        //         extra: CycleInfo {
        //             next_cycle_wait, ..
        //         },
        //         ..
        //     } = group.tx_rx_dc(&maindevice).await.expect("TX/RX");

        //     if response.all_op() {
        //         break;
        //     }

        //     smol::Timer::at(now + next_cycle_wait).await;
        // }

        // log::info!(
        //     "All SubDevices entered OP in {} us",
        //     op_request.elapsed().as_micros()
        // );

        // let term = Arc::new(AtomicBool::new(false));
        // signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&term))
        //     .expect("Register hook");

        // let mut sd = group.subdevice(&maindevice, 0)?;

        // // Main application process data cycle
        // loop {
        //     let now = Instant::now();

        //     let TxRxResponse {
        //         working_counter: _wkc,
        //         extra: CycleInfo {
        //             next_cycle_wait, ..
        //         },
        //         ..
        //     } = group.tx_rx_dc(&maindevice).await.expect("TX/RX");

        //     let io = sd.io_raw_mut();

        //     let i = io.inputs();

        //     let status_word = &i[0..2];
        //     let reported_op_mode = &i[2..3];

        //     let mut o = io.outputs();

        //     let control_word = &mut o[0..2];
        //     let op_mode = &mut o[2..3];

        //     OpMode::CyclicSynchronousPosition.pack_to_slice(op_mode)?;

        //     let status_word = StatusWord::unpack_from_slice(status_word)?;
        //     let reported_op_mode = OpMode::unpack_from_slice(reported_op_mode)?;

        //     // println!("Op mode {:?}", reported_op_mode);

        //     smol::Timer::at(now + next_cycle_wait).await;

        //     if term.load(Ordering::Relaxed) {
        //         log::info!("Exiting...");

        //         break;
        //     }
        // }

        // let group = group
        //     .into_safe_op(&maindevice)
        //     .await
        //     .expect("OP -> SAFE-OP");

        // log::info!("OP -> SAFE-OP");

        // let group = group
        //     .into_pre_op(&maindevice)
        //     .await
        //     .expect("SAFE-OP -> PRE-OP");

        // log::info!("SAFE-OP -> PRE-OP");

        // let _group = group.into_init(&maindevice).await.expect("PRE-OP -> INIT");

        // log::info!("PRE-OP -> INIT, shutdown complete");

        // Ok(())
    })
}
