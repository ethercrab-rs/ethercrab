//! Discover devices connected to the network.

use env_logger::Env;
use ethercrab::{
    error::Error, std::{ethercat_now, tx_rx_task}, ConfigureSubdevicePdi, MainDevice, MainDeviceConfig, PdiSegment, PdoDirection, PduStorage, SyncManagerType, Timeouts
};
use std::sync::Arc;

/// Maximum number of SubDevices that can be stored. This must be a power of 2 greater than 1.
const MAX_SUBDEVICES: usize = 128;
/// Maximum PDU data payload size - set this to the max PDI size or higher.
const MAX_PDU_DATA: usize = PduStorage::element_size(1100);
/// Maximum number of EtherCAT frames that can be in flight at any one time.
const MAX_FRAMES: usize = 16;
/// Maximum total PDI length.
const PDI_LEN: usize = 128;

static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();

fn main() {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let interface = std::env::args()
        .nth(1)
        .expect("Provide network interface as first argument.");

    log::info!("Discovering EtherCAT devices on {}...", interface);

    let (tx, rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");

    let maindevice = Arc::new(MainDevice::new(
        pdu_loop,
        Timeouts::default(),
        MainDeviceConfig {
            dc_static_sync_iterations: 0,
            ..MainDeviceConfig::default()
        },
    ));

    smol::block_on(async {
        smol::spawn(tx_rx_task(&interface, tx, rx).expect("spawn TX/RX task")).detach();

        let mut group = maindevice
            .init_single_group::<MAX_SUBDEVICES, PDI_LEN>(ethercat_now)
            .await
            .expect("Init");

        log::info!("Discovered {} SubDevices", group.len());

        for subdevice in group.iter(&maindevice) {
            log::info!(
                "--> SubDevice {:#06x} {} {}",
                subdevice.configured_address(),
                subdevice.name(),
                subdevice.identity()
            );
        }

        let group = group.into_pre_op_pdi_with_callback(&maindevice, ConfigureKistler5074B).await.expect("Pre-op PDI");

        group.into_op(&maindevice).await.expect("Op");
    });

    log::info!("Done.");
}

struct ConfigureKistler5074B;

impl ConfigureSubdevicePdi for ConfigureKistler5074B {
    type Error = Error;
    async fn configure<'a, S>(
            &mut self,
            subdevice: &mut ethercrab::SubDeviceRef<'a, S>,
            sync_managers: &[ethercrab::SyncManager],
            _fmmu_usage: &[ethercrab::FmmuUsage],
            direction: ethercrab::PdoDirection,
            global_offset: &mut ethercrab::PdiOffset,
        ) -> Result<ethercrab::PdiSegment, Self::Error>
        where
            S: std::ops::DerefMut<Target = ethercrab::SubDevice> {

        let start_offset = *global_offset;

        // XXX: The charge amp isn't spec-compliant, PDO mapping SDOs (e.g. 0x1600 and
        // 0x1A00) are not readable which breaks Ethercrab's default PDI setup.
        pub const SM_ASSIGNMENT_BASE_ADDRESS: u16 = 0x1C10;
        let (len_bytes, sm_idx, fmmu_idx) = match direction {
            PdoDirection::MasterRead => {
                const LEN_BYTES: u16 = 4 + (4 * 4 * 4);
                const SM_IDX: u8 = 3;
                const SM_ASSIGNMENT: u16 = SM_ASSIGNMENT_BASE_ADDRESS + (SM_IDX as u16);
                const FMMU_IDX: usize = 1;

                // PDO mapping
                // All PDOs are fixed, nothing to do.

                // PDO assignments
                // XXX: You have to play this little song and dance with the CoE array's length.
                subdevice
                    .sdo_write::<u8>(SM_ASSIGNMENT, 0, 0)
                    .await
                    .expect("zeroing pdo assignment count");
                // Status ChX - BYTE
                // Scaled Value ChX - REAL
                for channel_idx in 0..4u8 {
                    let sdo_subindex = 0x1A00_u16 + (u16::from(channel_idx) << 4);
                    subdevice
                        .sdo_write(SM_ASSIGNMENT, (channel_idx * 5) + 1, sdo_subindex)
                        .await
                        .expect("setting CH1 status PDO assignment");
                    subdevice
                        .sdo_write(SM_ASSIGNMENT, (channel_idx * 5) + 2, sdo_subindex + 1_u16)
                        .await
                        .expect("setting CH1 value PDO assignment");
                    subdevice
                        .sdo_write(SM_ASSIGNMENT, (channel_idx * 5) + 3, sdo_subindex + 2_u16)
                        .await
                        .expect("setting CH1 peak min PDO assignment");
                    subdevice
                        .sdo_write(SM_ASSIGNMENT, (channel_idx * 5) + 4, sdo_subindex + 3_u16)
                        .await
                        .expect("setting CH1 peak max PDO assignment");
                    subdevice
                        .sdo_write(SM_ASSIGNMENT, (channel_idx * 5) + 5, sdo_subindex + 4_u16)
                        .await
                        .expect("setting CH1 integral PDO assignment");
                }
                // Donzo
                subdevice
                    .sdo_write::<u8>(SM_ASSIGNMENT, 0, 4 * 5)
                    .await
                    .expect("setting PDO assignment count");

                (LEN_BYTES, SM_IDX, FMMU_IDX)
            }
            PdoDirection::MasterWrite => {
                const LEN_BYTES: u16 = 4;
                const SM_IDX: u8 = 2;
                const SM_ASSIGNMENT: u16 = SM_ASSIGNMENT_BASE_ADDRESS + (SM_IDX as u16);
                const FMMU_IDX: usize = 0;

                // PDO mapping: All PDOs are fixed, nothing to do.

                // PDO assignments
                // XXX: You have to play this little song and dance with the CoE array's length.
                subdevice
                    .sdo_write(SM_ASSIGNMENT, 0, 0_u8)
                    .await
                    .expect("zeroing pdo assignment count");
                // Control Ch - BYTE
                // XXX: Even though these are required PDO assignments, you do have to reassign
                // them. This may have been what was breaking EtherCrab's EEPROM based setup.
                subdevice
                    .sdo_write(SM_ASSIGNMENT, 1, 0x1600_u16)
                    .await
                    .expect("setting CH1 control PDO assignment");
                subdevice
                    .sdo_write(SM_ASSIGNMENT, 2, 0x1610_u16)
                    .await
                    .expect("setting CH2 control PDO assignment");
                subdevice
                    .sdo_write(SM_ASSIGNMENT, 3, 0x1620_u16)
                    .await
                    .expect("setting CH3 control PDO assignment");
                subdevice
                    .sdo_write(SM_ASSIGNMENT, 4, 0x1630_u16)
                    .await
                    .expect("setting CH4 control PDO assignment");
                // Donzo
                subdevice
                    .sdo_write(SM_ASSIGNMENT, 0, 4_u8)
                    .await
                    .expect("setting PDO assignment count");

                (LEN_BYTES, SM_IDX, FMMU_IDX)
            }
        };

        let len_bits = 8 * len_bytes;

        let sm_config = subdevice
            .write_sm_config(sm_idx, &sync_managers[usize::from(sm_idx)], len_bytes)
            .await
            .expect("configuring sync manager");
        let desired_sm_type = match direction {
            PdoDirection::MasterRead => SyncManagerType::ProcessDataRead,
            PdoDirection::MasterWrite => SyncManagerType::ProcessDataWrite,
        };
        subdevice
            .write_fmmu_config(len_bits, fmmu_idx, global_offset, desired_sm_type, &sm_config)
            .await
            .expect("configuring fmmu");

        Ok(PdiSegment {
            bit_len: usize::from(len_bits),
            bytes:   start_offset.up_to(*global_offset),
        })
    }
}