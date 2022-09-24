use packed_struct::prelude::*;

/// ETG1000.4 Table 56 – Fieldbus memory management unit (FMMU) entity.
#[derive(Default, Copy, Clone, Debug, PackedStruct, PartialEq, Eq)]
#[packed_struct(bit_numbering = "msb0", endian = "lsb")]
pub struct Fmmu {
    /// This parameter shall contain the start address in octets in the logical memory area of the memory translation.
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

    pub reserved_1: u8,
    pub reserved_2: u16,
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem;

    #[test]
    fn size() {
        // Unpacked size
        assert_eq!(mem::size_of::<Fmmu>(), 20);
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
                reserved_1: 0x00,
                reserved_2: 0x0000
            }
        )
    }
}
