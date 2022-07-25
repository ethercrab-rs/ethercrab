use crate::{
    client_inner::{ClientInternals, RequestState},
    command::Command,
    error::{Error, PduError},
    register::RegisterAddress,
    timer_factory::TimerFactory,
    PduData, ETHERCAT_ETHERTYPE, MASTER_ADDR,
};
use core::{future::Future, task::Poll};
use futures_lite::FutureExt;
use pnet::datalink::{self, DataLinkReceiver, DataLinkSender};
use smoltcp::wire::EthernetFrame;

pub type PduResponse<T> = (T, u16);

fn get_tx_rx(
    device: &str,
) -> Result<(Box<dyn DataLinkSender>, Box<dyn DataLinkReceiver>), std::io::Error> {
    let interfaces = datalink::interfaces();

    dbg!(&interfaces);

    let interface = interfaces
        .into_iter()
        .find(|interface| interface.name == device)
        .expect("Could not find interface");

    dbg!(interface.mac);

    let (tx, rx) = match datalink::channel(&interface, Default::default()) {
        Ok(datalink::Channel::Ethernet(tx, rx)) => (tx, rx),
        // FIXME
        Ok(_) => panic!("Unhandled channel type"),
        Err(e) => return Err(e),
    };

    Ok((tx, rx))
}

pub struct Client<const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, TIMEOUT> {
    client: std::sync::Arc<ClientInternals<MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>>,
}

// NOTE: Using a manual impl here as derived `Clone` can be too conservative. See:
//
// - https://www.reddit.com/r/rust/comments/42nuwc/cloning_phantomdata_problem/
// - https://github.com/rust-lang/rust/issues/26925
//
// This lets us clone `Client` even if `TIMEOUT` isn't `Clone`.
impl<const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, TIMEOUT> Clone
    for Client<MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>
{
    fn clone(&self) -> Self {
        Self {
            client: self.client.clone(),
        }
    }
}

impl<const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, TIMEOUT>
    Client<MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>
where
    TIMEOUT: TimerFactory + Send + 'static,
{
    pub fn new() -> Self {
        Self {
            client: std::sync::Arc::new(ClientInternals::new()),
        }
    }

    // TODO: Proper error - there are a couple of unwraps in here
    pub fn tx_rx_task(&self, device: &str) -> Result<impl Future<Output = ()>, std::io::Error> {
        let client_tx = self.client.clone();
        let client_rx = self.client.clone();

        let (mut tx, mut rx) = get_tx_rx(device)?;

        let tx_task = futures_lite::future::poll_fn::<(), _>(move |ctx| {
            if client_tx.send_waker.borrow().is_none() {
                client_tx
                    .send_waker
                    .borrow_mut()
                    .replace(ctx.waker().clone());
            }

            if let Ok(mut frames) = client_tx.frames.try_borrow_mut() {
                for request in frames.iter_mut() {
                    if let Some((state, pdu)) = request {
                        match state {
                            RequestState::Created => {
                                let mut packet_buf = [0u8; 1536];

                                let packet = pdu.to_ethernet_frame(&mut packet_buf).unwrap();

                                tx.send_to(packet, None).unwrap().expect("Send");

                                *state = RequestState::Waiting;
                            }
                            _ => (),
                        }
                    }
                }
            }

            Poll::Pending
        });

        let rx_task = smol::unblock(move || {
            loop {
                match rx.next() {
                    Ok(packet) => {
                        let packet = EthernetFrame::new_unchecked(packet);

                        // Look for EtherCAT packets whilst ignoring broadcast packets sent from self
                        if packet.ethertype() == ETHERCAT_ETHERTYPE
                            && packet.src_addr() != MASTER_ADDR
                        {
                            client_rx.parse_response_ethernet_frame(packet.payload());
                        }
                    }
                    Err(e) => {
                        // If an error occurs, we can handle it here
                        panic!("An error occurred while reading: {}", e);
                    }
                }
            }
        });

        Ok(tx_task.race(rx_task))
    }

    pub async fn brd<T>(&self, register: RegisterAddress) -> Result<PduResponse<T>, PduError>
    where
        T: PduData,
        <T as PduData>::Error: core::fmt::Debug,
    {
        let pdu = self
            .client
            .pdu(
                Command::Brd {
                    // Address is always zero when sent from master
                    address: 0,
                    register: register.into(),
                },
                // No input data; this is a read
                &[],
                T::len().try_into().expect("Length conversion"),
            )
            .await?;

        let res = T::try_from_slice(pdu.data.as_slice()).map_err(|e| {
            println!("{:?}", e);
            PduError::Decode
        })?;

        Ok((res, pdu.working_counter))
    }

    /// Auto Increment Physical Read.
    pub async fn aprd<T>(
        &self,
        address: u16,
        register: RegisterAddress,
    ) -> Result<PduResponse<T>, PduError>
    where
        T: PduData,
        <T as PduData>::Error: core::fmt::Debug,
    {
        let address = 0u16.wrapping_sub(address);

        let pdu = self
            .client
            .pdu(
                Command::Aprd {
                    address,
                    register: register.into(),
                },
                &[],
                T::len().try_into().expect("Length conversion"),
            )
            .await?;

        let res = T::try_from_slice(pdu.data.as_slice()).map_err(|e| {
            println!("{:?}", e);
            PduError::Decode
        })?;

        Ok((res, pdu.working_counter))
    }

    /// Configured address read.
    pub async fn fprd<T>(
        &self,
        address: u16,
        register: RegisterAddress,
    ) -> Result<PduResponse<T>, PduError>
    where
        T: PduData,
        <T as PduData>::Error: core::fmt::Debug,
    {
        let pdu = self
            .client
            .pdu(
                Command::Fprd {
                    address,
                    register: register.into(),
                },
                &[],
                T::len().try_into().expect("Length conversion"),
            )
            .await?;

        let res = T::try_from_slice(pdu.data.as_slice()).map_err(|e| {
            println!("{:?}", e);
            PduError::Decode
        })?;

        Ok((res, pdu.working_counter))
    }

    /// Auto Increment Physical Write.
    pub async fn apwr<T>(
        &self,
        address: u16,
        register: RegisterAddress,
        value: T,
    ) -> Result<PduResponse<T>, PduError>
    where
        T: PduData,
        <T as PduData>::Error: core::fmt::Debug,
    {
        let address = 0u16.wrapping_sub(address);

        let pdu = self
            .client
            .pdu(
                Command::Apwr {
                    address,
                    register: register.into(),
                },
                value.as_slice(),
                T::len().try_into().expect("Length conversion"),
            )
            .await?;

        let res = T::try_from_slice(pdu.data.as_slice()).map_err(|e| {
            println!("{:?}", e);
            PduError::Decode
        })?;

        Ok((res, pdu.working_counter))
    }
}
