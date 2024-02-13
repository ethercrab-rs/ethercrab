use super::storage::PduStorageRef;
use crate::{
    command::Command,
    error::{Error, PduError, PduValidationError},
    fmt,
    pdu_loop::{frame_header::FrameHeader, pdu_flags::PduFlags},
    ETHERCAT_ETHERTYPE, MASTER_ADDR,
};
use ethercrab_wire::{EtherCrabWireRead, EtherCrabWireSized};
use smoltcp::wire::{EthernetAddress, EthernetFrame};

/// EtherCAT frame receive adapter.
pub struct PduRx<'sto> {
    storage: PduStorageRef<'sto>,
    source_mac: EthernetAddress,
}

impl<'sto> PduRx<'sto> {
    pub(in crate::pdu_loop) fn new(storage: PduStorageRef<'sto>) -> Self {
        Self {
            storage,
            source_mac: MASTER_ADDR,
        }
    }

    /// Set the source MAC address to the given value.
    ///
    /// This is required on macOS (and BSD I believe) as the interface's MAC address cannot be
    /// overridden at the packet level for some reason.
    #[cfg(all(not(target_os = "linux"), unix))]
    pub(crate) fn set_source_mac(&mut self, new: EthernetAddress) {
        self.source_mac = new
    }

    /// Given a complete Ethernet II frame, parse a response PDU from it and wake the future that
    /// sent the frame.
    // NOTE: &mut self so this struct can only be used in one place.
    pub fn receive_frame(&mut self, ethernet_frame: &[u8]) -> Result<(), Error> {
        let raw_packet = EthernetFrame::new_checked(ethernet_frame)?;

        // Look for EtherCAT packets whilst ignoring broadcast packets sent from self. As per
        // <https://github.com/OpenEtherCATsociety/SOEM/issues/585#issuecomment-1013688786>, the
        // first slave will set the second bit of the MSB of the MAC address (U/L bit). This means
        // if we send e.g. 10:10:10:10:10:10, we receive 12:10:10:10:10:10 which passes through this
        // filter.
        if raw_packet.ethertype() != ETHERCAT_ETHERTYPE || raw_packet.src_addr() == self.source_mac
        {
            fmt::trace!("Ignore frame");

            return Ok(());
        }

        let i = raw_packet.payload();

        let header = FramePreamble::unpack_from_slice(i).map_err(|e| {
            fmt::error!("Failed to parse frame header: {}", e);

            e
        })?;

        let (data, working_counter) = header.data_wkc(i).map_err(|e| {
            fmt::error!("Could not get frame data/wkc: {}", e);

            e
        })?;

        let command = header.command()?;

        let FramePreamble {
            index, flags, irq, ..
        } = header;

        fmt::trace!(
            "Received frame with index {} ({:#04x}), WKC {}",
            index,
            index,
            working_counter,
        );

        let mut frame = self
            .storage
            .claim_receiving(index)
            .ok_or(PduError::InvalidIndex(index))?;

        if frame.index() != index {
            return Err(Error::Pdu(PduError::Validation(
                PduValidationError::IndexMismatch {
                    sent: frame.index(),
                    received: index,
                },
            )));
        }

        // Check for weird bugs where a slave might return a different command than the one sent for
        // this PDU index.
        if command.code() != frame.command().code() {
            return Err(Error::Pdu(PduError::Validation(
                PduValidationError::CommandMismatch {
                    sent: command,
                    received: frame.command(),
                },
            )));
        }

        let frame_data = frame.buf_mut();

        frame_data[0..usize::from(flags.len())].copy_from_slice(data);

        frame.mark_received(flags, irq, working_counter)?;

        Ok(())
    }
}

/// PDU frame header, command, index, flags and IRQ.
#[derive(Debug, Copy, Clone, ethercrab_wire::EtherCrabWireRead)]
#[wire(bytes = 12)]
pub struct FramePreamble {
    #[wire(bytes = 2)]
    header: FrameHeader,

    // NOTE: The following fields are included in the header length field value.
    #[wire(bytes = 1)]
    command_code: u8,
    #[wire(bytes = 1)]
    index: u8,
    #[wire(bytes = 4)]
    command_raw: [u8; 4],
    #[wire(bytes = 2)]
    flags: PduFlags,
    #[wire(bytes = 2)]
    irq: u16,
}

