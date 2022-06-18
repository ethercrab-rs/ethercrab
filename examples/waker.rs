use core::task::Poll;
use core::task::Waker;
use core::time::Duration;
use smol::LocalExecutor;
use std::cell::Cell;
use std::rc::Rc;

fn main() {
    let local_ex = LocalExecutor::new();

    futures_lite::future::block_on(local_ex.run(async {
        let mut client = Client::default();

        let mut client2 = client.clone();

        local_ex
            .spawn(async move {
                async_io::Timer::after(Duration::from_secs(1)).await;

                println!("Writing");

                client2.parse_response_ethernet_frame(&[]);
            })
            .detach();

        client.brd::<u8>(1010).await;
    }));
}

struct Pdu {
    // TODO
}

#[derive(Clone)]
struct Client {
    // wakers: heapless::FnvIndexMap<u8, Waker, 16>,
    // pdus: heapless::FnvIndexMap<u8, Pdu, 16>,
    waker: Rc<Cell<Option<Waker>>>,

    // TODO: lmao
    frame: Rc<Cell<Option<u8>>>,
}

impl Default for Client {
    fn default() -> Self {
        Self {
            waker: Rc::new(Cell::new(None)),
            frame: Rc::new(Cell::new(None)),
        }
    }
}

impl Client {
    pub async fn brd<T>(&self, address: u16) -> () {
        // Create PDU

        // Push PDU into output queue

        // Wait for response (TODO: timeout)

        // self.wait_for_response(0).await;

        println!("Before");
        futures_lite::future::poll_fn(|ctx| {
            println!("poll_fn");

            if let Some(frame) = self.frame.take() {
                println!("poll_fn -> Ready");
                Poll::Ready(frame)
            } else {
                self.waker.replace(Some(ctx.waker().clone()));

                println!("poll_fn -> Pending");

                Poll::Pending
            }
        })
        .await;
        println!("After");

        // todo!()
    }

    // pub fn next_frame(&mut self) -> Option<&[u8]> {
    //     // Check if we have any ethercat frames to send

    //     // If we do, grab first one in list and return it

    //     // TODO: Use bbqueue
    // }

    pub fn parse_response_ethernet_frame(&mut self, ethernet_frame_payload: &[u8]) {
        println!("Got ethernet frame");

        // Parse frame

        // If we got a frame, see if there's a waker in the map

        // If there's a waker, wake it

        // Send frame to whatever waited for it

        // dbg!(&self.waker);
        dbg!(&self.frame);

        // Frame is ready; tell everyone about it
        if let Some(waker) = self.waker.take() {
            println!("I have a waker");
            self.frame.replace(Some(1u8));
            waker.wake()
        }
    }

    // fn wait_for_response<'a>(&'a mut self, idx: u8) -> ReplyFutureThingy<'a> {
    //     ReplyFutureThingy { idx, client: self }
    // }
}
