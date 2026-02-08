mod abort_code;
mod headers;
pub mod services;

use crate::{
    ObjectDescriptionListQuery, ObjectDescriptionListQueryCounts, SubDevice, SubDeviceRef,
    error::{Error, Item, MailboxError, PduError},
    fmt,
    mailbox::{
        MailboxHeader, MailboxType,
        coe::{
            self,
            services::{
                CoeServiceRequest, ObjectDescriptionListRequest, ObjectDescriptionListResponse,
                SdoExpedited, SdoNormal, SdoSegmented,
            },
        },
    },
    pdu_loop::ReceivedPdu,
    register::RegisterAddress,
    subdevice::Mailbox,
    timer_factory::IntoTimeout,
};
use core::ops::Deref;
use core::{any::type_name, fmt::Debug};
use ethercrab_wire::{
    EtherCrabWireRead, EtherCrabWireReadSized, EtherCrabWireSized, EtherCrabWireWrite,
    EtherCrabWireWriteSized,
};

pub(crate) use headers::{CoeCommand, CoeHeader, CoeService, SdoExpeditedPayload, SdoInfoOpCode};

pub use abort_code::CoeAbortCode;
pub use headers::SubIndex;

pub struct Coe<'maindevice, S> {
    subdevice: &'maindevice SubDeviceRef<'maindevice, S>,
}

