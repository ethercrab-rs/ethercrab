/// Fieldbus Memory Management Unit (FMMU).
///
/// Used to map segments of the Process Data Image (PDI) to various parts of the slave memory space.
use core::fmt;
use packed_struct::prelude::*;

/// ETG1000.4 Table 56 – Fieldbus memory management unit (FMMU) entity.
#[derive(Default, Copy, Clone, PackedStruct, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[packed_struct(bit_numbering = "msb0", endian = "lsb", size_bytes = "16")]
pub struct Fmmu {
    /// This parameter shall contain the start address in octets in the logical memory area of the
    /// memory translation.
    #[packed_field(bytes = "0..=3")]
    pub logical_start_address: u32,

    #[packed_field(bytes = "4..=5")]
    pub length_bytes: u16,

    #[packed_field(bytes = "6", size_bits = "3")]
    pub logical_start_bit: u8,

    #[packed_field(bytes = "7", size_bits = "3")]
    pub logical_end_bit: u8,

    #[packed_field(bytes = "8..=9")]
    pub physical_start_address: u16,

    #[packed_field(bytes = "10", size_bits = "3")]
    pub physical_start_bit: u8,

    // 11th byte, last bit
    #[packed_field(bits = "95")]
    pub read_enable: bool,

    // 11th byte, penultimate bit
    #[packed_field(bits = "94")]
    pub write_enable: bool,

    // 12th byte, last bit
    #[packed_field(bits = "103")]
    pub enable: bool,
    // Encoded in `size_bytes` attribute of `packed_struct`.
    // pub reserved_1: u8,
    // pub reserved_2: u16,
}

impl fmt::Debug for Fmmu {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Fmmu")
            .field(
                "logical_start_address",
                &format_args!("{:#010x}", self.logical_start_address),
            )
            .field("length_bytes", &self.length_bytes)
            .field("logical_start_bit", &self.logical_start_bit)
            .field("logical_end_bit", &self.logical_end_bit)
            .field(
                "physical_start_address",
                &format_args!("{:#06x}", self.physical_start_address),
            )
            .field("physical_start_bit", &self.physical_start_bit)
            .field("read_enable", &self.read_enable)
            .field("write_enable", &self.write_enable)
            .field("enable", &self.enable)
            .finish()
    }
}

impl fmt::Display for Fmmu {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!(
            "logical start {:#010x}:{}, size {}, logical end bit {}, physical start {:#06x}:{}, {}{}, {}",
            self.logical_start_address,
            self.logical_start_bit,
            self.length_bytes,
            self.logical_end_bit,
            self.physical_start_address,
            self.physical_start_bit,
            if self.read_enable { "R" } else { "" },
            if self.write_enable { "W" } else { "O" },
            if self.enable{ "enabled" } else { "disabled" },
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem;

    #[test]
    fn default_is_zero() {
        assert_eq!(Fmmu::default().pack().unwrap(), [0u8; 16]);
    }

    #[test]
    fn size() {
        // Unpacked size
        assert_eq!(mem::size_of::<Fmmu>(), 16);
        // Packed size
        assert_eq!(Fmmu::packed_bytes_size(None).unwrap(), 16);
    }

    #[test]
    fn decode_one() {
        let raw = [
            // Logical start address
            0x00, 0x00, 0x00, 0x00, //
            // Length
            0x01, 0x00, //
            // Logical start bit
            0x00, //
            // Logical end bit
            0x03, //
            // Physical start address
            0x00, 0x10, //
            // Phyiscal start bit
            0x00, //
            // Read/write enable
            0x01, //
            // FMMU enable
            0x01, //
            // Padding
            0x00, 0x00, 0x00,
        ];

        let fmmu = Fmmu::unpack_from_slice(&raw).unwrap();

        assert_eq!(
            fmmu,
            Fmmu {
                logical_start_address: 0,
                length_bytes: 1,
                logical_start_bit: 0,
                logical_end_bit: 3,
                physical_start_address: 0x1000,
                physical_start_bit: 0,
                read_enable: true,
                write_enable: false,
                enable: true,
            }
        )
    }
}
