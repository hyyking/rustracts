use std::{
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use futures::{
    future::{Future, TryFuture},
    ready,
    stream::{FusedStream, Stream},
};
use tokio::time::{delay_until, Delay, Instant};

#[derive(Debug)]
pub struct Core<Fut> {
    core: ContractCore,
    f: Fut,
}
impl<Fut: TryFuture> Core<Fut> {
    pub(crate) fn new(bound: u32, duration: Duration, f: Fut) -> Self {
        Self {
            core: ContractCore::new(bound, duration),
            f,
        }
    }
}

#[derive(Debug)]
pub(crate) struct ContractCore {
    clock: Delay,
    duration: Duration,
    bound: u32,

    #[cfg(test)]
    pub(crate) tick: u32,

    #[cfg(not(test))]
    tick: u32,
}
impl ContractCore {
    pub(crate) fn new(bound: u32, duration: Duration) -> Self {
        Self {
            bound: bound,
            clock: delay_until(Instant::now() + Duration::new(0, 0)),
            duration: duration / bound,
            tick: 0,
        }
    }
}
impl FusedStream for ContractCore {
    fn is_terminated(&self) -> bool {
        self.tick >= self.bound
    }
}
impl Stream for ContractCore {
    type Item = Instant;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        println!("CORE POLL: {:?}", Instant::now());
        if self.is_terminated() {
            return Poll::Ready(None);
        }
        ready!(Pin::new(&mut self.clock).poll(cx));

        let now = Instant::now(); // self.clock.deadline();
        let next = self.duration;

        self.tick += 1;
        self.clock.reset(now + next);

        cx.waker().wake_by_ref();
        Poll::Ready(Some(now))
    }
}
