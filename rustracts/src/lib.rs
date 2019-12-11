use std::{
    future::Future,
    // marker::Unpin,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use futures::{
    future::{FusedFuture, TryFuture},
    ready,
};
use pin_utils::{unsafe_pinned, unsafe_unpinned};
use tokio::time::{interval, Interval};

pub struct Futures<Fut>
where
    Fut: TryFuture<Ok = (), Error = ()>,
{
    clock: Interval,
    ticks: usize,
    f: Option<Fut>,
    store: Option<Fut::Ok>,
}

impl<Fut> Futures<Fut>
where
    Fut: TryFuture<Ok = (), Error = ()>,
{
    unsafe_pinned!(f: Option<Fut>);
    unsafe_pinned!(clock: Interval);

    unsafe_unpinned!(ticks: usize);
    unsafe_unpinned!(store: Option<Fut::Ok>);

    pub fn new(duration: Duration, f: Fut) -> Self {
        Self {
            clock: interval(duration / 4),
            ticks: 0,
            f: Some(f),
            store: None,
        }
    }

    // Polling for the next tick (this function is undocumented in tokio)
    fn poll_tick(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        self.as_mut().clock().poll_tick(cx).map(|_| {
            *self.as_mut().ticks() += 1;
        })
    }
}

impl<Fut> FusedFuture for Futures<Fut>
where
    Fut: TryFuture<Ok = (), Error = ()>,
{
    fn is_terminated(&self) -> bool {
        self.f.is_none() | (self.ticks > 4)
    }
}

impl<Fut> Future for Futures<Fut>
where
    Fut: TryFuture<Ok = (), Error = ()>,
{
    type Output = Result<Fut::Ok, Fut>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.is_terminated() {
            if self.store.is_some() {
                return Poll::Ready(Ok(self.store().take().unwrap()));
            }
            return Poll::Ready(Ok(()));
        }

        // follow through the ticks
        ready!(self.as_mut().poll_tick(cx));

        // future has produced a value, finish the contract
        if self.store.is_some() {
            return Poll::Pending;
        }

        match ready!(self.as_mut().f().as_pin_mut().unwrap().try_poll(cx)) {
            Ok(value) => {
                debug_assert!(self.as_mut().store().replace(value).is_none());
                return Poll::Pending;
            }
            Err(_) => {
                let f = unsafe { self.f().get_unchecked_mut().take().unwrap() };
                return Poll::Ready(Err(f));
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[tokio::test]
    async fn dev() {
        let _: Result<(), _> = Futures::new(Duration::from_secs(4), async {
            if 10 > 3 {
                Ok(())
            } else {
                Err(())
            }
        })
        .await;
    }
}
