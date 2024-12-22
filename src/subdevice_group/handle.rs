use crate::{
    error::Error, fmt, pdi::PdiOffset, GroupId, MainDevice, SubDevice, SubDeviceGroup, SubDeviceRef,
};

/// A trait implemented only by [`SubDeviceGroup`] so multiple groups with different const params
/// can be stored in a hashmap, `Vec`, etc.
#[doc(hidden)]
#[sealed::sealed]
pub trait SubDeviceGroupHandle: Sync {
    /// Get the group's ID.
    fn id(&self) -> GroupId;

    /// Add a SubDevice device to this group.
    unsafe fn push(&self, subdevice: SubDevice) -> Result<(), Error>;

    /// Get a reference to the group with const generic params erased.
    fn as_ref(&self) -> SubDeviceGroupRef<'_>;
}

#[sealed::sealed]
impl<const MAX_SUBDEVICES: usize, const MAX_PDI: usize, S> SubDeviceGroupHandle
    for SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, S>
where
    S: Sync,
{
    fn id(&self) -> GroupId {
        self.id
    }

    unsafe fn push(&self, subdevice: SubDevice) -> Result<(), Error> {
        (*self.inner.get())
            .subdevices
            .push(subdevice)
            .map_err(|_| Error::Capacity(crate::error::Item::SubDevice))
    }

    fn as_ref(&self) -> SubDeviceGroupRef<'_> {
        SubDeviceGroupRef {
            max_pdi_len: MAX_PDI,
            inner: {
                let inner = unsafe { fmt::unwrap_opt!(self.inner.get().as_mut()) };

                GroupInnerRef {
                    subdevices: &mut inner.subdevices,
                    pdi_start: &mut inner.pdi_start,
                }
            },
        }
    }
}

#[derive(Debug)]
struct GroupInnerRef<'a> {
    subdevices: &'a mut [SubDevice],
    pdi_start: &'a mut PdiOffset,
}

/// A reference to a [`SubDeviceGroup`](crate::SubDeviceGroup) returned by the closure passed to
/// [`MainDevice::init`](crate::MainDevice::init).
#[doc(alias = "SlaveGroupRef")]
pub struct SubDeviceGroupRef<'a> {
    /// Maximum PDI length in bytes.
    max_pdi_len: usize,
    inner: GroupInnerRef<'a>,
}

impl<'a> SubDeviceGroupRef<'a> {
    /// Initialise all SubDevices in the group and place them in PRE-OP.
    // Clippy: shush
    #[allow(clippy::wrong_self_convention)]
    pub(crate) async fn into_pre_op<'sto>(
        &mut self,
        pdi_position: PdiOffset,
        maindevice: &'sto MainDevice<'sto>,
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
            let mut subdevice_config =
                SubDeviceRef::new(maindevice, subdevice.configured_address(), subdevice);

            // TODO: Move PRE-OP transition out of this so we can do it for the group just once
            subdevice_config.configure_mailboxes().await?;
        }

        Ok(pdi_position.increment(self.max_pdi_len as u16))
    }
}
