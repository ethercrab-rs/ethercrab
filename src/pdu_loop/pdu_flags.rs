use crate::LEN_MASK;
use ethercrab_wire::WireError;

/// PDU fields placed after ADP and ADO, e.g. `LEN`, `C` and `NEXT` fields in ETG1000.4 5.4.1.2
/// Table 14 – Auto increment physical read (APRD).
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

impl ethercrab_wire::EtherCatWire for PduFlags {
    const BYTES: usize = 2;

    type Arr = [u8; 2];

    fn pack_to_slice_unchecked<'buf>(&self, buf: &'buf mut [u8]) -> &'buf [u8] {
        let raw = self.length & LEN_MASK
            | (self.circulated as u16) << 14
            | (self.is_not_last as u16) << 15;

        let buf = &mut buf[0..Self::BYTES];

        buf.copy_from_slice(&raw.to_le_bytes());

        buf
    }

    fn pack(&self) -> Self::Arr {
        let mut buf = [0u8; 2];

        self.pack_to_slice_unchecked(&mut buf);

        buf
    }

    fn unpack_from_slice(buf: &[u8]) -> Result<Self, WireError> {
        let buf = buf.get(0..Self::BYTES).ok_or(WireError::Todo)?;

        let src = u16::from_le_bytes(buf.try_into().unwrap());

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
    use ethercrab_wire::EtherCatWire;

    use super::*;

    #[test]
    fn pdu_flags_round_trip() {
        let flags = PduFlags {
            length: 0x110,
            circulated: false,
            is_not_last: true,
        };

        let packed = flags.pack();

        assert_eq!(packed, [0x10, 0x81]);

        let unpacked = PduFlags::unpack_from_slice(&packed).unwrap();

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

        assert_eq!(flags.pack(), [0b0000_1100, 0b0000_0100]);
        assert_eq!(flags.pack(), [0x0c, 0x04]);
    }
}
