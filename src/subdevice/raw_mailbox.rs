use crate::{
    error::{Error, MailboxError},
    register::RegisterAddress,
    sync_manager_channel::Status as SmStatus,
    subdevice::{SubDevice, SubDeviceRef},
    timer_factory::IntoTimeout,
};
use core::{ops::Deref, time::Duration};

impl<'a, S> SubDeviceRef<'a, S>
where
    S: Deref<Target = SubDevice>,
{
    /// Send a complete mailbox telegram (6‑byte header + payload) to SM0 and read one reply from SM1.
    /// 
    /// This method expects a complete mailbox telegram including the 6-byte mailbox header followed
    /// by the payload. The telegram is forwarded unchanged (Type/Counter bits untouched) and exactly
    /// one reply telegram is returned from SM1, or timeout if none is produced within the specified
    /// timeout duration.
    /// 
    /// The device must be in PRE-OP state or higher. No PDI or DC configuration is required.
    /// 
    /// # Arguments
    /// * `request` - Complete mailbox telegram (6-byte header + payload)
    /// * `timeout` - Maximum time to wait for a response
    /// 
    /// # Returns
    /// * `Ok(Vec<u8>)` - Complete mailbox reply telegram including 6-byte header
    /// * `Err(Error::Mailbox(MailboxError::InvalidCount))` - Malformed request size
    /// * `Err(Error::Mailbox(MailboxError::TooLong))` - Request exceeds SM0 capacity
    /// * `Err(Error::Timeout)` - No response within timeout period
    pub async fn mailbox_raw_roundtrip(
        &self,
        request: &[u8],
        timeout: Duration,
    ) -> Result<Vec<u8>, Error> {
        // Basic header checks
        if request.len() < 6 {
            return Err(Error::Mailbox(MailboxError::InvalidCount));
        }
        let mbox_len = u16::from_le_bytes([request[0], request[1]]) as usize;
        if request.len() != 6 + mbox_len {
            return Err(Error::Mailbox(MailboxError::InvalidCount));
        }

        // Get mailbox configuration from SubDevice state (via Deref)
        let write_mailbox = self
            .config
            .mailbox
            .write
            .ok_or(Error::Mailbox(MailboxError::NoWriteMailbox))?;
        let read_mailbox = self
            .config
            .mailbox
            .read
            .ok_or(Error::Mailbox(MailboxError::NoReadMailbox))?;

        if request.len() > write_mailbox.len as usize {
            return Err(Error::Mailbox(MailboxError::TooLong { 
                address: self.configured_address, 
                sub_index: 0 
            }));
        }

        // Drain all stale data from SM1, not just one
        let sm1_status_addr = RegisterAddress::sync_manager_status(read_mailbox.sync_manager);
        let mut sm1_status: SmStatus = self.read(sm1_status_addr).receive(self.maindevice).await?;
        while sm1_status.mailbox_full {
            self.read(read_mailbox.address)
                .ignore_wkc()
                .receive_slice(self.maindevice, read_mailbox.len)
                .await?;
            sm1_status = self.read(sm1_status_addr).receive(self.maindevice).await?;
        }

        // Check SM0 not full (ready to accept)
        let sm0_status_addr = RegisterAddress::sync_manager_status(write_mailbox.sync_manager);
        let sm0_status: SmStatus = self.read(sm0_status_addr).receive(self.maindevice).await?;
        if sm0_status.mailbox_full {
            // Map to timeout to avoid adding NotReady publicly for now
            return Err(Error::Timeout);
        }

        // Write request to SM0 with exact request length
        self.write(write_mailbox.address)
            .with_len(request.len() as u16)
            .send(self.maindevice, request)
            .await?;

        // Poll SM1 until data (bounded by `timeout`)
        async {
            loop {
                let sm1: SmStatus = self.read(sm1_status_addr).receive(self.maindevice).await?;

                if sm1.mailbox_full {
                    let response = self.read(read_mailbox.address)
                        .receive_slice(self.maindevice, read_mailbox.len)
                        .await?;
                    
                    let reply = response.as_ref();

                    // Trim to header length
                    if reply.len() >= 6 {
                        let rep = u16::from_le_bytes([reply[0], reply[1]]) as usize;
                        if 6 + rep <= reply.len() {
                            let mut result = vec![0u8; 6 + rep];
                            result.copy_from_slice(&reply[..6 + rep]);
                            return Ok(result);
                        }
                    }
                    return Err(Error::Mailbox(MailboxError::InvalidCount));
                }

                self.maindevice.timeouts.loop_tick().await;
            }
        }
        .timeout(timeout)
        .await
    }
}
