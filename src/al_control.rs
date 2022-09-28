use crate::{al_status::AlState, PduRead};
use packed_struct::prelude::*;

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PackedStruct)]
#[packed_struct(size_bytes = "2", bit_numbering = "lsb0", endian = "lsb")]
pub struct AlControl {
    #[packed_field(bits = "8..=11", ty = "enum")]
    pub state: AlState,
    #[packed_field(bits = "12")]
    pub acknowledge: bool,
    #[packed_field(bits = "13")]
    pub id_request: bool,
}

impl AlControl {
    pub fn new(state: AlState) -> Self {
        Self {
            state,
            acknowledge: true,
            id_request: false,
        }
    }

    pub fn reset() -> Self {
        Self {
            state: AlState::Init,
            acknowledge: true,
            ..Default::default()
        }
    }
}

impl PduRead for AlControl {
    const LEN: u16 = u16::LEN;

    type Error = PackingError;

    fn try_from_slice(slice: &[u8]) -> Result<Self, Self::Error> {
        Self::unpack_from_slice(slice)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn al_control() {
        let value = AlControl {
            state: AlState::SafeOp,
            acknowledge: true,
            id_request: false,
        };

        let packed = value.pack().unwrap();

        assert_eq!(packed, [0x04 | 0x10, 0x00]);
    }

    #[test]
    fn unpack() {
        let value = AlControl {
            state: AlState::SafeOp,
            acknowledge: true,
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
