pub mod error;
pub mod headers;

use crate::idn_to_str;
use crate::mailbox::soe::error::SoeErrorCode;
use crate::mailbox::soe::headers::SoeAttributes;
use crate::{
    SubDevice, SubDeviceRef,
    error::{
        Error::{self, SoeError},
        MailboxError, PduError,
    },
    fmt,
    mailbox::{
        MailboxHeader, MailboxType, Priority,
        soe::headers::{SoeElementFlag, SoeErrorFlag, SoeFragmentationFlag},
    },
    pdu_loop::ReceivedPdu,
};
use core::ops::Deref;
use core::{any::type_name, fmt::Debug};
use ethercrab_wire::{EtherCrabWireRead, EtherCrabWireSized, EtherCrabWireWrite};

pub(crate) use headers::{SoeHeader, SoeOpcode};

pub struct Soe<'maindevice, S> {
    subdevice: &'maindevice SubDeviceRef<'maindevice, S>,
}

impl<'maindevice, S> Soe<'maindevice, S>
where
    S: Deref<Target = SubDevice>,
{
    pub fn new(subdevice: &'maindevice SubDeviceRef<'maindevice, S>) -> Self {
        Self { subdevice }
    }

    /// Send a mailbox request, wait for response mailbox to be ready, read response from mailbox
    /// and return as a slice with payload length
    async fn mailbox_write_read<R>(
        &'maindevice self,
        opcode: SoeOpcode,
        drive_num: u8,
        element: SoeElementFlag,
        idn_address: u16,
        data: R,
    ) -> Result<(usize, ReceivedPdu<'maindevice>), Error>
    where
        R: EtherCrabWireWrite + Debug,
    {
        let (read_mailbox, write_mailbox) =
            self.subdevice
                .wait_for_mailboxes()
                .await
                .inspect_err(|err| {
                    fmt::error!(
                        "{} {} {}",
                        self.subdevice.configured_address(),
                        self.subdevice.name(),
                        err
                    )
                })?;

        let mailbox_header_len = MailboxHeader::PACKED_LEN;
        let total_header_len = SoeHeader::PACKED_LEN + mailbox_header_len;

        // Maximum data payload per mailbox message
        let max_data_len = (write_mailbox.len as usize) - total_header_len;
        let mut request = vec![0u8; write_mailbox.len as usize];

        // Allocate buffer for data and pack
        let data_len = data.packed_len();
        let mut data_bytes = vec![0u8; data_len];
        data.pack_to_slice(&mut data_bytes)?;

        let mut data_sent: usize = 0;

        while data_len - data_sent > max_data_len {
            // Send maximum data length packets until what we have can fit in a single packet
            let counter = self.subdevice.mailbox_counter();
            let mailbox_header = MailboxHeader {
                // SoE header (always 4 bytes) + payload
                length: (SoeHeader::PACKED_LEN + max_data_len) as u16,
                // address: 0x0000,
                priority: Priority::Lowest,
                mailbox_type: MailboxType::Soe,
                counter,
            };

            let soe_header = SoeHeader {
                opcode,
                fragmentation: SoeFragmentationFlag::IncompleteFrame,
                error: SoeErrorFlag::NoError,
                drive_num,
                element_flag: element,
                idn: ((data_len - data_sent) / max_data_len).try_into()?,
            };

            mailbox_header.pack_to_slice(&mut request[..mailbox_header_len])?;
            soe_header.pack_to_slice(&mut request[mailbox_header_len..total_header_len])?;
            request[total_header_len..]
                .copy_from_slice(&data_bytes[data_sent..(data_sent + max_data_len)]);

            // Send data to SubDevice IN mailbox
            self.subdevice
                .write(write_mailbox.address)
                .with_len(write_mailbox.len)
                .send(self.subdevice.maindevice, &request[..])
                .await?;

            data_sent += max_data_len;

            // Wait for mailboxes to become available again before proceeding
            let (_, _) = self
                .subdevice
                .wait_for_mailboxes()
                .await
                .inspect_err(|err| {
                    fmt::error!(
                        "{} {} {}",
                        self.subdevice.configured_address(),
                        self.subdevice.name(),
                        err
                    )
                })?;
        }

        let counter = self.subdevice.mailbox_counter();
        let mailbox_header = MailboxHeader {
            // SoE header (always 4 bytes) + payload
            length: (SoeHeader::PACKED_LEN + (data_len - data_sent)) as u16,
            // address: 0x0000,
            priority: Priority::Lowest,
            mailbox_type: MailboxType::Soe,
            counter,
        };

        let soe_header = SoeHeader {
            opcode,
            fragmentation: SoeFragmentationFlag::CompleteTransmission,
            error: SoeErrorFlag::NoError,
            drive_num,
            element_flag: element,
            idn: idn_address,
        };

        mailbox_header.pack_to_slice(&mut request[..mailbox_header_len])?;
        soe_header.pack_to_slice(&mut request[mailbox_header_len..total_header_len])?;
        if data.packed_len() > 0 {
            request[total_header_len..(total_header_len + (data_len - data_sent))]
                .copy_from_slice(&data_bytes[data_sent..]);
        }

        // Send data to SubDevice IN mailbox
        self.subdevice
            .write(write_mailbox.address)
            .with_len(write_mailbox.len)
            .send(
                self.subdevice.maindevice,
                &request[..(data_len - data_sent + total_header_len)],
            )
            .await?;

        let mut response = self
            .subdevice
            .wait_for_mailbox_response(&read_mailbox)
            .await?;

        let mailbox_header = MailboxHeader::unpack_from_slice(&response)?;
        response.trim_front(MailboxHeader::PACKED_LEN);
        let soe_header = SoeHeader::unpack_from_slice(&response)?;
        response.trim_front(SoeHeader::PACKED_LEN);

        // Check the response header error bit
        match soe_header.error {
            SoeErrorFlag::NoError => {}
            SoeErrorFlag::ErrorOccurred => {
                let payload: &[u8] = &response;
                let error_code = u16::from_le_bytes([payload[0], payload[1]]);

                return Err(SoeError(SoeErrorCode::from(error_code)));
            }
        }

        // Validate that the mailbox response is to the request we just sent
        if mailbox_header.mailbox_type != MailboxType::Soe || soe_header.idn != idn_address {
            fmt::error!(
                "Invalid IDN response. Type: {:?} (expected {:?}), {} (expected {})",
                mailbox_header.mailbox_type,
                MailboxType::Soe,
                idn_to_str(soe_header.idn),
                idn_to_str(idn_address),
            );

            return Err(Error::Mailbox(MailboxError::IdnResponseInvalid {
                idn_address,
            }));
        }

        Ok((
            mailbox_header.length as usize - SoeHeader::PACKED_LEN,
            response,
        ))
    }

    pub async fn idn_read_element<T>(
        &self,
        drive_num: u8,
        idn_address: u16,
        element: SoeElementFlag,
    ) -> Result<T, Error>
    where
        T: EtherCrabWireRead,
    {
        let (l, response) = self
            .mailbox_write_read(SoeOpcode::ReadRequest, drive_num, element, idn_address, ())
            .await?;

        let data: &[u8] = &response[..l];

        T::unpack_from_slice(data).map_err(|_| {
            fmt::error!(
                "IDN read decode T: {}, data {:?} (len {})",
                type_name::<T>(),
                data,
                data.len(),
            );

            Error::Pdu(PduError::Decode)
        })
    }

    pub async fn idn_read_status(&self, drive_num: u8, idn_address: u16) -> Result<u16, Error> {
        self.idn_read_element::<u16>(drive_num, idn_address, SoeElementFlag::DataStateStatus)
            .await
    }

    pub async fn idn_read_name(&self, drive_num: u8, idn_address: u16) -> Result<String, Error> {
        let (l, response) = self
            .mailbox_write_read(
                SoeOpcode::ReadRequest,
                drive_num,
                SoeElementFlag::NameDescriptor,
                idn_address,
                (),
            )
            .await?;

        let data: &[u8] = &response[..l];

        Ok(str::from_utf8(data)
            .map_err(|_| {
                fmt::error!(
                    "IDN read decode T: {} (len {}) data {:?} (len {})",
                    type_name::<String>(),
                    data.len(),
                    data,
                    data.len(),
                );

                Error::Pdu(PduError::Decode)
            })?
            .to_owned())
    }

    pub async fn idn_read_attribute(
        &self,
        drive_num: u8,
        idn_address: u16,
    ) -> Result<SoeAttributes, Error> {
        self.idn_read_element::<SoeAttributes>(drive_num, idn_address, SoeElementFlag::Attribute)
            .await
    }

    pub async fn idn_read_units(&self, drive_num: u8, idn_address: u16) -> Result<String, Error> {
        let (l, response) = self
            .mailbox_write_read(
                SoeOpcode::ReadRequest,
                drive_num,
                SoeElementFlag::Unit,
                idn_address,
                (),
            )
            .await?;

        let data: &[u8] = &response[..l];

        String::unpack_from_slice(data).map_err(|_| {
            fmt::error!(
                "IDN read decode T: {} (len {}) data {:?} (len {})",
                type_name::<String>(),
                data.len(),
                data,
                data.len(),
            );

            Error::Pdu(PduError::Decode)
        })
    }

    pub async fn idn_read_min<T>(&self, drive_num: u8, idn_address: u16) -> Result<T, Error>
    where
        T: EtherCrabWireRead,
    {
        self.idn_read_element::<T>(drive_num, idn_address, SoeElementFlag::MinimumValue)
            .await
    }

    pub async fn idn_read_max<T>(&self, drive_num: u8, idn_address: u16) -> Result<T, Error>
    where
        T: EtherCrabWireRead,
    {
        self.idn_read_element::<T>(drive_num, idn_address, SoeElementFlag::MaximumValue)
            .await
    }

    pub async fn idn_read_data<T>(&self, drive_num: u8, idn_address: u16) -> Result<T, Error>
    where
        T: EtherCrabWireRead,
    {
        self.idn_read_element::<T>(drive_num, idn_address, SoeElementFlag::ValueData)
            .await
    }

    pub async fn idn_read_data_list(
        &self,
        drive_num: u8,
        idn_address: u16,
    ) -> Result<(u16, Vec<u16>), Error> {
        let (l, response) = self
            .mailbox_write_read(
                SoeOpcode::ReadRequest,
                drive_num,
                SoeElementFlag::ValueData,
                idn_address,
                (),
            )
            .await?;

        let data: &[u8] = &response[..l];

        let data_words: Vec<u16> = data
            .chunks_exact(2)
            .map(|chunk| {
                let array: [u8; 2] = chunk.try_into().unwrap();
                u16::from_le_bytes(array)
            })
            .collect();

        let _actual_length = data_words[0];
        let max_length = data_words[1];

        Ok((max_length, data_words[2..].to_vec()))
    }

    pub async fn idn_write_data<T>(
        &self,
        drive_num: u8,
        idn_address: u16,
        value: T,
    ) -> Result<(), Error>
    where
        T: EtherCrabWireWrite + Debug,
    {
        let (_, _) = self
            .mailbox_write_read(
                SoeOpcode::WriteRequest,
                drive_num,
                SoeElementFlag::ValueData,
                idn_address,
                value,
            )
            .await?;

        Ok(())
    }
}
