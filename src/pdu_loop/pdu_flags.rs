use crate::LEN_MASK;
use packed_struct::{PackedStruct, PackedStructInfo};

#[derive(Default, Copy, Clone, Debug, PartialEq, Eq)]
pub struct PduFlags {
    /// Data length of this PDU.
    pub(crate) length: u16,
    /// Circulating frame
    ///
    /// 0: Frame is not circulating,
    /// 1: Frame has circulated once
    circulated: bool,
    /// 0: last EtherCAT PDU in EtherCAT frame
    /// 1: EtherCAT PDU in EtherCAT frame follows
    is_not_last: bool,
}

impl PackedStruct for PduFlags {
    type ByteArray = [u8; 2];

    fn pack(&self) -> packed_struct::PackingResult<Self::ByteArray> {
        let raw = self.length & LEN_MASK
            | (self.circulated as u16) << 14
            | (self.is_not_last as u16) << 15;

        Ok(raw.to_le_bytes())
    }

    fn unpack(src: &Self::ByteArray) -> packed_struct::PackingResult<Self> {
        let src = u16::from_le_bytes(*src);

        let length = src & LEN_MASK;
        let circulated = (src >> 14) & 0x01 == 0x01;
        let is_not_last = (src >> 15) & 0x01 == 0x01;

        Ok(Self {
            length,
            circulated,
            is_not_last,
        })
    }
}

impl PackedStructInfo for PduFlags {
    fn packed_bits() -> usize {
        8 * 2
    }
}

impl PduFlags {
    pub const fn with_len(len: u16) -> Self {
        Self {
            length: len,
            circulated: false,
            is_not_last: false,
        }
    }

    pub const fn len(self) -> u16 {
        self.length
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pdu_flags_round_trip() {
        let flags = PduFlags {
            length: 0x110,
            circulated: false,
            is_not_last: true,
        };

        let packed = flags.pack().unwrap();

        assert_eq!(packed, [0x10, 0x81]);

        let unpacked = PduFlags::unpack(&packed).unwrap();

        assert_eq!(unpacked, flags);
    }

    #[test]
    fn correct_length() {
        let flags = PduFlags {
            length: 1036,
            circulated: false,
            is_not_last: false,
        };

        assert_eq!(flags.len(), 1036);

        assert_eq!(flags.pack().unwrap(), [0b0000_1100, 0b0000_0100]);
        assert_eq!(flags.pack().unwrap(), [0x0c, 0x04]);
    }
}
