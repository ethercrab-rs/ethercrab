use core::ops::Range;

/// An accumulator that stores the bit and byte offsets in the PDI so SubDevice IO data can be mapped
/// to/from the PDI using FMMUs.
///
/// PDI mappings are byte-aligned per each SubDevice.
#[derive(Default, Debug, Copy, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct PdiOffset {
    pub start_address: u32,
    // // Unused, but will become useful if we support bit-packed PDI mappings in the future.
    // start_bit: u8,
}

impl PdiOffset {
    /// Increment the address accumulator by a given number of bits, aligned to the next byte.
    pub fn increment_byte_aligned(self, bits: u16) -> Self {
        let inc_bytes = (bits + 7) / 8;

        self.increment_inner(0, inc_bytes)
    }

    pub fn increment(self, bytes: u16) -> Self {
        self.increment_inner(0, bytes)
    }

    /// Common code shared between byte and bit aligned public methods.
    fn increment_inner(self, _inc_bits: u16, inc_bytes: u16) -> Self {
        // // Bit count overflows a byte, so move into the next byte's bits by incrementing the byte
        // // index one more.
        // let start_bit = if u16::from(self.start_bit) + inc_bits >= 8 {
        //     inc_bytes += 1;

        //     ((u16::from(self.start_bit) + inc_bits) % 8) as u8
        // } else {
        //     self.start_bit + inc_bits as u8
        // };

        Self {
            start_address: self.start_address + u32::from(inc_bytes),
            // start_bit,
        }
    }

    // /// Compute end bit 0-7 in the final byte of the mapped PDI section.
    // fn end_bit(self, bits: u16) -> u8 {
    //     // SAFETY: The modulos here and in `increment` mean that all value can comfortably fit in a
    //     // u8, so all the `as` and non-checked `+` here are fine.

    //     let bits = (bits.saturating_sub(1) % 8) as u8;

    //     self.start_bit + bits % 8
    // }

    /// Compute an index range between this offset (inclusive) and another (exclusive).
    pub fn up_to(self, other: Self) -> Range<usize> {
        self.start_address as usize..other.start_address as usize
    }

    // Maybe one day we support packed PDIs. In that instance, uncomment this and the tests below.
    // DO NOT DELETE as part of cleanup
    // /// Increment, calculating values for _next_ mapping when the struct is read after increment.
    // fn increment(self, bits: u16) -> Self {
    //     let inc_bytes = bits / 8;
    //     let inc_bits = bits % 8;

    //     self.increment_inner(inc_bits, inc_bytes)
    // }
}

#[derive(Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct PdiSegment {
    pub bytes: Range<usize>,
    // pub bit_len: usize,
}

impl PdiSegment {
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    // pub fn is_empty(&self) -> bool {
    //     self.len() == 0
    // }
}

impl core::fmt::Display for PdiSegment {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if !self.bytes.is_empty() {
            write!(f, "{:#010x}..{:#010x}", self.bytes.start, self.bytes.end,)
        } else {
            f.write_str("(empty)")
        }
    }
}

// impl PdiSegment {
//     /// If this segment contains less than 8 bits, this method will calculate the bit mask for the
//     /// used bits.
//     pub fn bit_mask(self) -> Option<u8> {
//         (self.bit_len < 8).then(|| 2u8.pow(self.bit_len as u32) - 1)
//     }
// }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el2004_byte_aligned() {
        let input = PdiOffset::default();

        let input = input.increment_byte_aligned(4);

        assert_eq!(input, PdiOffset { start_address: 1 }, "first increment");

        let input = input.increment_byte_aligned(4);

        assert_eq!(input, PdiOffset { start_address: 2 }, "second increment");
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn fuzz_pdi_segment() {
        heckcheck::check(|(start_address, incr_bits): (u32, u16)| {
            let offset = PdiOffset { start_address };

            let new = offset.increment_byte_aligned(incr_bits);

            let incr_bits = u32::from(incr_bits);
            let incr_bytes = (incr_bits + 7) / 8;

            assert_eq!(
                new.start_address,
                offset.start_address + incr_bytes,
                "incorrect increment"
            );
            // assert_eq!(new.start_bit, 0, "not byte aligned");

            Ok(())
        });
    }

    // Maybe one day we support packed PDIs. DO NOT DELETE as part of cleanup.
    // #[test]
    // fn size_bytes() {
    //     // E.g. 2x EL2004, 1x EL1004
    //     let input = MappingOffset::default()
    //         .increment(4)
    //         .increment(4)
    //         .increment(4);

    //     assert_eq!(input.size_bytes(), 2);
    // }

    // #[test]
    // fn simulate_2_el2004() {
    //     let input = MappingOffset::default();

    //     let input = input.increment(4);

    //     assert_eq!(
    //         input,
    //         MappingOffset {
    //             start_address: 0,
    //             start_bit: 4
    //         }
    //     );

    //     let input = input.increment(4);

    //     assert_eq!(
    //         input,
    //         MappingOffset {
    //             start_address: 1,
    //             start_bit: 0
    //         }
    //     );
    // }

    // #[test]
    // fn end_bit() {
    //     let input = MappingOffset::default();

    //     assert_eq!(input.end_bit(4), 3);

    //     let input = input.increment(4);

    //     assert_eq!(input.end_bit(4), 7);

    //     let input = input.increment(4);

    //     assert_eq!(input.end_bit(4), 3);
    // }

    // #[test]
    // fn zero_length_end_bit() {
    //     let input = MappingOffset::default();

    //     assert_eq!(input.end_bit(0), 0);

    //     let input = input.increment(4);

    //     assert_eq!(input.end_bit(0), 4);
    // }

    // #[test]
    // fn cross_boundary() {
    //     let input = MappingOffset::default();

    //     let input = input.increment(6);

    //     assert_eq!(
    //         input,
    //         MappingOffset {
    //             start_address: 0,
    //             start_bit: 6
    //         }
    //     );

    //     let input = input.increment(6);

    //     assert_eq!(
    //         input,
    //         MappingOffset {
    //             start_address: 1,
    //             start_bit: 4
    //         }
    //     );
    // }
}
