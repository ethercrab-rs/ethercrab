use crate::slave_state::SlaveState;

/// The AL control/status word for an individual slave device.
///
/// Defined in ETG1000.6 Table 9 - AL Control Description.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, ethercrab_wire::EtherCrabWireReadWrite)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[wire(bits = 16)]
pub struct AlControl {
    /// AL status.
    #[wire(bits = 4)]
    pub state: SlaveState,
    /// Error flag.
    #[wire(bits = 1)]
    pub error: bool,
    /// ID request flag.
    #[wire(bits = 1, post_skip = 10)]
    pub id_request: bool,
}

impl AlControl {
    pub fn new(state: SlaveState) -> Self {
        Self {
            state,
            error: false,
            id_request: false,
        }
    }

    pub fn reset() -> Self {
        Self {
            state: SlaveState::Init,
            // Acknowledge error
            error: true,
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ethercrab_wire::{EtherCrabWireRead, EtherCrabWireWriteSized};

    #[test]
    fn al_control() {
        let value = AlControl {
            state: SlaveState::SafeOp,
            error: true,
            id_request: false,
        };

        let packed = value.pack();

        assert_eq!(packed, [0x04 | 0x10, 0x00]);
    }

    #[test]
    fn unpack() {
        let value = AlControl {
            state: SlaveState::SafeOp,
            error: true,
            id_request: false,
        };

        let parsed = AlControl::unpack_from_slice(&[0x04 | 0x10, 0x00]).unwrap();

        assert_eq!(value, parsed);
    }

    #[test]
    fn unpack_short() {
        let parsed = AlControl::unpack_from_slice(&[0x04 | 0x10]);

        assert!(parsed.is_err());
    }
}