impl FramePreamble {
    fn data_wkc<'buf>(&self, buf: &'buf [u8]) -> Result<(&'buf [u8], u16), Error> {
        // Jump past header in the buffer
        let header_offset = FramePreamble::PACKED_LEN;

        // The length of the PDU data body. There are two bytes after this that hold the working
        // counter, but are not counted as part of the PDU length from the header.
        let data_end = usize::from(self.header.payload_len);

        let data = buf.get(header_offset..data_end).ok_or(PduError::Decode)?;
        let wkc = buf
            .get(data_end..)
            .ok_or(Error::Pdu(PduError::Decode))
            .and_then(|raw| Ok(u16::unpack_from_slice(raw)?))?;

        Ok((data, wkc))
    }

    fn command(&self) -> Result<Command, Error> {
        Command::parse_code_data(self.command_code, self.command_raw)
    }

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
                self.command_raw == other.command_raw &&
                self.header == other.header
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

        let FramePreamble {
            header,
            command_code,
            index,
            command_raw,
            flags: _,
            irq,
        } = *self;

        // header.hash(state);
        command_code.hash(state);
        index.hash(state);

        if matches!(command_code, 4 | 5) {
            header.payload_len.hash(state);
            command_raw.hash(state);
        }

        // flags.hash(state);
        irq.hash(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pdu_loop::frame_header::ProtocolType::DlPdu;
    use core::hash::{Hash, Hasher};
    use std::collections::{hash_map::DefaultHasher, HashMap};

    // These shouldn't be derived for general use, just for testing
    impl Eq for FramePreamble {}
    impl PartialEq for FramePreamble {
        fn eq(&self, other: &Self) -> bool {
            self.test_only_hacked_equal(&other)
        }
    }
    impl Hash for FramePreamble {
        fn hash<H: Hasher>(&self, state: &mut H) {
            self.test_only_hacked_hash(state);
        }
    }

    // Just a sanity check...
    #[test]
    fn preamble_eq() {
        let a = FramePreamble {
            header: FrameHeader {
                payload_len: 13,
                protocol: DlPdu,
            },
            command_code: 2,
            index: 0,
            command_raw: [0, 0, 0, 0],
            flags: PduFlags {
                length: 1,
                circulated: false,
                is_not_last: false,
            },
            irq: 0,
        };

        let b = FramePreamble {
            header: FrameHeader {
                payload_len: 13,
                protocol: DlPdu,
            },
            command_code: 2,
            index: 0,
            command_raw: [0, 0, 0, 0],
            flags: PduFlags {
                length: 1,
                circulated: false,
                is_not_last: false,
            },
            irq: 0,
        };

        assert_eq!(a, b);

        let mut state = DefaultHasher::new();

        assert_eq!(a.hash(&mut state), b.hash(&mut state));
    }

    #[test]
    fn preamble_brd_eq() {
        let a = FramePreamble {
            header: FrameHeader {
                payload_len: 13,
                protocol: DlPdu,
            },
            command_code: 7,
            index: 0,
            command_raw: [0, 0, 0, 0],
            flags: PduFlags {
                length: 1,
                circulated: false,
                is_not_last: false,
            },
            irq: 0,
        };

        let b = FramePreamble {
            header: FrameHeader {
                payload_len: 13,
                protocol: DlPdu,
            },
            command_code: 7,
            index: 0,
            command_raw: [1, 0, 0, 0],
            flags: PduFlags {
                length: 1,
                circulated: false,
                is_not_last: false,
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
            FramePreamble {
                header: FrameHeader {
                    payload_len: 13,
                    protocol: DlPdu,
                },
                command_code: 7,
                index: 0,
                command_raw: [3, 0, 0, 0],
                flags: PduFlags {
                    length: 1,
                    circulated: false,
                    is_not_last: false,
                },
                irq: 0,
            },
            1234usize,
        );

        assert_eq!(
            map.get(&FramePreamble {
                header: FrameHeader {
                    payload_len: 13,
                    protocol: DlPdu,
                },
                command_code: 7,
                index: 0,
                command_raw: [0, 0, 0, 0],
                flags: PduFlags {
                    length: 1,
                    circulated: false,
                    is_not_last: false,
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
            FramePreamble {
                header: FrameHeader {
                    payload_len: 14,
                    protocol: DlPdu,
                },
                command_code: 8,
                index: 1,
                command_raw: [3, 0, 32, 1],
                flags: PduFlags {
                    length: 2,
                    circulated: false,
                    is_not_last: false,
                },
                irq: 0,
            },
            1234usize,
        );

        assert_eq!(
            map.get(&FramePreamble {
                header: FrameHeader {
                    payload_len: 14,
                    protocol: DlPdu,
                },
                command_code: 8,
                index: 1,
                command_raw: [0, 0, 32, 1],
                flags: PduFlags {
                    length: 2,
                    circulated: false,
                    is_not_last: false,
                },
                irq: 0,
            }),
            Some(&1234usize)
        );
    }
}
