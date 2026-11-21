//! Vendor-specific mailbox payload framing and transfers.

use crate::{
    error::{Error, Item, MailboxError},
    mailbox::{coe::Coe, MailboxHeader, MailboxType, Priority},
    subdevice::{SubDevice, SubDeviceRef},
};
use core::ops::Deref;
use ethercrab_wire::EtherCrabWireWriteSized;

const VOE_HEADER_LEN: usize = 6;
const MAILBOX_HEADER_LEN: usize = 6;


/// The VoE header followed by an opaque vendor-defined payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Packet<'a> {
    /// Vendor identifier from the VoE header.
    pub vendor_id: u32,
    /// Vendor-defined message type.
    pub vendor_type: u16,
    /// Opaque vendor-owned payload.
    pub payload: &'a [u8],
}

impl<'a> Packet<'a> {
    /// Decode the six-byte VoE header and borrow its opaque payload.
    pub fn decode(bytes: &'a [u8]) -> Option<Self> {
        if bytes.len() < 6 {
            return None;
        }
        Some(Self {
            vendor_id: u32::from_le_bytes(bytes[..4].try_into().ok()?),
            vendor_type: u16::from_le_bytes(bytes[4..6].try_into().ok()?),
            payload: &bytes[6..],
        })
    }

    /// Encode into caller-provided storage, returning the encoded byte count.
    pub fn encode(self, output: &mut [u8]) -> Option<usize> {
        let end = 6usize.checked_add(self.payload.len())?;
        let header = output.get_mut(..6)?;
        header[..4].copy_from_slice(&self.vendor_id.to_le_bytes());
        header[4..].copy_from_slice(&self.vendor_type.to_le_bytes());
        output.get_mut(6..end)?.copy_from_slice(self.payload);
        Some(end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vendor_header_and_payload_roundtrip() {
        let packet = Packet { vendor_id: 0x1337, vendor_type: 1, payload: &[0xaa, 0xbb] };
        let mut bytes = [0; 8];
        assert_eq!(packet.encode(&mut bytes), Some(8));
        assert_eq!(Packet::decode(&bytes), Some(packet));
        assert_eq!(Packet::decode(&bytes[..5]), None);
    }

    #[test]
    fn vendor_echo_vector_matches_standard_header() {
        let packet = Packet { vendor_id: 0x1337, vendor_type: 1, payload: &[0xaa] };
        let mut bytes = [0; 7];
        assert_eq!(packet.encode(&mut bytes), Some(7));
        assert_eq!(bytes, [0x37, 0x13, 0, 0, 1, 0, 0xaa]);
    }
}

/// VoE client for a generic vendor-specific mailbox service.
pub struct Voe<'maindevice, S> {
    coe: Coe<'maindevice, S>,
}

impl<'maindevice, S> Voe<'maindevice, S>
where
    S: Deref<Target = SubDevice>,
{
    /// Create a VoE client for a discovered SubDevice.
    pub fn new(subdevice: &'maindevice SubDeviceRef<'maindevice, S>) -> Self {
        Self { coe: Coe::new(subdevice) }
    }

    fn mailbox_len(&self, write: bool) -> Result<usize, Error> {
        if !self
            .coe
            .subdevice
            .config
            .mailbox
            .supported_protocols
            .contains(crate::eeprom::types::MailboxProtocols::VOE)
        {
            return Err(Error::Mailbox(MailboxError::NoWriteMailbox));
        }
        let mailbox = if write {
            self.coe.subdevice.config.mailbox.write
                .ok_or(Error::Mailbox(MailboxError::NoReadMailbox))?
        } else {
            self.coe.subdevice.config.mailbox.read
                .ok_or(Error::Mailbox(MailboxError::NoWriteMailbox))?
        };
        let len = usize::from(mailbox.len);
        if len <= MAILBOX_HEADER_LEN + VOE_HEADER_LEN {
            return Err(Error::Capacity(Item::SyncManager));
        }
        Ok(len)
    }

    /// Send one VoE payload and copy the reply's opaque payload into `reply`.
    ///
    /// Returns the reply's vendor message type and payload length. The reply
    /// vendor ID must match `vendor_id`.
    pub async fn exchange(
        &'maindevice self,
        vendor_id: u32,
        vendor_type: u16,
        payload: &[u8],
        reply: &mut [u8],
    ) -> Result<(u16, usize), Error> {
        let mailbox_len = self.mailbox_len(true)?.min(2048);
        let service_len = VOE_HEADER_LEN
            .checked_add(payload.len())
            .ok_or(Error::Capacity(Item::SyncManager))?;
        let frame_len = MAILBOX_HEADER_LEN
            .checked_add(service_len)
            .ok_or(Error::Capacity(Item::SyncManager))?;
        if frame_len > mailbox_len || service_len > u16::MAX as usize {
            return Err(Error::Capacity(Item::SyncManager));
        }

        let mut frame = [0; 2048];
        let header = MailboxHeader {
            length: service_len as u16,
            priority: Priority::Lowest,
            mailbox_type: MailboxType::VendorSpecific,
            counter: self.coe.subdevice.mailbox_counter(),
        };
        frame[..MAILBOX_HEADER_LEN].copy_from_slice(header.pack().as_ref());
        Packet { vendor_id, vendor_type, payload }
            .encode(&mut frame[MAILBOX_HEADER_LEN..frame_len])
            .ok_or(Error::Capacity(Item::SyncManager))?;

        let response = self.coe.mailbox_exchange_raw(&frame[..frame_len]).await?;
        let bytes: &[u8] = &response;
        if bytes.len() < MAILBOX_HEADER_LEN + VOE_HEADER_LEN
            || bytes[5] & 0x0f != MailboxType::VendorSpecific as u8
        {
            return Err(Error::Mailbox(MailboxError::InvalidCount));
        }
        let received_len = usize::from(u16::from_le_bytes([bytes[0], bytes[1]]));
        if received_len < VOE_HEADER_LEN
            || received_len > bytes.len() - MAILBOX_HEADER_LEN
            || received_len - VOE_HEADER_LEN > reply.len()
        {
            return Err(Error::Capacity(Item::SyncManager));
        }

        let packet = Packet::decode(&bytes[MAILBOX_HEADER_LEN..MAILBOX_HEADER_LEN + received_len])
            .ok_or(Error::Mailbox(MailboxError::InvalidCount))?;
        if packet.vendor_id != vendor_id {
            return Err(Error::Mailbox(MailboxError::InvalidCount));
        }
        reply[..packet.payload.len()].copy_from_slice(packet.payload);
        Ok((packet.vendor_type, packet.payload.len()))
    }
}
