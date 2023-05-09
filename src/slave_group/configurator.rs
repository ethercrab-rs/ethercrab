use super::HookFn;
use crate::{
    error::Error,
    pdi::PdiOffset,
    register::RegisterAddress,
    slave::{configuration::PdoDirection, Slave, SlaveRef},
    Client, SlaveGroup,
};
use core::{cell::UnsafeCell, time::Duration};

#[derive(Debug)]
struct GroupInnerRef<'a> {
    slaves: &'a mut [Slave],
    /// The number of bytes at the beginning of the PDI reserved for slave inputs.
    read_pdi_len: &'a mut usize,
    /// The total length (I and O) of the PDI for this group.
    pdi_len: &'a mut usize,
    start_address: &'a mut u32,
    /// Expected working counter when performing a read/write to all slaves in this group.
    ///
    /// This should be equivalent to `(slaves with inputs) + (2 * slaves with outputs)`.
    group_working_counter: &'a mut u16,
}

/// A reference to a [`SlaveGroup`](crate::SlaveGroup) returned by the closure passed to
/// [`Client::init`](crate::Client::init).
pub struct SlaveGroupRef<'a> {
    max_pdi_len: usize,
    preop_safeop_hook: &'a Option<HookFn>,
    inner: UnsafeCell<GroupInnerRef<'a>>,
}

impl<'a> SlaveGroupRef<'a> {
    pub(in crate::slave_group) fn new<const MAX_SLAVES: usize, const MAX_PDI: usize>(
        group: &'a SlaveGroup<MAX_SLAVES, MAX_PDI>,
    ) -> Self {
        Self {
            max_pdi_len: MAX_PDI,
            preop_safeop_hook: &group.preop_safeop_hook,
            inner: {
                let inner = unsafe { &mut *group.inner.get() };

                UnsafeCell::new(GroupInnerRef {
                    slaves: &mut inner.slaves,
                    read_pdi_len: &mut inner.read_pdi_len,
                    pdi_len: &mut inner.pdi_len,
                    start_address: &mut inner.start_address,
                    group_working_counter: &mut inner.group_working_counter,
                })
            },
        }
    }

    pub(crate) async unsafe fn configure_from_eeprom<'sto>(
        &self,
        // We need to start this group's PDI after that of the previous group. That offset is passed
        // in via `start_offset`.
        mut global_offset: PdiOffset,
        client: &'sto Client<'sto>,
    ) -> Result<PdiOffset, Error> {
        let inner = unsafe { &mut *self.inner.get() };

        log::debug!(
            "Going to configure group with {} slave(s), starting PDI offset {:#08x}",
            inner.slaves.len(),
            global_offset.start_address
        );

        // Set the starting position in the PDI for this group's segment
        *inner.start_address = global_offset.start_address;

        // Configure master read PDI mappings in the first section of the PDI
        for slave in inner.slaves.iter_mut() {
            let mut slave_config = SlaveRef::new(client, slave.configured_address, slave);

            // TODO: Split `SlaveGroupRef::configure_from_eeprom` so we can put all slaves into
            // SAFE-OP without waiting, then wait globally for all slaves to reach that state.
            // Currently startup time is extremely slow. NOTE: This method requests and waits for
            // the slave to enter PRE-OP
            slave_config.configure_mailboxes().await?;

            if let Some(hook) = self.preop_safeop_hook {
                // let conf = slave_config.as_ref();

                let fut = (hook)(&slave_config);

                fut.await?;
            }

            // We're in PRE-OP at this point
            global_offset = slave_config
                .configure_fmmus(
                    global_offset,
                    *inner.start_address,
                    PdoDirection::MasterRead,
                )
                .await?;
        }

        *inner.read_pdi_len = (global_offset.start_address - *inner.start_address) as usize;

        log::debug!("Slave mailboxes configured and init hooks called");

        // We configured all read PDI mappings as a contiguous block in the previous loop. Now we'll
        // configure the write mappings in a separate loop. This means we have IIIIOOOO instead of
        // IOIOIO.
        for (_i, slave) in inner.slaves.iter_mut().enumerate() {
            let addr = slave.configured_address;
            let name = slave.name.clone();

            let mut slave_config = SlaveRef::new(client, slave.configured_address, slave);

            // Still in PRE-OP
            global_offset = slave_config
                .configure_fmmus(
                    global_offset,
                    *inner.start_address,
                    PdoDirection::MasterWrite,
                )
                .await?;

            // FIXME: Just first slave or all slaves?
            // if name == "EL2004" {
            // if i == 0 {
            if false {
                log::info!("Slave {:#06x} {} DC", addr, name);
                // let slave_config = SlaveRef::new(client, slave.configured_address, ());

                // TODO: Pass in as config
                let cycle_time = Duration::from_millis(2).as_nanos() as u32;

                // Disable sync signals
                slave_config
                    .write(RegisterAddress::DcSyncActive, 0x00u8, "disable sync")
                    .await?;

                let local_time: u32 = slave_config
                    .read(RegisterAddress::DcSystemTime, "local time")
                    .await?;

                // TODO: Pass in as config
                // let startup_delay = Duration::from_millis(100).as_nanos() as u32;
                let startup_delay = 0;

                // TODO: Pass in as config
                let start_time = local_time + cycle_time + startup_delay;

                slave_config
                    .write(
                        RegisterAddress::DcSyncStartTime,
                        start_time,
                        "sync start time",
                    )
                    .await?;

                slave_config
                    .write(
                        RegisterAddress::DcSync0CycleTime,
                        cycle_time,
                        "sync cycle time",
                    )
                    .await?;

                // Enable cyclic operation (0th bit) and sync0 signal (1st bit)
                slave_config
                    .write(RegisterAddress::DcSyncActive, 0b11u8, "enable sync0")
                    .await?;
            }

            // We're done configuring FMMUs, etc, now we can request this slave go into SAFE-OP
            slave_config.request_safe_op_nowait().await?;

            // We have both inputs and outputs at this stage, so can correctly calculate the group
            // WKC.
            *inner.group_working_counter += slave_config.working_counter_sum();
        }

        log::debug!("Slave FMMUs configured for group. Able to move to SAFE-OP");

        let pdi_len = (global_offset.start_address - *inner.start_address) as usize;

        log::debug!(
            "Group PDI length: start {}, {} total bytes ({} input bytes)",
            inner.start_address,
            pdi_len,
            *inner.read_pdi_len
        );

        if pdi_len > self.max_pdi_len {
            Err(Error::PdiTooLong {
                max_length: self.max_pdi_len,
                desired_length: pdi_len,
            })
        } else {
            *inner.pdi_len = pdi_len;

            Ok(global_offset)
        }
    }
}
