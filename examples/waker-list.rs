use core::cell::RefCell;
use core::sync::atomic::{AtomicU8, Ordering};
use core::task::Poll;
use core::task::Waker;
use core::time::Duration;
use heapless::FnvIndexMap;
use smol::LocalExecutor;
use std::sync::Arc;

fn main() {
    let local_ex = LocalExecutor::new();

    futures_lite::future::block_on(local_ex.run(async {
        let client = Arc::new(Client::default());

        let client2 = client.clone();
        let client3 = client.clone();

        local_ex
            .spawn(async move {
                // println!("Writing");

                async_io::Timer::after(Duration::from_millis(2000)).await;

                loop {
                    async_io::Timer::after(Duration::from_millis(500)).await;
                    client2.parse_response_ethernet_frame(&[client2.current_idx()]);
                }
            })
            .detach();

        loop {
            dbg!(client.current_idx());
            dbg!(client3.current_idx());

            async_io::Timer::after(Duration::from_millis(1000)).await;
            client.brd::<u8>(1010).await;
        }
    }));
}

// #[derive(Clone)]
struct Client {
    wakers: RefCell<FnvIndexMap<u8, Waker, 16>>,
    frames: RefCell<FnvIndexMap<u8, heapless::Vec<u8, 16>, 16>>,
    idx: AtomicU8,
}

impl Default for Client {
    fn default() -> Self {
        Self {
            wakers: RefCell::new(FnvIndexMap::new()),
            frames: RefCell::new(FnvIndexMap::new()),
            idx: AtomicU8::new(0),
        }
    }
}

impl Client {
    pub async fn brd<T>(&self, _address: u16) -> () {
        let idx = self.idx.fetch_add(1, Ordering::Release);

        println!("BRD {idx}");

        futures_lite::future::poll_fn(|ctx| {
            let removed = { self.frames.borrow_mut().remove(&idx) };

            println!("poll_fn idx {} {}", idx, removed.is_some());

            if let Some(frame) = removed {
                println!("poll_fn -> Ready");
                Poll::Ready(frame)
            } else {
                self.wakers
                    .borrow_mut()
                    .insert(idx, ctx.waker().clone())
                    .expect("Failed to insert waker");

                println!("poll_fn -> Pending");

                Poll::Pending
            }
        })
        .await;
    }

    pub fn parse_response_ethernet_frame(&self, ethernet_frame_payload: &[u8]) {
        let idx = ethernet_frame_payload[0];

        println!("Got ethernet frame {idx}");

        let waker = { self.wakers.borrow_mut().remove(&idx) };

        // Frame is ready; tell everyone about it
        if let Some(waker) = waker {
            println!("I have a waker {}", idx);
            self.frames
                .borrow_mut()
                .insert(idx, heapless::Vec::new())
                .expect("Failed to insert");
            waker.wake()
        }
    }

    // DELETEME
    pub fn current_idx(&self) -> u8 {
        self.idx.load(Ordering::Relaxed).saturating_sub(1)
    }
}
