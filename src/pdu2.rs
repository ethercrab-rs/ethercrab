use crate::LEN_MASK;
use packed_struct::{prelude::*, types::bits::Bits};

// TODO: Logical PDU with 32 bit address
// TODO: Auto increment PDU with i16 address
pub struct Pdu<const N: usize> {
    command: Command,
    index: u8,
    register_address: u16,
    flags: PduFlags,
    irq: u16,
    working_counter: u16,
    data: [u8; N],
}

impl<const N: usize> Pdu<N> {
    pub const fn brd(register_address: u16) -> Self {
        debug_assert!(N < LEN_MASK as usize);

        Self {
            command: Command::Brd,
            index: 0,
            register_address,
            flags: PduFlags::with_len(N as u16),
            irq: 0,
            data: [0u8; N],
            working_counter: 0,
        }
    }

    pub const fn as_bytes(&self, buf: &mut [u8]) {
        // Best option

        //         #![no_std]

        // pub fn stuff(input: &mut [u8], lil_bit: &[u8]) -> Result<(), ()> {
        //     input.get_mut(0..lil_bit.len()).ok_or_else(|| ())?.copy_from_slice(lil_bit);

        //     Ok(())
        // }

        // Jonathan:

        // pub fn stuff(buf: &mut [u8], some_data: &[u8]) -> Result<(), ()> {
        //     buf.get_mut(0..some_data.len())
        //         .and_then(|b| b.copy_from_slice(some_data))
        //         .ok_or_else(|| ())
        // }

        // Best: https://godbolt.org/z/rdefsbb1s

        // Optimises panic out of ARM asm as well - note this when writing the real code

        // pub fn stuff(buf: &mut [u8], some_data: &[u8]) -> Result<(), ()> {
        //     if buf.len() >= some_data.len() {
        //         buf[..some_data.len()].copy_from_slice(some_data);
        //         Ok(())
        //     } else {
        //         Err(())
        //     }
        // }

        // Tbh, just go with copy_from_slice. It's good enough.
    }
}

enum Command {
    Aprd {
        /// Auto increment counter.
        address: i16,
    },
    Fprd {
        /// Configured station address.
        address: u16,
    },
    Brd,
    Lrd {
        /// Logical address.
        address: u32,
    },
}

impl Command {
    fn code(&self) -> CommandCode {
        match self {
            Self::Aprd { .. } => CommandCode::Aprd,
            Self::Fprd { .. } => CommandCode::Fprd,
            Self::Brd => CommandCode::Brd,
            Self::Lrd { .. } => CommandCode::Lrd,
        }
    }
}

/// Broadcast or configured station addressing.
// TODO: Packed struct derive
pub enum CommandCode {
    Aprd = 0x01,
    Fprd = 0x04,
    Brd = 0x07,
    Lrd = 0x0A,
}

#[derive(Copy, Clone, Debug, PackedStruct, PartialEq)]
// TODO: Fix endianness
#[packed_struct(size_bytes = "2", bit_numbering = "msb0", endian = "lsb")]
pub struct PduFlags {
    #[packed_field(bits = "0..=10")]
    length: u16,
    #[packed_field(bits = "11..=13")]
    _reserved: u8,
    /// Circulating frame
    ///
    /// 0: Frame is not circulating,
    /// 1: Frame has circulated once
    #[packed_field(bits = "14")]
    circulated: bool,
    /// 0: last EtherCAT PDU in EtherCAT frame
    /// 1: EtherCAT PDU in EtherCAT frame follows
    #[packed_field(bits = "15")]
    is_not_last: bool,
}

impl PduFlags {
    const fn with_len(len: u16) -> Self {
        Self {
            length: len,
            _reserved: 0,
            circulated: false,
            is_not_last: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_pdu_len() {
        let raw = 0b1100_0000_0000_0001u16;

        // TODO: Fix endianness
        let pdu_len = PduFlags::unpack_from_slice(&raw.to_be_bytes()).unwrap();

        assert_eq!(
            pdu_len,
            PduFlags {
                length: 1,
                _reserved: 0,
                circulated: true,
                is_not_last: true
            }
        );
    }
}
