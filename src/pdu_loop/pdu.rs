use crate::{command::Command, error::PduError, pdu_loop::frame_header::FrameHeader, LEN_MASK};
use cookie_factory::{
    bytes::{le_u16, le_u8},
    combinator::{skip, slice},
    gen_simple, GenError,
};
use packed_struct::prelude::*;

#[derive(Debug, Clone, Default)]
pub struct Pdu {
    command: Command,
    pub index: u8,
    pub flags: PduFlags,
    irq: u16,
    working_counter: u16,
}

impl Pdu {
    pub fn replace(
        &mut self,
        command: Command,
        data_length: u16,
        index: u8,
    ) -> Result<(), PduError> {
        self.command = command;
        self.flags = PduFlags::with_len(data_length);
        self.irq = 0;
        self.index = index;
        self.working_counter = 0;

        Ok(())
    }

    pub fn set_response(&mut self, flags: PduFlags, irq: u16, working_counter: u16) {
        self.flags = flags;
        self.irq = irq;
        self.working_counter = working_counter;
    }

    /// The size of the total payload to be insterted into an EtherCAT frame.
    pub(crate) fn ethercat_payload_len(&self) -> u16 {
        // TODO: Add unit test to stop regressions
        let pdu_overhead = 12;

        self.flags.len() + pdu_overhead
    }

    /// Write an EtherCAT frame into `buf`.
    pub fn to_ethernet_payload<'a>(&self, buf: &'a mut [u8], data: &[u8]) -> Result<(), PduError> {
        let header = FrameHeader::pdu(self.ethercat_payload_len());

        let buf = gen_simple(le_u16(header.0), buf).map_err(PduError::Encode)?;

        let buf = gen_simple(le_u8(self.command.code() as u8), buf)?;
        let buf = gen_simple(le_u8(self.index), buf)?;

        // Write address and register data
        let buf = gen_simple(slice(self.command.address()?), buf)?;

        let buf = gen_simple(le_u16(u16::from_le_bytes(self.flags.pack().unwrap())), buf)?;
        let buf = gen_simple(le_u16(self.irq), buf)?;

        // Probably a read; the data area of the frame to send could be any old garbage, so we'll
        // skip over it.
        let buf = if data.is_empty() {
            gen_simple(skip(usize::from(self.flags.len())), buf)?
        }
        // Probably a write
        else {
            gen_simple(slice(data), buf)?
        };

        // Working counter is always zero when sending
        let buf = gen_simple(le_u16(0u16), buf)?;

        if !buf.is_empty() {
            log::error!(
                "Expected fully used buffer, got {} bytes left instead",
                buf.len()
            );

            Err(PduError::Encode(GenError::BufferTooBig(buf.len())))
        } else {
            Ok(())
        }
    }

    pub fn index(&self) -> u8 {
        self.index
    }

    pub fn command(&self) -> Command {
        self.command
    }

    pub(crate) fn working_counter(&self) -> u16 {
        self.working_counter
    }
}

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
