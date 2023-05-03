use crate::{
    error::Error,
    pdu_data::{PduData, PduRead},
    Client, SubIndex,
};
use core::cell::UnsafeCell;

struct Slave {
    name: heapless::String<64>,
    configured_address: u16,
    // etc
}

struct SlaveRef<'a, S> {
    slave: &'a Slave,
    state: S,
}

impl<'a, S> SlaveRef<'a, S> {
    pub(crate) fn new(slave: &'a Slave) -> SlaveRef<'a, ()> {
        SlaveRef { slave, state: () }
    }

    pub(crate) fn with_state(slave: &'a Slave, state: S) -> SlaveRef<'a, S> {
        SlaveRef { slave, state }
    }

    pub(crate) fn into_state<S2>(&self, state: S2) -> SlaveRef<'a, S2> {
        SlaveRef {
            slave: self.slave,
            state,
        }
    }

    pub fn name(&self) -> &str {
        self.slave.name.as_str()
    }

    // Applicable to any slave state
    pub async fn write_sdo<'client, T>(
        &self,
        client: &'client Client<'client>,
        index: u16,
        sub_index: SubIndex,
        value: T,
    ) -> Result<(), Error>
    where
        T: PduData,
        <T as PduRead>::Error: core::fmt::Debug,
    {
        todo!()
    }
}

/// Typestate: A grouped slave with PDI
struct Pdi<'group> {
    i: &'group [u8],
    o: &'group mut [u8],
}

pub struct Group {
    slaves: heapless::Vec<Slave, 16>,
    io: UnsafeCell<[u8; 64]>,
}

impl Group {
    fn slave(&self, index: usize) -> Option<SlaveRef<'_, Pdi<'_>>> {
        self.slaves.get(index).map(|sl| {
            let state = Pdi {
                i: &unsafe { &*self.io.get() }[0..1],
                o: &mut unsafe { &mut *self.io.get() }[2..3],
            };

            SlaveRef::with_state(sl, state)
        })
    }
}
