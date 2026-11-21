//! File Access over EtherCAT packet encoding and transfers.

/// FoE request/response opcodes, ETG1000.4.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum Opcode {
    /// Read request.
    Read = 1,
    /// Write request.
    Write = 2,
    /// Data packet.
    Data = 3,
    /// Acknowledgement.
    Ack = 4,
    /// Transfer error.
    Error = 5,
    /// Busy response.
    Busy = 6,
}

/// Decoded FoE packet. `value` is a password, packet number, or error code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Packet<'a> {
    /// Packet operation.
    pub opcode: Opcode,
    /// Password, packet number, or error code.
    pub value: u32,
    /// Request filename or packet payload.
    pub data: &'a [u8],
}

/// Errors encountered when decoding FoE packets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeError {
    /// The FoE header is incomplete.
    TooShort,
    /// The opcode is not defined by FoE.
    UnknownOpcode,
}

impl<'a> Packet<'a> {
    /// Decode the six-byte FoE header and borrow its payload.
    pub fn decode(bytes: &'a [u8]) -> Result<Self, DecodeError> {
        if bytes.len() < 6 {
            return Err(DecodeError::TooShort);
        }
        let opcode = match u16::from_le_bytes([bytes[0], bytes[1]]) {
            1 => Opcode::Read,
            2 => Opcode::Write,
            3 => Opcode::Data,
            4 => Opcode::Ack,
            5 => Opcode::Error,
            6 => Opcode::Busy,
            _ => return Err(DecodeError::UnknownOpcode),
        };
        Ok(Self {
            opcode,
            value: u32::from_le_bytes(bytes[2..6].try_into().unwrap()),
            data: &bytes[6..],
        })
    }

    /// Encode a FoE packet into caller-provided storage.
    pub fn encode(self, output: &mut [u8]) -> Option<usize> {
        let end = 6usize.checked_add(self.data.len())?;
        let header = output.get_mut(..6)?;
        header[..2].copy_from_slice(&(self.opcode as u16).to_le_bytes());
        header[2..].copy_from_slice(&self.value.to_le_bytes());
        output.get_mut(6..end)?.copy_from_slice(self.data);
        Some(end)
    }
}

use crate::{
    error::{Error, Item, MailboxError},
    mailbox::{coe::Coe, MailboxHeader, MailboxType, Priority},
    subdevice::{SubDevice, SubDeviceRef},
};
use core::ops::Deref;
use ethercrab_wire::EtherCrabWireWriteSized;

const FOE_MAILBOX_OVERHEAD: usize = 12;

/// FoE client built on top of the SubDevice mailbox transport.
pub struct FoE<'maindevice, S> {
    coe: Coe<'maindevice, S>,
}

