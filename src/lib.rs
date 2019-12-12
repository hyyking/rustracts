#![warn(rust_2018_idioms)]

use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use futures::{
    future::{FusedFuture, TryFuture},
    ready,
    stream::{FusedStream, Stream},
};
use pin_utils::{unsafe_pinned, unsafe_unpinned};
use tokio::time::{interval, Instant, Interval};

type ContractError<E> = Result<Instant, E>;

pub trait ContractExt: TryFuture {
    fn as_futures(self, duration: Duration) -> Futures<Self>
    where
        Self: Sized,
    {
        Futures::new(duration, self)
    }
}
impl<Fut> ContractExt for Fut where Fut: TryFuture {}

#[derive(Debug)]
struct ContractCore {
    clock: Option<Interval>,
    bound: u32,
    tick: u32,
}
impl ContractCore {
    fn new(bound: u32, duration: Duration) -> Self {
        Self {
            bound: bound - 1,
            clock: Some(interval(duration / bound)),
            tick: 0,
        }
    }
    fn current(&self) -> u32 {
        self.tick
    }
}
impl FusedStream for ContractCore {
    fn is_terminated(&self) -> bool {
        self.tick > self.bound
    }
}
impl Stream for ContractCore {
    type Item = Instant;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.is_terminated() {
            drop(self.clock.take());
            return Poll::Ready(None);
        }
        let clock = self.clock.as_mut().unwrap(); // always init at tick 0
        clock.poll_tick(cx).map(|instant| {
            self.tick += 1;
            Some(instant)
        })
    }
}

#[derive(Debug)]
pub struct Futures<Fut>
where
    Fut: TryFuture,
{
    core: ContractCore,

    f: Option<Fut>,
    store: Option<Fut::Ok>,
}
impl<Fut> Futures<Fut>
where
    Fut: TryFuture,
{
    unsafe_pinned!(f: Option<Fut>);
    unsafe_pinned!(core: ContractCore);

    unsafe_unpinned!(store: Option<Fut::Ok>);

    pub fn new(duration: Duration, f: Fut) -> Self {
        Self {
            core: ContractCore::new(3, duration),
            f: Some(f),
            store: None,
        }
    }
    pub fn into_inner(self) -> Option<Fut> {
        self.f
    }
}
impl<Fut> FusedFuture for Futures<Fut>
where
    Fut: TryFuture,
{
    fn is_terminated(&self) -> bool {
        self.f.is_none()
    }
}
impl<Fut> Future for Futures<Fut>
where
    Fut: TryFuture,
{
    type Output = Result<Fut::Ok, ContractError<Fut::Error>>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.core.current() == 0 {
            let fut = self.as_mut().f().as_pin_mut().unwrap(); // always init at tick 0
            if let Poll::Ready(s) = fut.try_poll(cx) {
                match s {
                    Ok(value) => {
                        debug_assert!(self.as_mut().store().replace(value).is_none());
                        return Poll::Pending;
                    }
                    Err(e) => {
                        let _ = unsafe { self.f().get_unchecked_mut().take().unwrap() };
                        return Poll::Ready(Err(Err(e)));
                    }
                }
            }
        }

        // follow through the ticks and trigger on end
        if let None = ready!(self.as_mut().core().poll_next(cx)) {
            // if the store valued is there return it instead
            if let Some(s) = self.as_mut().store().take() {
                return Poll::Ready(Ok(s));
            }

            // if the future is stil there (meaning the store is empty since on reception we empty the
            // future slot) try to poll it one last time.
            if let Some(mut f) = unsafe { self.f().get_unchecked_mut().take() } {
                let p = unsafe { Pin::new_unchecked(&mut f).try_poll(cx) };
                return match p {
                    ok @ Poll::Ready(Ok(_)) => ok.map_err(|_| Ok(Instant::now())),
                    err @ Poll::Ready(Err(_)) => err.map_err(Err),
                    Poll::Pending => Poll::Ready(Err(Ok(Instant::now()))),
                };
            }
            // stream has ended so go in pending state
            return Poll::Pending;
        }

        // future has produced a value, finish the contract
        if self.store.is_some() {
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }

        let fut = self.as_mut().f().as_pin_mut().unwrap();
        match ready!(fut.try_poll(cx)) {
            Ok(value) => {
                debug_assert!(self.as_mut().store().replace(value).is_none());
                cx.waker().wake_by_ref();
                return Poll::Pending;
            }
            Err(e) => {
                let _ = unsafe { self.f().get_unchecked_mut().take().unwrap() };
                return Poll::Ready(Err(Err(e)));
            }
        }
    }
}

