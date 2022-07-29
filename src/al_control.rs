use crate::{al_status::AlState, PduRead};
use packed_struct::prelude::*;

#[derive(Copy, Clone, Debug, PartialEq, PackedStruct)]
#[packed_struct(size_bytes = "2", bit_numbering = "lsb0", endian = "lsb")]
pub struct AlControl {
    #[packed_field(bits = "8..=11", ty = "enum")]
    pub state: AlState,
    #[packed_field(bits = "12")]
    pub acknowledge: bool,
    #[packed_field(bits = "13")]
    pub id_request: bool,

    // Required, but AL control must write 2 bytes to be valid
    #[packed_field(bytes = "0")]
    _reserved: u8,
}

impl AlControl {
    pub fn new(state: AlState) -> Self {
        Self {
            state,
            acknowledge: true,
            id_request: false,
            _reserved: 0,
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
            _reserved: 0,
        };

        let packed = value.pack().unwrap();

        assert_eq!(packed, [0x00, 0x04 | 0x10]);
    }
}
