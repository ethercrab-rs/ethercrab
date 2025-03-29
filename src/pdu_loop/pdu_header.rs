use crate::pdu_loop::pdu_flags::PduFlags;

/// A single PDU header, command, index, flags and IRQ.
#[derive(Debug, Copy, Clone, ethercrab_wire::EtherCrabWireReadWrite)]
#[wire(bytes = 10)]
pub struct PduHeader {
    /// Raw command  code.
    #[wire(bytes = 1)]
    pub command_code: u8,

    /// EtherCAT frame index.
    #[wire(bytes = 1)]
    pub index: u8,

    /// Raw command data.
    ///
    /// This represents 2x `u16` or 1x `u32` depending on the command.
    #[wire(bytes = 4)]
    pub command_raw: [u8; 4],

    /// PDU flags.
    #[wire(bytes = 2)]
    pub flags: PduFlags,

    /// IRQ.
    #[wire(bytes = 2)]
    pub irq: u16,
}

impl PduHeader {
    /// A hacked equality check used for replay tests only.
    ///
    /// It treats `command_raw` specially as this can change in responses.
    ///
    /// Please do not use outside the replay tests.
    #[doc(hidden)]
    #[allow(unused)]
    pub fn test_only_hacked_equal(&self, other: &Self) -> bool {
        self.command_code == other.command_code
            && self.index == other.index
            && if matches!(self.command_code, 4 | 5) {
                self.command_raw == other.command_raw
            } else {
                true
            }
            // && self.flags == other.flags
            && self.irq == other.irq
    }

    /// Similar to [`test_only_hacked_equal`].
    ///
    /// Please do not use outside replay tests.
    #[doc(hidden)]
    #[allow(unused)]
    pub fn test_only_hacked_hash(&self, state: &mut impl core::hash::Hasher) {
        use core::hash::Hash;

        let PduHeader {
            command_code,
            index,
            command_raw,
            flags: _,
            irq,
        } = *self;

        command_code.hash(state);
        index.hash(state);

        if matches!(command_code, 4 | 5) {
            command_raw.hash(state);
        }

        irq.hash(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::hash::{Hash, Hasher};
    use ethercrab_wire::EtherCrabWireRead;
    use std::collections::{HashMap, hash_map::DefaultHasher};

    // These shouldn't be derived for general use, just for testing
    impl Eq for PduHeader {}
    impl PartialEq for PduHeader {
        fn eq(&self, other: &Self) -> bool {
            self.test_only_hacked_equal(other)
        }
    }
    impl Hash for PduHeader {
        fn hash<H: Hasher>(&self, state: &mut H) {
            self.test_only_hacked_hash(state);
        }
    }

    #[test]
    fn decode() {
        // FPRD reg 0x900, 16 bytes
        let packet_bytes = [
            0x04, 0x12, 0x00, 0x10, 0x00, 0x09, 0x10, 0x00, 0x00, 0x00, 0x0a, 0xc9, 0x83, 0xcc,
            0x9c, 0xcd, 0x83, 0xcc, 0x00, 0x00, 0x00, 0x00, 0x56, 0x65, 0x72, 0x6c, 0x01, 0x00,
        ];

        let header = PduHeader::unpack_from_slice(&packet_bytes);

        assert_eq!(
            header,
            Ok(PduHeader {
                command_code: 0x04,
                index: 0x12,
                command_raw: [0x00, 0x10, 0x00, 0x09],
                flags: PduFlags {
                    length: 16,
                    circulated: false,
                    more_follows: false
                },
                irq: 0
            })
        );
    }

    // Just a sanity check...
    #[test]
    fn preamble_eq() {
        let a = PduHeader {
            command_code: 2,
            index: 0,
            command_raw: [0, 0, 0, 0],
            flags: PduFlags {
                length: 1,
                circulated: false,
                more_follows: false,
            },
            irq: 0,
        };

        let b = PduHeader {
            command_code: 2,
            index: 0,
            command_raw: [0, 0, 0, 0],
            flags: PduFlags {
                length: 1,
                circulated: false,
                more_follows: false,
            },
            irq: 0,
        };

        assert_eq!(a, b);
    }

    #[test]
    fn preamble_brd_eq() {
        let a = PduHeader {
            command_code: 7,
            index: 0,
            command_raw: [0, 0, 0, 0],
            flags: PduFlags {
                length: 1,
                circulated: false,
                more_follows: false,
            },
            irq: 0,
        };

        let b = PduHeader {
            command_code: 7,
            index: 0,
            command_raw: [1, 0, 0, 0],
            flags: PduFlags {
                length: 1,
                circulated: false,
                more_follows: false,
            },
            irq: 0,
        };

        // Different `command_raw` but `command_code` is BRD so the equality should still hold.
        assert_eq!(a, b);

        let mut state_a = DefaultHasher::new();
        let mut state_b = DefaultHasher::new();

        a.hash(&mut state_a);
        b.hash(&mut state_b);

        // Hashes remain equal because we look up by sent preamble, not the potentially modified
        // receive.
        assert_eq!(state_a.finish(), state_b.finish());
    }

    #[test]
    fn find_brd() {
        let mut map = HashMap::new();

        map.insert(
            PduHeader {
                command_code: 7,
                index: 0,
                command_raw: [3, 0, 0, 0],
                flags: PduFlags {
                    length: 1,
                    circulated: false,
                    more_follows: false,
                },
                irq: 0,
            },
            1234usize,
        );

        assert_eq!(
            map.get(&PduHeader {
                command_code: 7,
                index: 0,
                command_raw: [0, 0, 0, 0],
                flags: PduFlags {
                    length: 1,
                    circulated: false,
                    more_follows: false,
                },
                irq: 0,
            }),
            Some(&1234usize)
        );
    }

    #[test]
    fn find_bwr() {
        let mut map = HashMap::new();

        map.insert(
            PduHeader {
                command_code: 8,
                index: 1,
                command_raw: [3, 0, 32, 1],
                flags: PduFlags {
                    length: 2,
                    circulated: false,
                    more_follows: false,
                },
                irq: 0,
            },
            1234usize,
        );

        assert_eq!(
            map.get(&PduHeader {
                command_code: 8,
                index: 1,
                command_raw: [0, 0, 32, 1],
                flags: PduFlags {
                    length: 2,
                    circulated: false,
                    more_follows: false,
                },
                irq: 0,
            }),
            Some(&1234usize)
        );
    }
}