#[cfg(test)]
mod controled {
    use super::{
        realtime::{ms, s},
        ContractExt, Instant,
    };
    use tokio::time;
    use tokio_test::*;

    #[tokio::test]
    async fn ok() {
        time::pause();

        let mut f = task::spawn(async { Result::<bool, ()>::Ok(true) }.as_futures(s(4)));

        assert_pending!(f.poll());

        // simulate the ticks
        f.enter(|_, f| unsafe {
            f.get_unchecked_mut().core.tick += 4;
        });
        time::advance(s(4)).await;

        assert_ready_ok!(f.poll());
    }

    #[tokio::test]
    async fn err_canceled() {
        time::pause();

        let mut f = task::spawn(async { Result::<(), bool>::Err(true) }.as_futures(s(4)));

        assert!(assert_ready_err!(f.poll()).is_err());
    }

    #[tokio::test]
    async fn err_timedout() {
        time::pause();

        let mut f = task::spawn(
            async {
                time::delay_for(s(5)).await;
                Result::<bool, ()>::Ok(true)
            }
                .as_futures(s(4)),
        );

        assert_pending!(f.poll());
        // simulate the ticks
        f.enter(|_, f| unsafe {
            f.get_unchecked_mut().core.tick += 4;
        });
        time::advance(s(4)).await;
        // ok on error means the future timed out
        assert!(assert_ready_err!(f.poll()).is_ok());
    }

    #[tokio::test]
    async fn last_second() {
        time::pause();

        let mut f = task::spawn(
            async {
                time::delay_until(Instant::now() + ms(4000)).await;
                Result::<bool, ()>::Ok(true)
            }
                .as_futures(ms(4000)),
        );

        assert_pending!(f.poll());
        f.enter(|_, f| unsafe {
            f.get_unchecked_mut().core.tick += 4;
        });
        time::advance(ms(4001)).await;
        assert!(assert_ready_ok!(f.poll()));
    }
}

#[cfg(test)]
mod realtime {
    use super::{ContractExt, Duration, Instant};
    use futures::FutureExt;
    use tokio::time;
    use tokio_test::*;

    pub(super) fn s(t: u64) -> Duration {
        Duration::from_secs(t)
    }
    pub(super) fn ms(t: u64) -> Duration {
        Duration::from_millis(t)
    }

    #[tokio::test]
    async fn ok() {
        let f = tokio::spawn(async { Result::<bool, ()>::Ok(true) }.as_futures(s(4)));

        assert_ok!(assert_ok!(f.await));
    }

    #[tokio::test]
    async fn err_canceled() {
        let f = tokio::spawn(async { Result::<(), bool>::Err(true) }.as_futures(s(4)));

        assert!(assert_err!(assert_ok!(f.await)).is_err());
    }

    #[tokio::test]
    async fn err_timedout() {
        let f = tokio::spawn(
            async {
                time::delay_for(s(5)).await;
                Result::<bool, ()>::Ok(true)
            }
                .as_futures(s(4)),
        );

        assert!(assert_err!(assert_ok!(f.await)).is_ok());
    }

    #[tokio::test]
    async fn last_second() {
        let f = tokio::spawn(
            async {
                time::delay_until(Instant::now() + ms(4000)).await;
                Result::<bool, ()>::Ok(true)
            }
                .as_futures(ms(4000))
                .fuse(),
        );
        assert!(assert_ok!(f.await).is_ok());
    }
}
