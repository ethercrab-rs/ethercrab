use crate::{error::WrappedPackingError, fmt, pdu_data::PduRead, slave_state::SlaveState};
use packed_struct::prelude::*;

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct AlControl {
    pub state: SlaveState,
    pub error: bool,
    pub id_request: bool,
}

impl PackedStruct for AlControl {
    type ByteArray = [u8; 2];

    fn pack(&self) -> packed_struct::PackingResult<Self::ByteArray> {
        let byte = (u8::from(self.state) & 0x0f)
            | ((self.error as u8) << 4)
            | ((self.id_request as u8) << 5);

        Ok([byte, 0])
    }

    fn unpack(src: &Self::ByteArray) -> packed_struct::PackingResult<Self> {
        let byte = src[0];

        fmt::trace!("AL raw byte {:#010b} (slice {:?})", byte, src);

        let state = SlaveState::try_from(byte & 0x0f).unwrap_or(SlaveState::Unknown);
        let error = (byte & (1 << 4)) > 0;
        let id_request = (byte & (1 << 5)) > 0;

        Ok(Self {
            state,
            error,
            id_request,
        })
    }
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

impl PduRead for AlControl {
    const LEN: u16 = u16::LEN;

    type Error = WrappedPackingError;

    fn try_from_slice(slice: &[u8]) -> Result<Self, Self::Error> {
        match Self::unpack_from_slice(slice) {
            Err(PackingError::InvalidValue) => Ok(Self {
                state: SlaveState::Unknown,
                ..Default::default()
            }),
            Err(e) => Err(e.into()),
            Ok(res) => Ok(res),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn al_control() {
        let value = AlControl {
            state: SlaveState::SafeOp,
            error: true,
            id_request: false,
        };

        let packed = value.pack().unwrap();

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
