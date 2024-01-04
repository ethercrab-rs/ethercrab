/// Fieldbus Memory Management Unit (FMMU).
///
/// Used to map segments of the Process Data Image (PDI) to various parts of the slave memory space.
use core::fmt;

/// ETG1000.4 Table 56 – Fieldbus memory management unit (FMMU) entity.
#[derive(Default, Copy, Clone, PartialEq, Eq, ethercrab_wire::EtherCrabWireReadWrite)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[wire(bytes = 16)]
pub struct Fmmu {
    /// This parameter shall contain the start address in octets in the logical memory area of the
    /// memory translation.
    #[wire(bytes = 4)]
    pub logical_start_address: u32,

    #[wire(bytes = 2)]
    pub length_bytes: u16,

    #[wire(bits = 3, post_skip = 5)]
    pub logical_start_bit: u8,

    #[wire(bits = 3, post_skip = 5)]
    pub logical_end_bit: u8,

    #[wire(bytes = 2)]
    pub physical_start_address: u16,

    #[wire(bits = 3, post_skip = 5)]
    pub physical_start_bit: u8,

    #[wire(bits = 1)]
    pub read_enable: bool,

    #[wire(bits = 1, post_skip = 6)]
    pub write_enable: bool,

    // Lots of spare bytes after this one!
    #[wire(bits = 1, post_skip = 31)]
    pub enable: bool,
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
    use ethercrab_wire::{EtherCrabWireRead, EtherCrabWireSized, EtherCrabWireWriteSized};

    #[test]
    fn default_is_zero() {
        assert_eq!(Fmmu::default().pack(), [0u8; 16]);
    }

    #[test]
    fn size() {
        // Unpacked size
        assert_eq!(mem::size_of::<Fmmu>(), 16);
        // Packed size
        assert_eq!(Fmmu::PACKED_LEN, 16);
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
