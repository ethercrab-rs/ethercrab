//! Ethernet frame, copied from SmolTCP,
//! [here](https://github.com/smoltcp-rs/smoltcp/blob/53caf70f640d5ccb3cd1492e1cb178bc7dfa3cdd/src/wire/ethernet.rs)
//! at time of writing.
//!
//! Then drastically modified/stripped down to suit EtherCrab's needs.

use core::fmt;

use crate::error::{Error, PduError};

/// A six-octet Ethernet II address.
#[derive(Debug, Hash, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Default)]
pub struct EthernetAddress(pub [u8; 6]);

impl EthernetAddress {
    /// The broadcast address.
    pub const BROADCAST: EthernetAddress = EthernetAddress([0xff; 6]);

    /// Construct an Ethernet address from a sequence of octets, in big-endian.
    ///
    /// # Panics
    /// The function panics if `data` is not six octets long.
    pub fn from_bytes(data: &[u8]) -> EthernetAddress {
        let mut bytes = [0; 6];
        bytes.copy_from_slice(data);
        EthernetAddress(bytes)
    }

    /// Return an Ethernet address as a sequence of octets, in big-endian.
    pub const fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Display for EthernetAddress {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let bytes = self.0;
        write!(
            f,
            "{:02x}-{:02x}-{:02x}-{:02x}-{:02x}-{:02x}",
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5]
        )
    }
}

#[cfg(feature = "defmt")]
impl defmt::Format for EthernetAddress {
    fn format(&self, fmt: defmt::Formatter) {
        let bytes = self.0;
        defmt::write!(
            fmt,
            "{:02x}-{:02x}-{:02x}-{:02x}-{:02x}-{:02x}",
            bytes[0],
            bytes[1],
            bytes[2],
            bytes[3],
            bytes[4],
            bytes[5]
        )
    }
}

/// A read/write wrapper around an Ethernet II frame buffer.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct EthernetFrame<T: AsRef<[u8]>> {
    buffer: T,
}

mod field {
    use core::ops::{Range, RangeFrom};

    pub const DESTINATION: Range<usize> = 0..6;
    pub const SOURCE: Range<usize> = 6..12;
    pub const ETHERTYPE: Range<usize> = 12..14;
    pub const PAYLOAD: RangeFrom<usize> = 14..;
}

/// The Ethernet header length
pub const ETHERNET_HEADER_LEN: usize = field::PAYLOAD.start;

impl<T: AsRef<[u8]>> EthernetFrame<T> {
    /// Imbue a raw octet buffer with Ethernet frame structure.
    pub const fn new_unchecked(buffer: T) -> EthernetFrame<T> {
        EthernetFrame { buffer }
    }

    /// Shorthand for a combination of [new_unchecked] and [check_len].
    ///
    /// [new_unchecked]: #method.new_unchecked
    /// [check_len]: #method.check_len
    pub fn new_checked(buffer: T) -> Result<EthernetFrame<T>, Error> {
        let packet = Self::new_unchecked(buffer);
        packet.check_len()?;
        Ok(packet)
    }

    /// Ensure that no accessor method will panic if called.
    /// Returns `Err(Error)` if the buffer is too short.
    pub fn check_len(&self) -> Result<(), Error> {
        let len = self.buffer.as_ref().len();
        if len < ETHERNET_HEADER_LEN {
            Err(Error::Pdu(PduError::Ethernet))
        } else {
            Ok(())
        }
    }

    /// Consumes the frame, returning the underlying buffer.
    pub fn into_inner(self) -> T {
        self.buffer
    }

    /// Return the length of a frame header.
    pub const fn header_len() -> usize {
        ETHERNET_HEADER_LEN
    }

    /// Return the length of a buffer required to hold a packet with the payload
    /// of a given length.
    pub const fn buffer_len(payload_len: usize) -> usize {
        ETHERNET_HEADER_LEN + payload_len
    }

    /// Return the destination address field.
    #[inline]
    pub fn dst_addr(&self) -> EthernetAddress {
        let data = self.buffer.as_ref();
        EthernetAddress::from_bytes(&data[field::DESTINATION])
    }

    /// Return the source address field.
    #[inline]
    pub fn src_addr(&self) -> EthernetAddress {
        let data = self.buffer.as_ref();
        EthernetAddress::from_bytes(&data[field::SOURCE])
    }

    /// Return the EtherType field, without checking for 802.1Q.
    #[inline]
    pub fn ethertype(&self) -> u16 {
        let data = self.buffer.as_ref();

        // Ethernet is big-endian
        data.get(field::ETHERTYPE)
            .map(|res| u16::from_be_bytes(res.try_into().unwrap()))
            // EtherCrab only really cares whether the ethertype is 0x88a4, so defaulting to zero on
            // unparseable ethertypes is fine here (imo, lol)
            .unwrap_or(0)
    }
}

impl<'a, T: AsRef<[u8]> + ?Sized> EthernetFrame<&'a T> {
    /// Return a pointer to the payload, without checking for 802.1Q.
    #[inline]
    pub fn payload(&self) -> &'a [u8] {
        let data = self.buffer.as_ref();
        &data[field::PAYLOAD]
    }
}

impl<T: AsRef<[u8]> + AsMut<[u8]>> EthernetFrame<T> {
    /// Set the destination address field.
    #[inline]
    pub fn set_dst_addr(&mut self, value: EthernetAddress) {
        let data = self.buffer.as_mut();
        data[field::DESTINATION].copy_from_slice(value.as_bytes())
    }

    /// Set the source address field.
    #[inline]
    pub fn set_src_addr(&mut self, value: EthernetAddress) {
        let data = self.buffer.as_mut();
        data[field::SOURCE].copy_from_slice(value.as_bytes())
    }

    /// Set the EtherType field.
    #[inline]
    pub fn set_ethertype(&mut self, value: u16) {
        let data = self.buffer.as_mut();

        data[field::ETHERTYPE].copy_from_slice(&value.to_be_bytes());
    }

    /// Return a mutable pointer to the payload.
    #[inline]
    pub fn payload_mut(&mut self) -> &mut [u8] {
        let data = self.buffer.as_mut();
        &mut data[field::PAYLOAD]
    }
}

impl<T: AsRef<[u8]>> AsRef<[u8]> for EthernetFrame<T> {
    fn as_ref(&self) -> &[u8] {
        self.buffer.as_ref()
    }
}

impl<T: AsRef<[u8]>> fmt::Display for EthernetFrame<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "EthernetII src={} dst={} type={}",
            self.src_addr(),
            self.dst_addr(),
            self.ethertype()
        )
    }
}
