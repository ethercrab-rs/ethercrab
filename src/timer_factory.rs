pub trait TimerFactory: core::future::Future + Unpin {
    fn timer(duration: core::time::Duration) -> Self;
}

impl TimerFactory for smol::Timer {
    fn timer(duration: core::time::Duration) -> Self {
        Self::after(duration)
    }
}
