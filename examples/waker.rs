use core::task::Poll;
use core::task::Waker;
use core::time::Duration;
use smol::LocalExecutor;
use std::cell::Cell;
use std::rc::Rc;

fn main() {
    let local_ex = LocalExecutor::new();

    futures_lite::future::block_on(local_ex.run(async {
        let client = Client::default();

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

#[derive(Clone)]
struct Client {
    waker: Rc<Cell<Option<Waker>>>,
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
    }

    pub fn parse_response_ethernet_frame(&mut self, ethernet_frame_payload: &[u8]) {
        println!("Got ethernet frame");

        // Frame is ready; tell everyone about it
        if let Some(waker) = self.waker.take() {
            println!("I have a waker");
            self.frame.replace(Some(1u8));
            waker.wake()
        }
    }
}