impl<'maindevice, S> FoE<'maindevice, S>
where
    S: Deref<Target = SubDevice>,
{
    /// Create a FoE client for a discovered SubDevice.
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
            .contains(crate::eeprom::types::MailboxProtocols::FOE)
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
        if len <= FOE_MAILBOX_OVERHEAD {
            return Err(Error::Capacity(Item::SyncManager));
        }
        Ok(len)
    }

    fn build_frame<'a>(
        &self,
        frame: &'a mut [u8],
        opcode: Opcode,
        value: u32,
        data: &[u8],
        mailbox_len: usize,
    ) -> Result<&'a [u8], Error> {
        let service_len = 6usize
            .checked_add(data.len())
            .ok_or(Error::Capacity(Item::SyncManager))?;
        let frame_len = 6usize
            .checked_add(service_len)
            .ok_or(Error::Capacity(Item::SyncManager))?;
        if frame_len > mailbox_len || service_len > u16::MAX as usize {
            return Err(Error::Capacity(Item::SyncManager));
        }
        let header = MailboxHeader {
            length: service_len as u16,
            priority: Priority::Lowest,
            mailbox_type: MailboxType::Foe,
            counter: self.coe.subdevice.mailbox_counter(),
        };
        frame[..6].copy_from_slice(header.pack().as_ref());
        Packet { opcode, value, data }
            .encode(&mut frame[6..frame_len])
            .ok_or(Error::Capacity(Item::SyncManager))?;
        Ok(&frame[..frame_len])
    }

    fn store_reply(bytes: &[u8], reply: &mut [u8]) -> Result<usize, Error> {
        if bytes.len() < FOE_MAILBOX_OVERHEAD || bytes[5] & 0x0f != MailboxType::Foe as u8 {
            return Err(Error::Foe(0));
        }
        let received_len = usize::from(u16::from_le_bytes([bytes[0], bytes[1]]));
        if received_len < 6 || received_len > bytes.len() - 6 || received_len > reply.len() {
            return Err(Error::Capacity(Item::SyncManager));
        }
        reply[..received_len].copy_from_slice(&bytes[6..6 + received_len]);
        Ok(received_len)
    }

    fn decode(reply: &[u8]) -> Result<Packet<'_>, Error> {
        let packet = Packet::decode(reply).map_err(|_| Error::Foe(0))?;
        if packet.opcode == Opcode::Error {
            return Err(Error::Foe(packet.value));
        }
        Ok(packet)
    }

    /// Download a FoE file using acknowledged packets.
    pub async fn write_file(
        &'maindevice self,
        filename: &str,
        password: u32,
        data: &[u8],
    ) -> Result<(), Error> {
        if filename.as_bytes().contains(&0) {
            return Err(Error::Foe(0));
        }
        let mailbox_len = self.mailbox_len(true)?.min(2048);
        let packet_capacity = mailbox_len - FOE_MAILBOX_OVERHEAD;
        let mut frame = [0; 2048];
        let mut reply = [0; 2048];

        let request = self.build_frame(&mut frame, Opcode::Write, password, filename.as_bytes(), mailbox_len)?;
        let response = self.coe.mailbox_exchange_raw(request).await?;
        let len = Self::store_reply(&response, &mut reply)?;
        {
            let ack = Self::decode(&reply[..len])?;
            if ack.opcode != Opcode::Ack || ack.value != 0 {
                return Err(Error::Foe(0));
            }
        }

        let mut offset = 0;
        let mut packet_number = 1u32;
        loop {
            let end = (offset + packet_capacity).min(data.len());
            let chunk_len = end - offset;
            let request = self.build_frame(
                &mut frame,
                Opcode::Data,
                packet_number,
                &data[offset..end],
                mailbox_len,
            )?;
            let response = self.coe.mailbox_exchange_raw(request).await?;
            let len = Self::store_reply(&response, &mut reply)?;
            {
                let ack = Self::decode(&reply[..len])?;
                if ack.opcode != Opcode::Ack || ack.value != packet_number {
                    return Err(Error::Foe(0));
                }
            }
            offset = end;
            if chunk_len < packet_capacity {
                return Ok(());
            }
            packet_number = packet_number.wrapping_add(1);
        }
    }

    /// Upload a FoE file into caller-provided storage.
    pub async fn read_file_into(
        &'maindevice self,
        filename: &str,
        password: u32,
        output: &mut [u8],
    ) -> Result<usize, Error> {
        if filename.as_bytes().contains(&0) {
            return Err(Error::Foe(0));
        }
        let mailbox_len = self.mailbox_len(true)?.min(2048);
        let packet_capacity = mailbox_len - FOE_MAILBOX_OVERHEAD;
        let mut frame = [0; 2048];
        let mut reply = [0; 2048];

        let request = self.build_frame(&mut frame, Opcode::Read, password, filename.as_bytes(), mailbox_len)?;
        let response = self.coe.mailbox_exchange_raw(request).await?;
        let mut reply_len = Self::store_reply(&response, &mut reply)?;

        let mut written = 0usize;
        let mut expected = 1u32;
        loop {
            let packet_len = {
                let packet = Self::decode(&reply[..reply_len])?;
                if packet.opcode != Opcode::Data || packet.value != expected {
                    return Err(Error::Foe(0));
                }
                let end = written
                    .checked_add(packet.data.len())
                    .filter(|end| *end <= output.len())
                    .ok_or(Error::Capacity(Item::SyncManager))?;
                output[written..end].copy_from_slice(packet.data);
                written = end;
                packet.data.len()
            };

            let ack = self.build_frame(&mut frame, Opcode::Ack, expected, &[], mailbox_len)?;
            self.coe.mailbox_send_raw(ack).await?;
            if packet_len < packet_capacity {
                return Ok(written);
            }

            expected = expected.wrapping_add(1);
            let next = self.coe.mailbox_receive_raw().await?;
            reply_len = Self::store_reply(&next, &mut reply)?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_roundtrip_and_truncation() {
        let packet = Packet { opcode: Opcode::Data, value: 0x1020_3040, data: &[1, 2] };
        let mut bytes = [0; 8];
        assert_eq!(packet.encode(&mut bytes), Some(8));
        assert_eq!(Packet::decode(&bytes), Ok(packet));
        assert_eq!(Packet::decode(&bytes[..5]), Err(DecodeError::TooShort));
        bytes[..2].copy_from_slice(&0u16.to_le_bytes());
        assert_eq!(Packet::decode(&bytes), Err(DecodeError::UnknownOpcode));
    }

    #[test]
    fn rrq_and_wrq_match_soem_and_preserve_foe_error_codes() {
        let expected_rrq = [1, 0, 0, 0, 0, 0, b'd', b'e', b'm', b'o', b'.', b'b', b'i', b'n'];
        let expected_wrq = [2, 0, 0, 0, 0, 0, b'd', b'e', b'm', b'o', b'.', b'b', b'i', b'n'];
        for (opcode, expected) in [(Opcode::Read, expected_rrq), (Opcode::Write, expected_wrq)] {
            let mut bytes = [0; 14];
            assert_eq!(Packet { opcode, value: 0, data: b"demo.bin" }.encode(&mut bytes), Some(14));
            assert_eq!(bytes, expected);
        }
        let mut data = [0; 8];
        Packet { opcode: Opcode::Data, value: 1, data: &[0xaa, 0xbb] }.encode(&mut data);
        assert_eq!(data, [3, 0, 1, 0, 0, 0, 0xaa, 0xbb]);
        let mut ack = [0; 6];
        Packet { opcode: Opcode::Ack, value: 1, data: &[] }.encode(&mut ack);
        assert_eq!(ack, [4, 0, 1, 0, 0, 0]);
        let mut error = [0; 6];
        Packet { opcode: Opcode::Error, value: 0x8000, data: &[] }.encode(&mut error);
        assert_eq!(Packet::decode(&error).unwrap().value, 0x8000);
    }
}
