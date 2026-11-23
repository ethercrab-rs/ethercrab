//! Demonstrate slowly spinning an AM8051-0EH1-0000 motor with an AX5203 drive.
//!
//! Uses newly implemented SoE functionality, namely writing to IDNs (SoE equivalent of CoE SDOs),
//! and SoE derived PDO setup.
//!
//! IMPORTANT: This examples uses a hard-coded function to initialize the IDNs for the motor, brake,
//! and encoder type for this particular combination of hardware. If you intend to use this, you MUST
//! modify / regenerate this startup list for your particular hardware combination!

use env_logger::Env;
use ethercrab::{
    DcSync, MainDevice, MainDeviceConfig, PduStorage, RegisterAddress, Timeouts,
    error::Error,
    idn,
    std::ethercat_now,
    subdevice_group::{CycleInfo, DcConfiguration, TxRxResponse},
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
const PDI_LEN: usize = 128;

static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();

const TICK_INTERVAL: Duration = Duration::from_micros(2000);

fn main() -> Result<(), Error> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let interface = std::env::args()
        .nth(1)
        .expect("Provide network interface as first argument.");

    log::info!("Starting AX5000 SoE demo...");
    log::info!("Run with RUST_LOG=ethercrab=debug or =trace for debug information");

    let (tx, rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");

    let maindevice = Arc::new(MainDevice::new(
        pdu_loop,
        Timeouts {
            wait_loop_delay: TICK_INTERVAL,
            state_transition: Duration::from_secs(20),
            pdu: Duration::from_millis(2000),
            ..Timeouts::default()
        },
        MainDeviceConfig {
            dc_static_sync_iterations: 10_000,
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
    {
        // Set thread priority to realtime
        use thread_priority::{
            RealtimeThreadSchedulePolicy, ThreadPriority, ThreadPriorityValue,
            ThreadSchedulePolicy, set_thread_priority_and_policy, thread_native_id,
        };
        let thread_id = thread_native_id();
        set_thread_priority_and_policy(
            thread_id,
            ThreadPriority::Crossplatform(ThreadPriorityValue::try_from(49u8).unwrap()),
            ThreadSchedulePolicy::Realtime(RealtimeThreadSchedulePolicy::Fifo),
        )
        .unwrap_or_else(|_| {
            log::warn!(
                "Could not set realtime thread priority. Are the PREEMPT_RT patches in use?"
            );

            thread_priority::set_current_thread_priority(
                thread_priority::ThreadPriority::Crossplatform(
                    thread_priority::ThreadPriorityValue::try_from(48u8).unwrap(),
                ),
            )
            .expect("Failed to set thread priority at all!");
        })
    }

    smol::block_on(async {
        let mut group = maindevice
            .init_single_group::<MAX_SUBDEVICES, PDI_LEN>(ethercat_now)
            .await
            .expect("Init");

        // The group will be in PRE-OP at this point
        for mut subdevice in group.iter_mut(&maindevice) {
            if subdevice.name() == "AX5203-0000-0216" {
                log::info!("Begin IDN configuration");
                // Begin XML config
                transition_ps(&subdevice).await?;
                // End XML config
                log::info!("End IDN configuration");
            }

            log::info!("Setting DC Sync0");
            subdevice.set_dc_sync(DcSync::Sync01 {
                sync1_period: Duration::from_micros(1750),
            });
        }

        log::info!("Group has {} SubDevices", group.len());

        let mut averages = Vec::new();

        for _ in 0..group.len() {
            averages.push(ExponentialMovingAverage::new(64).unwrap());
        }

        log::info!("Moving into PRE-OP with PDI");

        let group = group.into_pre_op_pdi(&maindevice).await?;

        log::info!("Done. PDI available. Waiting for SubDevices to align");

        for subdevice in group.iter(&maindevice) {
            let io = subdevice.io_raw();

            log::info!(
                "-> SubDevice {:#06x} {} inputs: {} bytes, outputs: {} bytes",
                subdevice.configured_address(),
                subdevice.name(),
                io.inputs().len(),
                io.outputs().len()
            );
        }

        let mut now = Instant::now();
        let start = Instant::now();

        // Repeatedly send group PDI and sync frame to align all SubDevice clocks. We use an
        // exponential moving average of each SubDevice's deviation from the EtherCAT System Time
        // (the time in the DC reference SubDevice) and take the maximum deviation. When that is
        // below 100ns (arbitraily chosen value for this demo), we call the sync good enough and
        // exit the loop.
        loop {
            group
                .tx_rx_sync_system_time(&maindevice)
                .await
                .expect("TX/RX");

            let mut max_deviation = 0;

            for (s1, ema) in group.iter(&maindevice).zip(averages.iter_mut()) {
                let diff = match s1
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

                let ema_next = ema.next(diff as f64);

                max_deviation = max_deviation.max(ema_next.abs() as u32);
            }

            if now.elapsed() >= Duration::from_millis(1000) {
                now = Instant::now();

                log::info!("--> Max deviation {} ns", max_deviation);

                // Less than 500ns max deviation as an example threshold.
                // <https://github.com/OpenEtherCATsociety/SOEM/issues/487#issuecomment-786245585>
                // mentions less than 100us as a good enough value as well.
                if max_deviation < 500 {
                    log::info!("Clocks settled after {} ms", start.elapsed().as_millis());
                    break;
                }
            }

            tick_interval.next().await;
        }

        log::info!("Alignment done");

        log::info!("Configuring DC Sync for group...");
        // SubDevice clocks are aligned. We can turn DC on now.
        let group = group
            .configure_dc_sync(
                &maindevice,
                DcConfiguration {
                    // Start SYNC0 100ms in the future
                    start_delay: Duration::from_millis(100),
                    // SYNC0 period differs from the process period here
                    // since SYNC1 takes up the slack 1750 us (set previously)
                    sync0_period: Duration::from_micros(250),
                    // Send process data half way through cycle
                    sync0_shift: TICK_INTERVAL / 2,
                },
            )
            .await?;

        for subdevice in group.iter(&maindevice) {
            if subdevice.name() == "AX5203-0000-0216" {
                subdevice
                    .register_write(RegisterAddress::DcCyclicUnitControl, 0x30u8)
                    .await?;
            }
        }
        log::info!("DC Sync configured for group.");

        log::info!("Requesting transition to SAFE-OP...");
        let group = group
            .into_safe_op(&maindevice)
            .await
            .expect("PRE-OP -> SAFE-OP");

        log::info!("SAFE-OP");

        // Request OP state without waiting for all SubDevices to reach it. Allows the immediate
        // start of the process data cycle, which is required when DC sync is used, otherwise
        // SubDevices never reach OP, most often timing out with a SyncManagerWatchdog error.
        let group = group
            .request_into_op(&maindevice)
            .await
            .expect("SAFE-OP -> OP");

        log::info!("OP requested");

        let op_request = Instant::now();

        // Send PDI and check group state until all SubDevices enter OP state. At this point, we can
        // exit this loop and enter the main process data loop that does not have the state check
        // overhead present here.
        loop {
            let now = Instant::now();

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

            smol::Timer::at(now + next_cycle_wait).await;
        }

        log::info!(
            "All SubDevices entered OP in {} us",
            op_request.elapsed().as_micros()
        );

        let term = Arc::new(AtomicBool::new(false));
        signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&term))
            .expect("Register hook");
        let mut exit_request = false;
        let mut ready_to_exit = false;

        // Initialize position command to the current position to prevent jumps
        let mut position_command: i32 = group
            .subdevice(&maindevice, 0)?
            .idn_read_data(0, idn!(S, 0, 0051))
            .await?;
        let mut control_word: u16;

        // Main application process data cycle
        loop {
            group.tx_rx_dc(&maindevice).await.expect("TX/RX");

            for subdevice in group.iter(&maindevice) {
                if subdevice.name() == "AX5203-0000-0216" {
                    if exit_request {
                        control_word = 0;
                    } else {
                        // Bit 13 - Restart, Bit 14 - Enable, Bit 15 - Drive on
                        control_word = 0b11100000_00000000;
                    }

                    {
                        // AT structure is
                        // Bytes 0-1:   u16, status word (S-0-0135)
                        // Bytes 2-5:   i32, position feedback (S-0-0051)
                        // Bytes 6-9:   i32, following distance (S-0-0189)
                        // Bytes 10-11: i16, torque feedback (S-0-0084)
                        let inputs = subdevice.inputs_raw();

                        let status_word: u16 =
                            u16::from_le_bytes(inputs[0..=1].try_into().unwrap());
                        let _position_feedback: i32 =
                            i32::from_le_bytes(inputs[2..=5].try_into().unwrap());
                        let _following_distance: i32 =
                            i32::from_le_bytes(inputs[6..=9].try_into().unwrap());
                        let _torque_feedback: i16 =
                            i16::from_le_bytes(inputs[10..=11].try_into().unwrap());

                        // Bit 3 - Drive observing values
                        if status_word & 0b00000000_00001000 != 0 {
                            // Tested on a motor in 20 bit encoder resolution with 8:1 gearbox
                            // This will roughly result in a 6 rpm on the output shaft for this setup
                            position_command += 200 * 8;
                        }

                        // If exit has been requested, check if power is off
                        if exit_request && (status_word >> 14 < 3) {
                            ready_to_exit = true;
                        }

                        // // Leave this commented out to avoid jitter issues
                        // log::info!(
                        //     "{status_word:0b}: {position_feedback}\t {following_distance}" //\t {torque_feedback}"
                        // )
                    }

                    {
                        // MDT structure is:
                        // Bytes 0-1: u16, control word (S-0-0134)
                        // Bytes 2-5: i32, position command (S-0-0047)
                        let mut o = subdevice.outputs_raw_mut();

                        o[0..2].copy_from_slice(&control_word.to_le_bytes());
                        o[2..6].copy_from_slice(&position_command.to_le_bytes());
                    }
                }
            }

            // smol::Timer::at(now + next_cycle_wait).await;
            tick_interval.next().await;

            // Hook signal so we can write CSV data before exiting
            if term.load(Ordering::Relaxed) && !exit_request {
                log::info!("Exiting...");
                exit_request = true;
            }

            if ready_to_exit {
                log::info!("Ready to exit.");
                break;
            }
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
    })
}

// Translated via a script from a TwinCAT startup list XML file -- values will vary by motor and encoder type!!!
pub async fn transition_ps(
    subdevice: &ethercrab::SubDeviceRef<'_, &mut ethercrab::SubDevice>,
) -> Result<(), Error> {
    //Feature flags
    subdevice
        .idn_write_data(
            0,
            32778u16,
            [
                0xffu8, 0xffu8, 0xf9u8, 0x07u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8,
            ],
        )
        .await?;
    //Feature flags
    subdevice
        .idn_write_data(
            1,
            32778u16,
            [
                0xffu8, 0xffu8, 0xf9u8, 0x07u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8,
            ],
        )
        .await?;
    //Telegram type
    subdevice.idn_write_data(0, 15u16, [0x07u8, 0x00u8]).await?;
    //AT list
    subdevice
        .idn_write_data(
            0,
            16u16,
            [
                0x06u8, 0x00u8, 0x06u8, 0x00u8, 0x33u8, 0x00u8, 0xbdu8, 0x00u8, 0x54u8, 0x00u8,
            ],
        )
        .await?;
    //Telegram type
    subdevice.idn_write_data(1, 15u16, [0x07u8, 0x00u8]).await?;
    //AT list
    subdevice
        .idn_write_data(
            1,
            16u16,
            [
                0x06u8, 0x00u8, 0x06u8, 0x00u8, 0x33u8, 0x00u8, 0xbdu8, 0x00u8, 0x54u8, 0x00u8,
            ],
        )
        .await?;
    //MDT list
    subdevice
        .idn_write_data(0, 24u16, [0x02u8, 0x00u8, 0x02u8, 0x00u8, 0x2fu8, 0x00u8])
        .await?;
    //MDT list
    subdevice
        .idn_write_data(1, 24u16, [0x02u8, 0x00u8, 0x02u8, 0x00u8, 0x2fu8, 0x00u8])
        .await?;
    //Tncyc - NC cycle time
    subdevice.idn_write_data(0, 1u16, [0xd0u8, 0x07u8]).await?;
    //Tscyc - Comm cycle time
    subdevice.idn_write_data(0, 2u16, [0xd0u8, 0x07u8]).await?;
    //Power management control word
    subdevice
        .idn_write_data(0, 32972u16, [0x09u8, 0x08u8])
        .await?;
    //Nominal mains voltage
    subdevice
        .idn_write_data(0, 32969u16, 2080u16) //[0xd0u8, 0x07u8])
        .await?;
    //Mains voltage negative tolerance range
    subdevice
        .idn_write_data(0, 32971u16, [0x64u8, 0x00u8])
        .await?;
    //Mains voltage positive tolerance range
    subdevice
        .idn_write_data(0, 32970u16, [0x64u8, 0x00u8])
        .await?;
    //Max DC link voltage
    subdevice
        .idn_write_data(0, 32984u16, [0x2eu8, 0x22u8])
        .await?;
    //Operation mode
    subdevice.idn_write_data(0, 32u16, [0x0bu8, 0x00u8]).await?;
    //Configured motor type
    subdevice
        .idn_write_data(
            0,
            32821u16,
            [
                0x0cu8, 0x00u8, 0x1eu8, 0x00u8, 0x41u8, 0x4du8, 0x38u8, 0x30u8, 0x35u8, 0x31u8,
                0x2du8, 0x78u8, 0x45u8, 0x78u8, 0x31u8, 0x00u8,
            ],
        )
        .await?;
    //Number of pole pairs
    subdevice
        .idn_write_data(0, 32819u16, [0x04u8, 0x00u8])
        .await?;
    //Mechanical motor data
    subdevice
        .idn_write_data(
            0,
            32839u16,
            [
                0x08u8, 0x00u8, 0x08u8, 0x00u8, 0x22u8, 0x01u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8,
                0x00u8, 0x00u8,
            ],
        )
        .await?;
    //Electrical commutation offset
    subdevice
        .idn_write_data(0, 32825u16, [0x78u8, 0x69u8])
        .await?;
    //Motor continuous stall current
    subdevice
        .idn_write_data(0, 111u16, [0x8cu8, 0x0au8, 0x00u8, 0x00u8])
        .await?;
    //Motor rated current
    subdevice
        .idn_write_data(0, 196u16, [0xf6u8, 0x09u8, 0x00u8, 0x00u8])
        .await?;
    //Motor peak current
    subdevice
        .idn_write_data(0, 109u16, [0x44u8, 0x2fu8, 0x00u8, 0x00u8])
        .await?;
    //Motor rated voltage
    subdevice
        .idn_write_data(0, 32845u16, [0xa0u8, 0x0fu8])
        .await?;
    //Motor winding: Dielectric strength
    subdevice
        .idn_write_data(0, 32835u16, [0xc4u8, 0x22u8])
        .await?;
    //Electric motor model
    subdevice
        .idn_write_data(
            0,
            32834u16,
            [
                0x08u8, 0x00u8, 0x08u8, 0x00u8, 0x74u8, 0x04u8, 0x00u8, 0x00u8, 0xaeu8, 0x10u8,
                0x00u8, 0x00u8,
            ],
        )
        .await?;
    //Motor warning temperature
    subdevice
        .idn_write_data(0, 201u16, [0xb0u8, 0x04u8])
        .await?;
    //Motor shut down temperature
    subdevice
        .idn_write_data(0, 204u16, [0x78u8, 0x05u8])
        .await?;
    //Motor EMF
    subdevice
        .idn_write_data(0, 32823u16, [0xe2u8, 0x04u8])
        .await?;
    //Thermal motor model
    subdevice
        .idn_write_data(
            0,
            32830u16,
            [
                0x10u8, 0x00u8, 0x10u8, 0x00u8, 0x44u8, 0x07u8, 0x50u8, 0x00u8, 0x64u8, 0x00u8,
                0x01u8, 0x00u8, 0x6bu8, 0x00u8, 0x19u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8,
            ],
        )
        .await?;
    //Motor peak torque
    subdevice
        .idn_write_data(0, 32841u16, [0xeau8, 0x06u8, 0x00u8, 0x00u8])
        .await?;
    //Motor inductance characteristic
    subdevice
        .idn_write_data(
            0,
            32843u16,
            [
                0x50u8, 0x00u8, 0x50u8, 0x00u8, 0xbau8, 0x04u8, 0x00u8, 0x00u8, 0x74u8, 0x09u8,
                0x00u8, 0x00u8, 0x2eu8, 0x0eu8, 0x00u8, 0x00u8, 0xe8u8, 0x12u8, 0x00u8, 0x00u8,
                0xa2u8, 0x17u8, 0x00u8, 0x00u8, 0x5cu8, 0x1cu8, 0x00u8, 0x00u8, 0x16u8, 0x21u8,
                0x00u8, 0x00u8, 0xd0u8, 0x25u8, 0x00u8, 0x00u8, 0x94u8, 0x2au8, 0x00u8, 0x00u8,
                0x44u8, 0x2fu8, 0x00u8, 0x00u8, 0x54u8, 0x10u8, 0x00u8, 0x00u8, 0xbeu8, 0x0fu8,
                0x00u8, 0x00u8, 0xd8u8, 0x0eu8, 0x00u8, 0x00u8, 0xfcu8, 0x0du8, 0x00u8, 0x00u8,
                0xf8u8, 0x0cu8, 0x00u8, 0x00u8, 0x5eu8, 0x0bu8, 0x00u8, 0x00u8, 0x56u8, 0x09u8,
                0x00u8, 0x00u8, 0xb2u8, 0x07u8, 0x00u8, 0x00u8, 0x0eu8, 0x06u8, 0x00u8, 0x00u8,
                0x6au8, 0x04u8, 0x00u8, 0x00u8,
            ],
        )
        .await?;
    //Motor torque/force characteristic
    subdevice
        .idn_write_data(
            0,
            32842u16,
            [
                0x50u8, 0x00u8, 0x50u8, 0x00u8, 0xbau8, 0x04u8, 0x00u8, 0x00u8, 0x74u8, 0x09u8,
                0x00u8, 0x00u8, 0x2eu8, 0x0eu8, 0x00u8, 0x00u8, 0xe8u8, 0x12u8, 0x00u8, 0x00u8,
                0xa2u8, 0x17u8, 0x00u8, 0x00u8, 0x5cu8, 0x1cu8, 0x00u8, 0x00u8, 0x16u8, 0x21u8,
                0x00u8, 0x00u8, 0xd0u8, 0x25u8, 0x00u8, 0x00u8, 0x94u8, 0x2au8, 0x00u8, 0x00u8,
                0x44u8, 0x2fu8, 0x00u8, 0x00u8, 0x02u8, 0x01u8, 0x00u8, 0x00u8, 0xf3u8, 0x01u8,
                0x00u8, 0x00u8, 0xd1u8, 0x02u8, 0x00u8, 0x00u8, 0x9du8, 0x03u8, 0x00u8, 0x00u8,
                0x56u8, 0x04u8, 0x00u8, 0x00u8, 0x00u8, 0x05u8, 0x00u8, 0x00u8, 0x96u8, 0x05u8,
                0x00u8, 0x00u8, 0x18u8, 0x06u8, 0x00u8, 0x00u8, 0x90u8, 0x06u8, 0x00u8, 0x00u8,
                0xeau8, 0x06u8, 0x00u8, 0x00u8,
            ],
        )
        .await?;
    //Motor temperature sensor characteristic
    subdevice
        .idn_write_data(
            0,
            32844u16,
            [
                0x28u8, 0x00u8, 0x28u8, 0x00u8, 0x70u8, 0xfeu8, 0x65u8, 0xffu8, 0x5au8, 0x00u8,
                0x4fu8, 0x01u8, 0x44u8, 0x02u8, 0x39u8, 0x03u8, 0x2eu8, 0x04u8, 0x28u8, 0x05u8,
                0x18u8, 0x06u8, 0x12u8, 0x07u8, 0x67u8, 0x01u8, 0xb8u8, 0x01u8, 0x16u8, 0x02u8,
                0x82u8, 0x02u8, 0xfau8, 0x02u8, 0x80u8, 0x03u8, 0x13u8, 0x04u8, 0xb4u8, 0x04u8,
                0x61u8, 0x05u8, 0x18u8, 0x06u8,
            ],
        )
        .await?;
    //Motor brake
    subdevice
        .idn_write_data(0, 32828u16, [0x01u8, 0x00u8])
        .await?;
    //Drive on delay time
    subdevice
        .idn_write_data(0, 206u16, [0x50u8, 0x00u8])
        .await?;
    //Drive off delay time
    subdevice
        .idn_write_data(0, 207u16, [0x28u8, 0x00u8])
        .await?;
    //Motor brake current monitoring level
    subdevice
        .idn_write_data(0, 32827u16, [0x0eu8, 0x01u8])
        .await?;
    //Motor brake data
    subdevice
        .idn_write_data(
            0,
            32840u16,
            [
                0x06u8, 0x00u8, 0x06u8, 0x00u8, 0x84u8, 0x03u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8,
            ],
        )
        .await?;
    //Motor continuous stall torque
    subdevice
        .idn_write_data(0, 32838u16, [0xe1u8, 0x01u8, 0x00u8, 0x00u8])
        .await?;
    //Maximum motor speed
    subdevice
        .idn_write_data(0, 113u16, [0x28u8, 0x23u8, 0x00u8, 0x00u8])
        .await?;
    //Bipolar velocity limit value
    subdevice
        .idn_write_data(0, 91u16, [0x52u8, 0xb8u8, 0x1eu8, 0x09u8])
        .await?;
    //Motor construction type
    subdevice
        .idn_write_data(0, 32818u16, [0x00u8, 0x00u8])
        .await?;
    //Motor data constraints
    subdevice
        .idn_write_data(
            0,
            32857u16,
            [
                0x24u8, 0x00u8, 0x24u8, 0x00u8, 0x26u8, 0x83u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8,
                0x00u8, 0x00u8, 0x40u8, 0x1fu8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8,
                0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8,
                0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8,
            ],
        )
        .await?;
    //Configured drive type
    subdevice
        .idn_write_data(
            0,
            32822u16,
            [
                0x10u8, 0x00u8, 0x1eu8, 0x00u8, 0x41u8, 0x58u8, 0x35u8, 0x32u8, 0x30u8, 0x33u8,
                0x2du8, 0x30u8, 0x30u8, 0x30u8, 0x30u8, 0x2du8, 0x23u8, 0x23u8, 0x23u8, 0x23u8,
            ],
        )
        .await?;
    //Configured channel current
    subdevice
        .idn_write_data(0, 32861u16, [0x8cu8, 0x0au8, 0x00u8, 0x00u8])
        .await?;
    //Configured channel peak current
    subdevice
        .idn_write_data(0, 32860u16, [0x18u8, 0x15u8, 0x00u8, 0x00u8])
        .await?;
    //Time limitation for peak current
    subdevice
        .idn_write_data(0, 32820u16, [0x00u8, 0x00u8])
        .await?;
    //Current controller settings 2
    subdevice
        .idn_write_data(0, 33219u16, [0x00u8, 0x00u8])
        .await?;
    //Current loop proportional gain 1
    subdevice
        .idn_write_data(0, 106u16, [0x01u8, 0x05u8])
        .await?;
    //Current control loop integral action time 1
    subdevice
        .idn_write_data(0, 107u16, [0x08u8, 0x00u8])
        .await?;
    //Velocity filter 1: Low pass time constant
    subdevice
        .idn_write_data(0, 33279u16, [0x96u8, 0x00u8])
        .await?;
    //Velocity loop proportional gain
    subdevice
        .idn_write_data(0, 100u16, [0x88u8, 0x00u8, 0x00u8, 0x00u8])
        .await?;
    //Velocity loop integral action time
    subdevice
        .idn_write_data(0, 101u16, [0x50u8, 0x00u8])
        .await?;
    //Feedback 1 type
    subdevice
        .idn_write_data(
            0,
            32918u16,
            [
                0xe0u8, 0x00u8, 0xe0u8, 0x00u8, 0x03u8, 0x00u8, 0x00u8, 0x00u8, 0x53u8, 0x69u8,
                0x63u8, 0x6bu8, 0x23u8, 0x45u8, 0x44u8, 0x4du8, 0x33u8, 0x35u8, 0x2du8, 0x32u8,
                0x4bu8, 0x46u8, 0x30u8, 0x41u8, 0x30u8, 0x32u8, 0x34u8, 0x41u8, 0x00u8, 0x00u8,
                0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8,
                0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8,
                0x00u8, 0x00u8, 0x0eu8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8,
                0x05u8, 0x00u8, 0x0eu8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8,
                0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0xe8u8, 0x03u8, 0x00u8, 0x00u8,
                0xe8u8, 0x03u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8,
                0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8,
                0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x07u8, 0x00u8,
                0x0eu8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x18u8, 0x00u8, 0x0cu8, 0x00u8,
                0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x02u8, 0x00u8, 0x02u8, 0x00u8, 0x00u8, 0x00u8,
                0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8,
                0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8,
                0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8,
                0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8,
                0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8,
                0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8,
                0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8,
                0x96u8, 0x00u8, 0x96u8, 0x00u8, 0x73u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8,
                0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8,
                0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8,
            ],
        )
        .await?;
    //Velocity observer
    subdevice
        .idn_write_data(
            0,
            33282u16,
            [
                0x0cu8, 0x00u8, 0x0cu8, 0x00u8, 0x01u8, 0x00u8, 0x00u8, 0x00u8, 0xf4u8, 0x01u8,
                0xe8u8, 0x03u8, 0xbcu8, 0x02u8, 0x00u8, 0x00u8,
            ],
        )
        .await?;
    //Motor temperature sensor type
    subdevice
        .idn_write_data(0, 32829u16, [0x07u8, 0x00u8])
        .await?;
    //Operation mode
    subdevice.idn_write_data(1, 32u16, [0x0bu8, 0x00u8]).await?;
    //Configured motor type
    subdevice
        .idn_write_data(
            1,
            32821u16,
            [
                0x0cu8, 0x00u8, 0x1eu8, 0x00u8, 0x41u8, 0x4du8, 0x38u8, 0x30u8, 0x35u8, 0x31u8,
                0x2du8, 0x78u8, 0x45u8, 0x78u8, 0x31u8, 0x00u8,
            ],
        )
        .await?;
    //Number of pole pairs
    subdevice
        .idn_write_data(1, 32819u16, [0x04u8, 0x00u8])
        .await?;
    //Mechanical motor data
    subdevice
        .idn_write_data(
            1,
            32839u16,
            [
                0x08u8, 0x00u8, 0x08u8, 0x00u8, 0x23u8, 0x01u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8,
                0x00u8, 0x00u8,
            ],
        )
        .await?;
    //Motor continuous stall current
    subdevice
        .idn_write_data(1, 111u16, [0x8cu8, 0x0au8, 0x00u8, 0x00u8])
        .await?;
    //Motor rated current
    subdevice
        .idn_write_data(1, 196u16, [0xf6u8, 0x09u8, 0x00u8, 0x00u8])
        .await?;
    //Motor peak current
    subdevice
        .idn_write_data(1, 109u16, [0x44u8, 0x2fu8, 0x00u8, 0x00u8])
        .await?;
    //Motor rated voltage
    subdevice
        .idn_write_data(1, 32845u16, [0xa0u8, 0x0fu8])
        .await?;
    //Motor winding: Dielectric strength
    subdevice
        .idn_write_data(1, 32835u16, [0xc4u8, 0x22u8])
        .await?;
    //Electric motor model
    subdevice
        .idn_write_data(
            1,
            32834u16,
            [
                0x08u8, 0x00u8, 0x08u8, 0x00u8, 0x74u8, 0x04u8, 0x00u8, 0x00u8, 0xaeu8, 0x10u8,
                0x00u8, 0x00u8,
            ],
        )
        .await?;
    //Motor warning temperature
    subdevice
        .idn_write_data(1, 201u16, [0xb0u8, 0x04u8])
        .await?;
    //Motor shut down temperature
    subdevice
        .idn_write_data(1, 204u16, [0x78u8, 0x05u8])
        .await?;
    //Motor EMF
    subdevice
        .idn_write_data(1, 32823u16, [0xe2u8, 0x04u8])
        .await?;
    //Thermal motor model
    subdevice
        .idn_write_data(
            1,
            32830u16,
            [
                0x10u8, 0x00u8, 0x10u8, 0x00u8, 0x44u8, 0x07u8, 0x50u8, 0x00u8, 0x64u8, 0x00u8,
                0x01u8, 0x00u8, 0x6bu8, 0x00u8, 0x19u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8,
            ],
        )
        .await?;
    //Motor peak torque
    subdevice
        .idn_write_data(1, 32841u16, [0xeau8, 0x06u8, 0x00u8, 0x00u8])
        .await?;
    //Motor inductance characteristic
    subdevice
        .idn_write_data(
            1,
            32843u16,
            [
                0x50u8, 0x00u8, 0x50u8, 0x00u8, 0xbau8, 0x04u8, 0x00u8, 0x00u8, 0x74u8, 0x09u8,
                0x00u8, 0x00u8, 0x2eu8, 0x0eu8, 0x00u8, 0x00u8, 0xe8u8, 0x12u8, 0x00u8, 0x00u8,
                0xa2u8, 0x17u8, 0x00u8, 0x00u8, 0x5cu8, 0x1cu8, 0x00u8, 0x00u8, 0x16u8, 0x21u8,
                0x00u8, 0x00u8, 0xd0u8, 0x25u8, 0x00u8, 0x00u8, 0x8au8, 0x2au8, 0x00u8, 0x00u8,
                0x44u8, 0x2fu8, 0x00u8, 0x00u8, 0x54u8, 0x10u8, 0x00u8, 0x00u8, 0xbeu8, 0x0fu8,
                0x00u8, 0x00u8, 0xd8u8, 0x0eu8, 0x00u8, 0x00u8, 0xfcu8, 0x0du8, 0x00u8, 0x00u8,
                0xf8u8, 0x0cu8, 0x00u8, 0x00u8, 0x5eu8, 0x0bu8, 0x00u8, 0x00u8, 0x56u8, 0x09u8,
                0x00u8, 0x00u8, 0xb2u8, 0x07u8, 0x00u8, 0x00u8, 0x0eu8, 0x06u8, 0x00u8, 0x00u8,
                0x6au8, 0x04u8, 0x00u8, 0x00u8,
            ],
        )
        .await?;
    //Motor torque/force characteristic
    subdevice
        .idn_write_data(
            1,
            32842u16,
            [
                0x50u8, 0x00u8, 0x50u8, 0x00u8, 0xbau8, 0x04u8, 0x00u8, 0x00u8, 0x74u8, 0x09u8,
                0x00u8, 0x00u8, 0x2eu8, 0x0eu8, 0x00u8, 0x00u8, 0xe8u8, 0x12u8, 0x00u8, 0x00u8,
                0xa2u8, 0x17u8, 0x00u8, 0x00u8, 0x5cu8, 0x1cu8, 0x00u8, 0x00u8, 0x16u8, 0x21u8,
                0x00u8, 0x00u8, 0xd0u8, 0x25u8, 0x00u8, 0x00u8, 0x8au8, 0x2au8, 0x00u8, 0x00u8,
                0x44u8, 0x2fu8, 0x00u8, 0x00u8, 0x02u8, 0x01u8, 0x00u8, 0x00u8, 0xf3u8, 0x01u8,
                0x00u8, 0x00u8, 0xd1u8, 0x02u8, 0x00u8, 0x00u8, 0x9du8, 0x03u8, 0x00u8, 0x00u8,
                0x57u8, 0x04u8, 0x00u8, 0x00u8, 0x00u8, 0x05u8, 0x00u8, 0x00u8, 0x96u8, 0x05u8,
                0x00u8, 0x00u8, 0x1bu8, 0x06u8, 0x00u8, 0x00u8, 0x8du8, 0x06u8, 0x00u8, 0x00u8,
                0xeau8, 0x06u8, 0x00u8, 0x00u8,
            ],
        )
        .await?;
    //Motor temperature sensor characteristic
    subdevice
        .idn_write_data(
            1,
            32844u16,
            [
                0x28u8, 0x00u8, 0x28u8, 0x00u8, 0x70u8, 0xfeu8, 0x65u8, 0xffu8, 0x5au8, 0x00u8,
                0x4fu8, 0x01u8, 0x44u8, 0x02u8, 0x39u8, 0x03u8, 0x2eu8, 0x04u8, 0x23u8, 0x05u8,
                0x18u8, 0x06u8, 0x12u8, 0x07u8, 0x67u8, 0x01u8, 0xb8u8, 0x01u8, 0x16u8, 0x02u8,
                0x82u8, 0x02u8, 0xfau8, 0x02u8, 0x80u8, 0x03u8, 0x13u8, 0x04u8, 0xb4u8, 0x04u8,
                0x61u8, 0x05u8, 0x18u8, 0x06u8,
            ],
        )
        .await?;
    //Motor brake
    subdevice
        .idn_write_data(1, 32828u16, [0x01u8, 0x00u8])
        .await?;
    //Drive on delay time
    subdevice
        .idn_write_data(1, 206u16, [0x50u8, 0x00u8])
        .await?;
    //Drive off delay time
    subdevice
        .idn_write_data(1, 207u16, [0x28u8, 0x00u8])
        .await?;
    //Motor brake current monitoring level
    subdevice
        .idn_write_data(1, 32827u16, [0x0eu8, 0x01u8])
        .await?;
    //Motor brake data
    subdevice
        .idn_write_data(
            1,
            32840u16,
            [
                0x06u8, 0x00u8, 0x06u8, 0x00u8, 0x84u8, 0x03u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8,
            ],
        )
        .await?;
    //Electrical commutation offset
    subdevice
        .idn_write_data(1, 32825u16, [0x78u8, 0x69u8])
        .await?;
    //Motor continuous stall torque
    subdevice
        .idn_write_data(1, 32838u16, [0xe1u8, 0x01u8, 0x00u8, 0x00u8])
        .await?;
    //Maximum motor speed
    subdevice
        .idn_write_data(1, 113u16, [0x28u8, 0x23u8, 0x00u8, 0x00u8])
        .await?;
    //Bipolar velocity limit value
    subdevice
        .idn_write_data(1, 91u16, [0x52u8, 0xb8u8, 0x1eu8, 0x09u8])
        .await?;
    //Motor construction type
    subdevice
        .idn_write_data(1, 32818u16, [0x00u8, 0x00u8])
        .await?;
    //Motor data constraints
    subdevice
        .idn_write_data(
            1,
            32857u16,
            [
                0x24u8, 0x00u8, 0x24u8, 0x00u8, 0x26u8, 0x83u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8,
                0x00u8, 0x00u8, 0x40u8, 0x1fu8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8,
                0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8,
                0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8,
            ],
        )
        .await?;
    //Configured drive type
    subdevice
        .idn_write_data(
            1,
            32822u16,
            [
                0x10u8, 0x00u8, 0x1eu8, 0x00u8, 0x41u8, 0x58u8, 0x35u8, 0x32u8, 0x30u8, 0x33u8,
                0x2du8, 0x30u8, 0x30u8, 0x30u8, 0x30u8, 0x2du8, 0x23u8, 0x23u8, 0x23u8, 0x23u8,
            ],
        )
        .await?;
    //Configured channel current
    subdevice
        .idn_write_data(1, 32861u16, [0x8cu8, 0x0au8, 0x00u8, 0x00u8])
        .await?;
    //Configured channel peak current
    subdevice
        .idn_write_data(1, 32860u16, [0x18u8, 0x15u8, 0x00u8, 0x00u8])
        .await?;
    //Time limitation for peak current
    subdevice
        .idn_write_data(1, 32820u16, [0x00u8, 0x00u8])
        .await?;
    //Current controller settings 2
    subdevice
        .idn_write_data(1, 33219u16, [0x00u8, 0x00u8])
        .await?;
    //Current loop proportional gain 1
    subdevice
        .idn_write_data(1, 106u16, [0x01u8, 0x05u8])
        .await?;
    //Current control loop integral action time 1
    subdevice
        .idn_write_data(1, 107u16, [0x08u8, 0x00u8])
        .await?;
    //Velocity filter 1: Low pass time constant
    subdevice
        .idn_write_data(1, 33279u16, [0x96u8, 0x00u8])
        .await?;
    //Velocity loop proportional gain
    subdevice
        .idn_write_data(1, 100u16, [0x88u8, 0x00u8, 0x00u8, 0x00u8])
        .await?;
    //Velocity loop integral action time
    subdevice
        .idn_write_data(1, 101u16, [0x50u8, 0x00u8])
        .await?;
    //Feedback 1 type
    subdevice
        .idn_write_data(
            1,
            32918u16,
            [
                0xe0u8, 0x00u8, 0xe0u8, 0x00u8, 0x03u8, 0x00u8, 0x00u8, 0x00u8, 0x45u8, 0x44u8,
                0x4du8, 0x33u8, 0x35u8, 0x2du8, 0x32u8, 0x4bu8, 0x46u8, 0x30u8, 0x41u8, 0x30u8,
                0x53u8, 0x30u8, 0x33u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8,
                0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8,
                0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8,
                0x00u8, 0x00u8, 0x18u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8,
                0x05u8, 0x00u8, 0x18u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8,
                0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0xe8u8, 0x03u8, 0x00u8, 0x00u8,
                0xe8u8, 0x03u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8,
                0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8,
                0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x07u8, 0x00u8,
                0x18u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x18u8, 0x00u8, 0x0cu8, 0x00u8,
                0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x02u8, 0x00u8, 0x02u8, 0x00u8, 0x00u8, 0x00u8,
                0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8,
                0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8,
                0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8,
                0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8,
                0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8,
                0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8,
                0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8,
                0x96u8, 0x00u8, 0x00u8, 0x00u8, 0x7du8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8,
                0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8,
                0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8,
            ],
        )
        .await?;
    //Velocity observer
    subdevice
        .idn_write_data(
            1,
            33282u16,
            [
                0x0cu8, 0x00u8, 0x0cu8, 0x00u8, 0x01u8, 0x00u8, 0x00u8, 0x00u8, 0xf4u8, 0x01u8,
                0xe8u8, 0x03u8, 0xbcu8, 0x02u8, 0x00u8, 0x00u8,
            ],
        )
        .await?;
    //Motor temperature sensor type
    subdevice
        .idn_write_data(1, 32829u16, [0x07u8, 0x00u8])
        .await?;
    Ok(())
}
