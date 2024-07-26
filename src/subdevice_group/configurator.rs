use crate::{
    error::Error,
    fmt,
    pdi::PdiOffset,
    subdevice::{SubDevice, SubDeviceRef},
    Client, SubDeviceGroup,
};
use atomic_refcell::AtomicRefCell;

#[derive(Debug)]
struct GroupInnerRef<'a> {
    subdevices: &'a mut [AtomicRefCell<SubDevice>],
    pdi_start: &'a mut PdiOffset,
}

// TODO: Prove if this is safe. All this stuff is internal to the crate and short lived so I think
// we might be ok... lol probably not :(.
//
// This is in response to <https://github.com/ethercrab-rs/ethercrab/issues/56#issuecomment-1618904531>
unsafe impl<'a> Sync for SubDeviceGroupRef<'a> {}
unsafe impl<'a> Send for SubDeviceGroupRef<'a> {}

/// A reference to a [`SubDeviceGroup`](crate::SubDeviceGroup) returned by the closure passed to
/// [`Client::init`](crate::Client::init).
pub struct SubDeviceGroupRef<'a> {
    /// Maximum PDI length in bytes.
    max_pdi_len: usize,
    inner: GroupInnerRef<'a>,
}

impl<'a> SubDeviceGroupRef<'a> {
    pub(in crate::subdevice_group) fn new<const MAX_SUBDEVICES: usize, const MAX_PDI: usize, S>(
        group: &'a SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, S>,
    ) -> Self {
        Self {
            max_pdi_len: MAX_PDI,
            inner: {
                let inner = unsafe { fmt::unwrap_opt!(group.inner.get().as_mut()) };

                GroupInnerRef {
                    subdevices: &mut inner.subdevices,
                    pdi_start: &mut inner.pdi_start,
                }
            },
        }
    }

    /// Initialise all SubDevices in the group and place them in PRE-OP.
    // Clippy: shush
    #[allow(clippy::wrong_self_convention)]
    pub(crate) async fn into_pre_op<'sto>(
        &mut self,
        pdi_position: PdiOffset,
        client: &'sto Client<'sto>,
    ) -> Result<PdiOffset, Error> {
        let inner = &mut self.inner;

        // Set the starting position in the PDI for this group's segment
        *inner.pdi_start = pdi_position;

        fmt::debug!(
            "Going to configure group with {} SubDevice(s), starting PDI offset {:#08x}",
            inner.subdevices.len(),
            inner.pdi_start.start_address
        );

        // Configure master read PDI mappings in the first section of the PDI
        for subdevice in inner.subdevices.iter_mut() {
            let subdevice = subdevice.get_mut();

            let mut subdevice_config =
                SubDeviceRef::new(client, subdevice.configured_address(), subdevice);

            // TODO: Move PRE-OP transition out of this so we can do it for the group just once
            subdevice_config.configure_mailboxes().await?;
        }

        Ok(pdi_position.increment(self.max_pdi_len as u16))
    }
}
