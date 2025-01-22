use crate::SubDeviceState;
use ethercrab_wire::{EtherCrabWireRead, EtherCrabWireSized};

/// Response information from transmitting the Process Data Image (PDI).
#[derive(Debug, PartialEq)]
#[non_exhaustive]
pub struct TxRxResponse<const N: usize, T = ()> {
    /// Working counter.
    pub working_counter: u16,

    /// The status of all SubDevices **within this group**.
    pub subdevice_states: heapless::Vec<SubDeviceState, N>,

    /// Additional data, for example a [`CycleInfo`](crate::subdevice_group::CycleInfo) struct
    /// holding Distributed Clocks information.
    pub extra: T,
}

bitflags::bitflags! {
    #[derive(Debug, Copy, Clone, PartialEq, Eq)]
    pub struct GroupState: u8 {
        /// No state recorded/read/known.
        const NONE = 0x00;
        /// EtherCAT `INIT` state.
        const INIT = 0x01;
        /// EtherCAT `PRE-OP` state.
        const PRE_OP = 0x02;
        // Commented out as it creates an ambiguity between INIT | PREOP and BOOTSTRAP.
        // /// EtherCAT `BOOT` state.
        // const BOOTSTRAP = 0x03;
        /// EtherCAT `SAFE-OP` state.
        const SAFE_OP = 0x04;
        /// EtherCAT `OP` state.
        const OP = 0x08;
    }
}

impl EtherCrabWireSized for GroupState {
    const PACKED_LEN: usize = 1;

    type Buffer = [u8; Self::PACKED_LEN];

    fn buffer() -> Self::Buffer {
        [0u8; Self::PACKED_LEN]
    }
}

impl EtherCrabWireRead for GroupState {
    fn unpack_from_slice(buf: &[u8]) -> Result<Self, ethercrab_wire::WireError> {
        u8::unpack_from_slice(buf)
            .and_then(|value| Self::from_bits(value).ok_or(ethercrab_wire::WireError::InvalidValue))
    }
}

impl<const N: usize, T> TxRxResponse<N, T> {
    /// Get a bitmap of all SubDevice states in this group.
    pub fn group_state(&self) -> GroupState {
        let bitmap = self
            .subdevice_states
            .iter()
            .fold(0u8, |acc, state| acc | u8::from(*state));

        GroupState::from_bits_truncate(bitmap)
    }

    /// If all SubDevices in the group are in the same state, return that state.
    ///
    /// If more than one state is present in the group, `None` will be returned.
    pub fn group_in_single_state(&self) -> Option<SubDeviceState> {
        let state = self.group_state();

        if state.bits().count_ones() > 1 {
            None
        } else {
            match state {
                GroupState::NONE => Some(SubDeviceState::None),
                GroupState::INIT => Some(SubDeviceState::Init),
                GroupState::PRE_OP => Some(SubDeviceState::PreOp),
                GroupState::SAFE_OP => Some(SubDeviceState::SafeOp),
                GroupState::OP => Some(SubDeviceState::Op),
                _ => None,
            }
        }
    }

    /// Test if every SubDevice in the group is in the same given state.
    ///
    /// Note that testing for `SubDeviceState::Bootstrap` will always return false due to
    /// ambiguities between between `INIT | PREOP`` and `BOOTSTRAP`.
    pub fn is_in_state(&self, desired_state: SubDeviceState) -> bool {
        let state = self.group_state();

        match desired_state {
            SubDeviceState::None => state == GroupState::NONE,
            SubDeviceState::Init => state == GroupState::INIT,
            SubDeviceState::PreOp => state == GroupState::PRE_OP,
            // Always false as this is ambiguous between INIT | PREOP and BOOTSTRAP
            SubDeviceState::Bootstrap => false,
            SubDeviceState::SafeOp => state == GroupState::SAFE_OP,
            SubDeviceState::Op => state == GroupState::OP,
            SubDeviceState::Other(n) => state.bits() == n,
        }
    }

    /// A helper method to ease EtherCrab version upgrades.
    pub fn all_op(&self) -> bool {
        self.group_in_single_state()
            .filter(|s| *s == SubDeviceState::Op)
            .is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_op() {
        let all_op = TxRxResponse {
            working_counter: 0,
            subdevice_states: {
                let mut v = heapless::Vec::<_, 3>::new();

                let _ = v.push(SubDeviceState::Op);
                let _ = v.push(SubDeviceState::Op);
                let _ = v.push(SubDeviceState::Op);

                v
            },
            extra: (),
        };

        let some_op = TxRxResponse {
            working_counter: 0,
            subdevice_states: {
                let mut v = heapless::Vec::<_, 3>::new();

                let _ = v.push(SubDeviceState::Op);
                let _ = v.push(SubDeviceState::SafeOp);
                let _ = v.push(SubDeviceState::Op);

                v
            },
            extra: (),
        };

        assert_eq!(all_op.is_in_state(SubDeviceState::Op), true);
        assert_eq!(some_op.is_in_state(SubDeviceState::Op), false);
    }

    #[test]
    fn none_state() {
        let res = TxRxResponse {
            working_counter: 0,
            subdevice_states: {
                let mut v = heapless::Vec::<_, 3>::new();

                let _ = v.push(SubDeviceState::Init);
                let _ = v.push(SubDeviceState::PreOp);
                let _ = v.push(SubDeviceState::PreOp);

                v
            },
            extra: (),
        };

        assert_eq!(res.is_in_state(SubDeviceState::SafeOp), false);
    }
}
