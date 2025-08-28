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
    /// PRE‑OP is sufficient; no PDI/DC required.
    pub async fn mailbox_raw_roundtrip(
        &self,
        request: &[u8],
        timeout: Duration,
    ) -> Result<Vec<u8>, Error> {
        // Basic header checks (don't add public error variants here)
        if request.len() < 6 {
            return Err(Error::Mailbox(MailboxError::TooLong { 
                address: self.configured_address, 
                sub_index: 0 
            }));
        }
        let mbox_len = u16::from_le_bytes([request[0], request[1]]) as usize;
        if request.len() != 6 + mbox_len {
            // Map to generic error to avoid public API churn for now.
            return Err(Error::Timeout);
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

        // If SM1 has stale data, drain once
        let sm1_status_addr = RegisterAddress::sync_manager_status(read_mailbox.sync_manager);
        let sm1_status: SmStatus = self.read(sm1_status_addr).receive(self.maindevice).await?;
        if sm1_status.mailbox_full {
            self.read(read_mailbox.address)
                .ignore_wkc()
                .receive_slice(self.maindevice, read_mailbox.len)
                .await?;
        }

        // Check SM0 not full (ready to accept)
        let sm0_status_addr = RegisterAddress::sync_manager_status(write_mailbox.sync_manager);
        let sm0_status: SmStatus = self.read(sm0_status_addr).receive(self.maindevice).await?;
        if sm0_status.mailbox_full {
            // Map to timeout to avoid adding NotReady publicly for now
            return Err(Error::Timeout);
        }

        // Write request to SM0
        self.write(write_mailbox.address)
            .with_len(write_mailbox.len)
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