impl<'maindevice, S> Coe<'maindevice, S>
where
    S: Deref<Target = SubDevice>,
{
    pub fn new(subdevice: &'maindevice SubDeviceRef<'maindevice, S>) -> Self {
        Self { subdevice }
    }

    /// Get CoE read/write mailboxes, waiting for them to be ready to read/write.
    async fn wait_for_mailboxes(&self) -> Result<(Mailbox, Mailbox), Error> {
        let write_mailbox = self
            .subdevice
            .config
            .mailbox
            .write
            .ok_or(Error::Mailbox(MailboxError::NoReadMailbox))?;
        let read_mailbox = self
            .subdevice
            .config
            .mailbox
            .read
            .ok_or(Error::Mailbox(MailboxError::NoWriteMailbox))?;

        let mailbox_read_sm_status =
            RegisterAddress::sync_manager_status(read_mailbox.sync_manager);
        let mailbox_write_sm_status =
            RegisterAddress::sync_manager_status(write_mailbox.sync_manager);

        // Ensure SubDevice OUT (master IN) mailbox is empty. We'll retry this multiple times in
        // case the SubDevice is still busy or bugged or something.
        for i in 0..10 {
            let sm_status = self
                .subdevice
                .read(mailbox_read_sm_status)
                .receive::<crate::sync_manager_channel::Status>(self.subdevice.maindevice)
                .await?;

            // If flag is set, read entire mailbox to clear it
            if sm_status.mailbox_full {
                fmt::debug!(
                    "SubDevice {:#06x} OUT mailbox not empty (status {:?}). Clearing.",
                    self.subdevice.configured_address(),
                    sm_status
                );

                self.subdevice
                    .read(read_mailbox.address)
                    .ignore_wkc()
                    .receive_slice(self.subdevice.maindevice, read_mailbox.len)
                    .await?;
            } else {
                break;
            }

            // Don't delay on first iteration
            if i > 0 {
                self.subdevice.maindevice.timeouts.loop_tick().await;
            }

            if i > 1 {
                fmt::debug!("--> Retrying clear");
            }
        }

        // Wait for SubDevice IN mailbox to be available to receive data from master
        async {
            loop {
                let sm_status = self
                    .subdevice
                    .read(mailbox_write_sm_status)
                    .receive::<crate::sync_manager_channel::Status>(self.subdevice.maindevice)
                    .await?;

                if !sm_status.mailbox_full {
                    break Ok(());
                }

                self.subdevice.maindevice.timeouts.loop_tick().await;
            }
        }
        .timeout(self.subdevice.maindevice.timeouts.mailbox_echo())
        .await
        .inspect_err(|&e| {
            fmt::error!(
                "Mailbox IN ready error for SubDevice {:#06x}: {}",
                self.subdevice.configured_address(),
                e
            );
        })?;

        Ok((read_mailbox, write_mailbox))
    }

    /// Wait for a mailbox response
    async fn wait_for_mailbox_response(
        &self,
        read_mailbox: &Mailbox,
    ) -> Result<ReceivedPdu, Error> {
        let mailbox_read_sm = RegisterAddress::sync_manager_status(read_mailbox.sync_manager);

        // Wait for SubDevice OUT mailbox to be ready
        async {
            loop {
                let sm_status = self
                    .subdevice
                    .read(mailbox_read_sm)
                    .receive::<crate::sync_manager_channel::Status>(self.subdevice.maindevice)
                    .await?;

                if sm_status.mailbox_full {
                    break Ok(());
                }

                self.subdevice.maindevice.timeouts.loop_tick().await;
            }
        }
        .timeout(self.subdevice.maindevice.timeouts.mailbox_response())
        .await
        .inspect_err(|&e| {
            fmt::error!(
                "Response mailbox IN error for SubDevice {:#06x}: {}",
                self.subdevice.configured_address(),
                e
            );
        })?;

        // Read acknowledgement from SubDevice OUT mailbox
        let response = self
            .subdevice
            .read(read_mailbox.address)
            .receive_slice(self.subdevice.maindevice, read_mailbox.len)
            .await?;

        // TODO: Retries. Refer to SOEM's `ecx_mbxreceive` for inspiration

        Ok(response)
    }

    /// Send a mailbox request, wait for response mailbox to be ready, read response from mailbox
    /// and return as a slice.
    async fn mailbox_write_read<R>(
        &'maindevice self,
        request: R,
    ) -> Result<(R, ReceivedPdu<'maindevice>), Error>
    where
        R: CoeServiceRequest + Debug,
    {
        let (read_mailbox, write_mailbox) = self.wait_for_mailboxes().await.inspect_err(|err| {
            fmt::error!(
                "{} {} {}",
                self.subdevice.configured_address(),
                self.subdevice.name(),
                err
            )
        })?;

        // Send data to SubDevice IN mailbox
        self.subdevice
            .write(write_mailbox.address)
            .with_len(write_mailbox.len)
            .send(self.subdevice.maindevice, &request.pack().as_ref())
            .await?;

        let mut response = self.wait_for_mailbox_response(&read_mailbox).await?;

        /// A super generalised version of the various header shapes for responses, extracting only
        /// what we need in this method.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, ethercrab_wire::EtherCrabWireRead)]
        #[wire(bytes = 12)]
        struct HeadersRaw {
            #[wire(bytes = 6)]
            header: MailboxHeader,

            #[wire(bytes = 2)]
            coe_header: CoeHeader,

            #[wire(pre_skip = 5, bits = 3)]
            command: CoeCommand,

            // 9 bytes up to here

            // SAFETY: These fields will be garbage (but not invalid) if the response is NOT an
            // abort transfer request. Use with caution!
            #[wire(bytes = 2)]
            address: u16,
            #[wire(bytes = 1)]
            sub_index: u8,
        }

        let headers = HeadersRaw::unpack_from_slice(&response)?;

        assert_ne!(headers.coe_header.service, CoeService::Emergency);

        if headers.coe_header.service == CoeService::Emergency {
            #[derive(Debug, Copy, Clone, ethercrab_wire::EtherCrabWireRead)]
            #[wire(bytes = 8)]
            struct EmergencyData {
                #[wire(bytes = 2)]
                error_code: u16,
                #[wire(bytes = 1)]
                error_register: u8,
                #[wire(bytes = 5)]
                extra_data: [u8; 5],
            }

            response.trim_front(HeadersRaw::PACKED_LEN);

            let decoded = EmergencyData::unpack_from_slice(&response)?;

            #[cfg(not(feature = "defmt"))]
            fmt::error!(
                "Mailbox emergency code {:#06x}, register {:#04x}, data {:#04x?}",
                decoded.error_code,
                decoded.error_register,
                decoded.extra_data
            );
            #[cfg(feature = "defmt")]
            fmt::error!(
                "Mailbox emergency code {:#06x}, register {:#04x}, data {=[u8]}",
                decoded.error_code,
                decoded.error_register,
                decoded.extra_data
            );

            Err(Error::Mailbox(MailboxError::Emergency {
                error_code: decoded.error_code,
                error_register: decoded.error_register,
            }))
        } else if headers.command == CoeCommand::Abort {
            // ETG 1000.6 §5.6.2.7.1 Table 40
            response.trim_front(HeadersRaw::PACKED_LEN);
            let code = CoeAbortCode::unpack_from_slice(&response)?;

            fmt::error!(
                "Mailbox error for SubDevice {:#06x} (supports complete access: {}): {}",
                self.subdevice.configured_address(),
                self.subdevice.config.mailbox.complete_access,
                code
            );

            Err(Error::Mailbox(MailboxError::Aborted {
                code,
                address: headers.address,
                sub_index: headers.sub_index,
            }))
        }
        // Validate that the mailbox response is to the request we just sent
        else if headers.header.mailbox_type != MailboxType::Coe
            || !request.validate_response(headers.address, headers.sub_index)
        {
            fmt::error!(
                "Invalid SDO response. Type: {:?} (expected {:?}), index {}, subindex {}",
                headers.header.mailbox_type,
                MailboxType::Coe,
                headers.address,
                headers.sub_index,
            );

            Err(Error::Mailbox(MailboxError::SdoResponseInvalid {
                address: headers.address,
                sub_index: headers.sub_index,
            }))
        } else {
            let headers = R::unpack_from_slice(&response)?;

            response.trim_front(HeadersRaw::PACKED_LEN);

            Ok((headers, response))
        }
    }

    /// Handle submitting to mailboxes for the SDO Information service.
    ///
    /// If the subdevice doesn't have the necessary mailboxes, this will return `Ok(None)`.
    ///
    /// Per ETG.1000.5 §6.1.4.1.3.4, this means sending one request and then awaiting many
    /// responses.
    // TODO: make this generic w.r.t. SDO Info header type.
    async fn send_sdo_info_service(
        &self,
        request: coe::services::ObjectDescriptionListRequest,
    ) -> Result<Option<heapless::Vec<u8, { u16::MAX as usize * 2 }>>, Error> {
        let (read_mailbox, write_mailbox) = match self.wait_for_mailboxes().await {
            Ok((read, write)) => Ok((read, write)),
            Err(Error::Mailbox(MailboxError::NoReadMailbox | MailboxError::NoWriteMailbox)) => {
                return Ok(None);
            }
            Err(err) => Err(err),
        }?;

        // Send data to SubDevice IN mailbox
        self.subdevice
            .write(write_mailbox.address)
            .with_len(write_mailbox.len)
            .send(self.subdevice.maindevice, &request.pack().as_ref())
            .await?;

        const COE_HEADER_AND_LIST_TYPE_SIZE: usize = 8;

        let mut consumed_list_type = false;
        // The biggest SDO Info request is listing all the available objects,
        // which is u16::MAX * 2 = 0x1fffe bytes big (ETG.1000.6 §5.6.3.3,
        // CiA 301 §7.4.1).
        let mut buf = heapless::Vec::<u8, 0x1fffe>::new();
        loop {
            let mut response = self.wait_for_mailbox_response(&read_mailbox).await?;
            let headers = ObjectDescriptionListResponse::unpack_from_slice(&response)?;
            if headers.sdo_info_header.op_code == SdoInfoOpCode::GetObjectDescriptionListResponse {
                let length = headers.mailbox.length as usize - COE_HEADER_AND_LIST_TYPE_SIZE;
                fmt::trace!(
                    "CoE Info, {} fragments left",
                    headers.sdo_info_header.fragments_left
                );
                response.trim_front(ObjectDescriptionListResponse::PACKED_LEN);
                if !consumed_list_type {
                    response.trim_front(2); // skip over the list type
                    consumed_list_type = true;
                }
                buf.extend_from_slice(&response[..length])
                    .map_err(|_| Error::Internal)?;
                if !headers.sdo_info_header.incomplete {
                    break;
                }
            }
        }
        Ok(Some(buf))
    }

    /// Write a value to the given SDO index (address) and sub-index.
    ///
    /// Note that this method currently only supports expedited SDO downloads (4 bytes maximum).
    pub async fn sdo_write<T>(
        &self,
        index: u16,
        sub_index: impl Into<SubIndex>,
        value: T,
    ) -> Result<(), Error>
    where
        T: EtherCrabWireWrite,
    {
        let sub_index = sub_index.into();

        let counter = self.subdevice.mailbox_counter();

        if value.packed_len() > 4 {
            fmt::error!("Only 4 byte SDO writes or smaller are supported currently.");

            // TODO: Normal SDO download. Only expedited requests for now
            return Err(Error::Internal);
        }

        let mut buf = [0u8; 4];

        value.pack_to_slice(&mut buf)?;

        let request =
            SdoExpedited::download(counter, index, sub_index, buf, value.packed_len() as u8);

        fmt::trace!("CoE download");

        let (_response, _data) = self.mailbox_write_read(request).await?;

        // TODO: Validate reply?

        Ok(())
    }

    /// Write multiple sub-indices of the given SDO.
    ///
    /// This is NOT a complete access write. This method is provided as sugar over individual calls
    /// to [`sdo_write`](SubDeviceRef::sdo_write) and handles setting the SDO length and sub-index
    /// automatically.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use ethercrab::{
    /// #     error::Error, MainDevice, MainDeviceConfig, PduStorage, Timeouts, std::ethercat_now
    /// # };
    /// # static PDU_STORAGE: PduStorage<8, 32> = PduStorage::new();
    /// # let (_tx, _rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");
    /// # let maindevice = MainDevice::new(pdu_loop, Timeouts::default(), MainDeviceConfig::default());
    /// # async {
    /// # let mut group = maindevice
    /// #     .init_single_group::<8, 8>(ethercat_now)
    /// #     .await
    /// #     .expect("Init");
    /// let subdevice = group.subdevice(&maindevice, 0).expect("No subdevice!");
    ///
    /// // This is equivalent to...
    /// subdevice.sdo_write_array(0x1c13, &[
    ///     0x1a00u16,
    ///     0x1a02,
    ///     0x1a04,
    ///     0x1a06,
    /// ]).await?;
    ///
    /// // ... this
    /// // subdevice.sdo_write(0x1c13, 0, 0u8).await?;
    /// // subdevice.sdo_write(0x1c13, 1, 0x1a00u16).await?;
    /// // subdevice.sdo_write(0x1c13, 2, 0x1a02u16).await?;
    /// // subdevice.sdo_write(0x1c13, 3, 0x1a04u16).await?;
    /// // subdevice.sdo_write(0x1c13, 4, 0x1a06u16).await?;
    /// // subdevice.sdo_write(0x1c13, 0, 4u8).await?;
    /// # Ok::<(), ethercrab::error::Error>(())
    /// # };
    /// ```
    pub async fn sdo_write_array<T>(&self, index: u16, values: impl AsRef<[T]>) -> Result<(), Error>
    where
        T: EtherCrabWireWrite,
    {
        let values = values.as_ref();

        self.sdo_write(index, 0, 0u8).await?;

        for (i, value) in values.iter().enumerate() {
            // Subindices start from 1
            let i = i + 1;

            self.sdo_write(index, i as u8, value).await?;
        }

        self.sdo_write(index, 0, values.len() as u8).await?;

        Ok(())
    }

    /// Read all sub-indices of the given SDO.
    ///
    /// This method will return an error if the number of sub-indices in the SDO is greater than
    /// `MAX_ENTRIES.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use ethercrab::{
    /// #     error::Error, MainDevice, MainDeviceConfig, PduStorage, Timeouts, std::ethercat_now
    /// # };
    /// # static PDU_STORAGE: PduStorage<8, 32> = PduStorage::new();
    /// # let (_tx, _rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");
    /// # let maindevice = MainDevice::new(pdu_loop, Timeouts::default(), MainDeviceConfig::default());
    /// # async {
    /// # let mut group = maindevice
    /// #     .init_single_group::<8, 8>(ethercat_now)
    /// #     .await
    /// #     .expect("Init");
    /// let subdevice = group.subdevice(&maindevice, 0).expect("No subdevice!");
    ///
    /// // Reading the TxPDO assignment
    /// // This is equivalent to...
    /// let values = subdevice.sdo_read_array::<u16, 3>(0x1c13).await?;
    ///
    /// // ... this
    /// // let len: u8 = subdevice.sdo_read(0x1c13, 0).await?;
    /// // let value1: u16 = subdevice.sdo_read(0x1c13, 1).await?;
    /// // let value2: u16 = subdevice.sdo_read(0x1c13, 2).await?;
    /// // let value3: u16 = subdevice.sdo_read(0x1c13, 3).await?;
    ///
    /// # Ok::<(), ethercrab::error::Error>(())
    /// # };
    /// ```
    pub async fn sdo_read_array<T, const MAX_ENTRIES: usize>(
        &self,
        index: u16,
    ) -> Result<heapless::Vec<T, MAX_ENTRIES>, Error>
    where
        T: EtherCrabWireReadSized,
    {
        let len = self.sdo_read::<u8>(index, 0).await?;

        if usize::from(len) > MAX_ENTRIES {
            return Err(Error::Capacity(Item::SdoSubIndex));
        }

        let mut values = heapless::Vec::new();

        for i in 1..=len {
            let value = self.sdo_read::<T>(index, i).await?;
            values.push(value).map_err(|_| Error::Internal)?;
        }

        Ok(values)
    }

    pub(crate) async fn sdo_read_expedited<T>(
        &self,
        index: u16,
        sub_index: impl Into<SubIndex>,
    ) -> Result<T, Error>
    where
        T: SdoExpeditedPayload,
    {
        debug_assert!(
            T::PACKED_LEN <= 4,
            "expedited transfers are up to 4 bytes long, this T is {}",
            T::PACKED_LEN
        );

        let sub_index = sub_index.into();

        let request = SdoNormal::upload(self.subdevice.mailbox_counter(), index, sub_index);

        fmt::trace!("CoE upload {:#06x} {:?}", index, sub_index);

        let (headers, response) = self.mailbox_write_read(request).await?;
        let data: &[u8] = &response;

        // Expedited transfers where the data is 4 bytes or less long, denoted in the SDO header
        // size value.
        if headers.sdo_header.expedited_transfer {
            let data_len = 4usize.saturating_sub(usize::from(headers.sdo_header.size));

            Ok(T::unpack_from_slice(
                data.get(0..data_len).ok_or(Error::Internal)?,
            )?)
        } else {
            Err(Error::Internal)
        }
    }

    /// Read a value from an SDO (Service Data Object) from the given index (address) and sub-index.
    pub async fn sdo_read<T>(&self, index: u16, sub_index: impl Into<SubIndex>) -> Result<T, Error>
    where
        T: EtherCrabWireReadSized,
    {
        let sub_index = sub_index.into();

        let mut storage = T::buffer();
        let buf = storage.as_mut();

        let request = SdoNormal::upload(self.subdevice.mailbox_counter(), index, sub_index);

        fmt::trace!("CoE upload {:#06x} {:?}", index, sub_index);

        let (headers, response) = self.mailbox_write_read(request).await?;
        let data: &[u8] = &response;

        // Expedited transfers where the data is 4 bytes or less long, denoted in the SDO header
        // size value.
        let response_payload = if headers.sdo_header.expedited_transfer {
            let data_len = 4usize.saturating_sub(usize::from(headers.sdo_header.size));

            data.get(0..data_len).ok_or(Error::Internal)?
        }
        // Data is either a normal upload or a segmented upload
        else {
            let data_length = headers.header.length.saturating_sub(0x0a);

            let complete_size = u32::unpack_from_slice(data)?;
            let data = data.get(u32::PACKED_LEN..).ok_or(Error::Internal)?;

            // The provided buffer isn't long enough to contain all mailbox data.
            if complete_size > buf.len() as u32 {
                return Err(Error::Mailbox(MailboxError::TooLong {
                    address: headers.sdo_header.index,
                    sub_index: headers.sdo_header.sub_index,
                }));
            }

            // If it's a normal upload, the response payload is returned in the initial mailbox read
            if complete_size <= u32::from(data_length) {
                data.get(0..usize::from(data_length))
                    .ok_or(Error::Internal)?
            }
            // If it's a segmented upload, we must make subsequent requests to load all segment data
            // from the read mailbox.
            else {
                let mut toggle = false;
                let mut total_len = 0usize;

                loop {
                    let request = SdoSegmented::upload(self.subdevice.mailbox_counter(), toggle);

                    fmt::trace!("CoE upload segmented");

                    let (headers, data) = self.mailbox_write_read(request).await?;

                    // The spec defines the data length as n-3, so we'll just go with that magic
                    // number...
                    let mut chunk_len = usize::from(headers.header.length - 3);

                    // Special case as per spec: Minimum response size is 7 bytes. For smaller
                    // responses, we must remove the number of unused bytes at the end of the
                    // response. Extremely weird.
                    if chunk_len == 7 {
                        chunk_len -= usize::from(headers.sdo_header.segment_data_size);
                    }

                    let data = data.get(0..chunk_len).ok_or(Error::Internal)?;

                    buf.get_mut(total_len..(total_len + chunk_len))
                        .ok_or(Error::Internal)?
                        .copy_from_slice(data);

                    total_len += chunk_len;

                    if headers.sdo_header.is_last_segment {
                        break;
                    }

                    toggle = !toggle;
                }

                buf.get(0..total_len).ok_or(Error::Internal)?
            }
        };

        T::unpack_from_slice(response_payload).map_err(|_| {
            fmt::error!(
                "SDO expedited data decode T: {} (len {}) data {:?} (len {})",
                type_name::<T>(),
                T::PACKED_LEN,
                response_payload,
                response_payload.len()
            );

            Error::Pdu(PduError::Decode)
        })
    }

    /// List out all of the CoE objects' addresses of kind `list_type`.
    ///
    /// For devices without CoE mailboxes, this will return `Ok(None)`.
    pub async fn sdo_info_object_description_list(
        &self,
        list_type: ObjectDescriptionListQuery,
    ) -> Result<Option<heapless::Vec<u16, /* # of u16s */ { u16::MAX as usize + 1 }>>, Error> {
        let request = ObjectDescriptionListRequest::get_object_description_list(
            self.subdevice.mailbox_counter(),
            list_type,
        );
        let Some(response_payload) = self.send_sdo_info_service(request).await? else {
            return Ok(None);
        };

        // The standard recommends to sort this, but I don't think that should be imposed onto the user
        <heapless::Vec<u16, 0x1_0000>>::unpack_from_slice(&response_payload)
            .map_err(|_| {
                fmt::error!(
                    "SDO Info Get OD List (type {}) data {:?} (len {})",
                    list_type,
                    response_payload,
                    response_payload.len()
                );

                Error::Pdu(PduError::Decode)
            })
            .map(Some)
    }

    /// Count how many objects match each [`ObjectDescriptionListQuery`].
    pub async fn sdo_info_object_quantities(
        &self,
    ) -> Result<Option<ObjectDescriptionListQueryCounts>, Error> {
        let request =
            ObjectDescriptionListRequest::get_object_quantities(self.subdevice.mailbox_counter());
        let Some(response_payload) = self.send_sdo_info_service(request).await? else {
            return Ok(None);
        };

        ObjectDescriptionListQueryCounts::unpack_from_slice(&response_payload)
            .map_err(|_| {
                fmt::error!(
                    "SDO Info Get OD List (type Object Quantities) data {:?} (len {})",
                    response_payload,
                    response_payload.len()
                );

                Error::Pdu(PduError::Decode)
            })
            .map(Some)
    }
}
