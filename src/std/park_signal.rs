use std::{
    sync::Arc,
    task::Wake,
    thread::{self, Thread},
};

pub struct ParkSignal {
    current_thread: Thread,
}

impl ParkSignal {
    pub fn new() -> Self {
        Self {
            current_thread: thread::current(),
        }
    }

    pub fn wait(&self) {
        thread::park();
    }

    // fn wait_timeout(&self, timeout: Duration) {
    //     thread::park_timeout(timeout)
    // }
}

impl Wake for ParkSignal {
    fn wake(self: Arc<Self>) {
        self.current_thread.unpark();
    }
}
