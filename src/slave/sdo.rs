use super::SlaveRef;
use crate::{
    coe::{self, abort_code::AbortCode, services::CoeServiceTrait, SdoAccess},
    command::Command,
    error::{Error, PduError},
    mailbox::MailboxType,
    pdu_loop::CheckWorkingCounter,
    register::RegisterAddress,
    sync_manager_channel::SyncManagerChannel,
    timer_factory::TimerFactory,
    PduData, PduRead,
};
use core::time::Duration;
use core::{any::type_name, fmt::Debug};
use nom::{bytes::complete::take, number::complete::le_u32};
use num_enum::TryFromPrimitive;
use packed_struct::{PackedStruct, PackedStructSlice};

impl<'a, const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, TIMEOUT>
    SlaveRef<'a, MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>
where
    TIMEOUT: TimerFactory,
{
    pub async fn write_sdo<T>(&self, index: u16, value: T, access: SdoAccess) -> Result<(), Error>
    where
        T: PduData,
        <T as PduRead>::Error: Debug,
    {
        let counter = self.client.mailbox_counter();

        if T::len() > 4 {
            // TODO: Normal SDO download. Only expedited requests for now
            panic!("Data too long");
        }

        let mut data = [0u8; 4];

        let len = usize::from(T::len());

        data[0..len].copy_from_slice(value.as_slice());

        let request = coe::services::download(counter, index, access, data, len as u8);

        let (_response, _data) = self.send_coe_service(request).await?;

        // TODO: Validate reply?

        Ok(())
    }

    async fn send_coe_service<H>(&self, request: H) -> Result<(H, &[u8]), Error>
    where
        H: CoeServiceTrait + packed_struct::PackedStructInfo,
        <H as PackedStruct>::ByteArray: AsRef<[u8]>,
    {
        let write_mailbox = self.config.mailbox.write.ok_or(Error::NoMailbox)?;
        let read_mailbox = self.config.mailbox.read.ok_or(Error::NoMailbox)?;

        let counter = request.counter();

        self.client
            .pdu_loop
            .pdu_tx_readwrite_len(
                Command::Fpwr {
                    address: self.configured_address,
                    register: write_mailbox.address,
                },
                request.pack().unwrap().as_ref(),
                write_mailbox.len,
            )
            .await?
            .wkc(1, "SDO upload request")?;

        // Wait for slave send mailbox to be ready
        crate::timeout::<TIMEOUT, _, _>(Duration::from_millis(1000), async {
            let mailbox_read_sm = RegisterAddress::sync_manager(read_mailbox.sync_manager);

            loop {
                let sm = self
                    .read::<SyncManagerChannel>(mailbox_read_sm, "Master read mailbox")
                    .await?;

                if sm.status.mailbox_full {
                    break Result::<(), _>::Ok(());
                }

                TIMEOUT::timer(Duration::from_millis(10)).await;
            }
        })
        .await
        .map_err(|e| {
            log::error!("Mailbox read ready timeout");

            e
        })?;

        // Receive data from slave send mailbox
        let response = self
            .client
            .pdu_loop
            .pdu_tx_readonly(
                Command::Fprd {
                    address: self.configured_address,
                    register: read_mailbox.address,
                },
                read_mailbox.len,
            )
            .await?
            .wkc(1, "SDO read mailbox")?;

        // TODO: Retries. Refer to SOEM's `ecx_mbxreceive` for inspiration

        let headers_len = H::packed_bits() / 8;

        let (headers, data) = response.split_at(headers_len);

        // TODO: Ability to use segmented headers
        let headers = H::unpack_from_slice(headers).map_err(|e| {
            log::error!("Failed to unpack mailbox response headers: {e}");

            e
        })?;

        if headers.is_aborted() {
            let code = data[0..4]
                .try_into()
                .map_err(|_| ())
                .and_then(|arr| {
                    AbortCode::try_from_primitive(u32::from_le_bytes(arr)).map_err(|_| ())
                })
                .unwrap_or(AbortCode::General);

            Err(Error::SdoAborted(code))
        } else if
        // Validate that the mailbox response is to the request we just sent
        headers.mailbox_type() != MailboxType::Coe || headers.counter() != counter {
            Err(Error::SdoResponseInvalid)
        } else {
            Ok((headers, data))
        }
    }

    pub async fn read_sdo<T>(&self, index: u16, access: SdoAccess) -> Result<T, Error>
    where
        T: PduData,
        <T as PduRead>::Error: Debug,
    {
        let mut buf = [0u8; MAX_PDU_DATA];

        self.read_sdo_buf(index, access, &mut buf)
            .await
            .and_then(|data| {
                T::try_from_slice(data).map_err(|_| {
                    log::error!(
                        "SDO expedited data decode T: {} (len {}) data {:?} (len {})",
                        type_name::<T>(),
                        T::len(),
                        data,
                        data.len()
                    );

                    Error::Pdu(PduError::Decode)
                })
            })
    }

    pub async fn read_sdo_buf<'buf>(
        &self,
        index: u16,
        access: SdoAccess,
        buf: &'buf mut [u8],
    ) -> Result<&'buf [u8], Error> {
        let request = coe::services::upload(self.client.mailbox_counter(), index, access);

        let (headers, data) = self.send_coe_service(request).await?;

        // Expedited transfers where the data is 4 bytes or less long, denoted in the SDO header
        // size value.
        if headers.sdo_header.flags.expedited_transfer {
            let data_len = 4usize.saturating_sub(usize::from(headers.sdo_header.flags.size));
            let data = &data[0..data_len];

            let buf = &mut buf[0..data_len];

            buf.copy_from_slice(data);

            Ok(buf)
        }
        // Data is either a normal upload or a segmented upload
        else {
            let data_length = headers.header.length.saturating_sub(0x0a);

            let (data, complete_size) = le_u32(data)?;

            // The provided buffer isn't long enough to contain all mailbox data.
            if complete_size > buf.len() as u32 {
                return Err(Error::MailboxTooLong);
            }

            // If it's a normal upload, the response payload is returned in the initial mailbox read
            if complete_size <= u32::from(data_length) {
                let (_rest, data) = take(data_length)(data)?;

                buf.copy_from_slice(data);

                Ok(&buf[0..usize::from(data_length)])
            }
            // If it's a segmented upload, we must make subsequent requests to load all segment data
            // from the read mailbox.
            else {
                let mut toggle = false;
                let mut total_len = 0usize;

                loop {
                    let request =
                        coe::services::upload_segmented(self.client.mailbox_counter(), toggle);

                    let (headers, data) = self.send_coe_service(request).await?;

                    // The spec defines the data length as n-3, so we'll just go with that magic
                    // number...
                    let mut chunk_len = usize::from(headers.header.length - 3);

                    // Special case as per spec: Minimum response size is 7 bytes. For smaller
                    // responses, we must remove the number of unused bytes at the end of the
                    // response. Extremely weird.
                    if chunk_len == 7 {
                        chunk_len -= usize::from(headers.sdo_header.segment_data_size);
                    }

                    let data = &data[0..chunk_len];

                    buf[total_len..][..chunk_len].copy_from_slice(data);
                    total_len += chunk_len;

                    if headers.sdo_header.is_last_segment {
                        break;
                    }

                    toggle = !toggle;
                }

                Ok(&buf[0..total_len])
            }
        }
    }
}
