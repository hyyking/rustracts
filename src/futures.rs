use ::core::{
    future::Future,
    marker::Unpin,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use crate::ContractResult;

use ::futures::{
    future::{Either, FusedFuture, TryFuture},
    ready,
};
use ::pin_utils::{unsafe_pinned, unsafe_unpinned};
use ::tokio::time::{delay_for, Delay};

#[derive(Debug)]
pub struct Futures<Fut: TryFuture> {
    core: Delay,

    f: Option<Fut>,
    store: Option<Fut::Ok>,
}

impl<Fut: TryFuture + Unpin> Unpin for Futures<Fut> {}

impl<Fut: TryFuture> Futures<Fut> {
    unsafe_pinned!(f: Option<Fut>);
    unsafe_pinned!(core: Delay);

    unsafe_unpinned!(store: Option<Fut::Ok>);

    pub(super) fn new(duration: Duration, f: Fut) -> Self {
        Self {
            core: delay_for(duration),
            f: Some(f),
            store: None,
        }
    }
    pub fn into_inner(self) -> Fut {
        self.f.expect("future has already returned")
    }
    fn poll_and_store(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Result<bool, Fut::Error> {
        if let Some(f) = self.as_mut().f().as_pin_mut() {
            match f.try_poll(cx) {
                Poll::Ready(Err(err)) => return Err(err),
                Poll::Ready(Ok(ok)) => {
                    if let None = self.store {
                        self.as_mut().store().replace(ok);
                        unsafe { drop(self.f().get_unchecked_mut().take()) }
                    }
                    return Ok(true);
                }
                _ => return Ok(false),
            }
        };
        Ok(false)
    }
}
impl<Fut: TryFuture> FusedFuture for Futures<Fut> {
    fn is_terminated(&self) -> bool {
        self.f.is_none()
    }
}
impl<Fut: TryFuture> Future for Futures<Fut> {
    type Output = ContractResult<Fut>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if let Err(e) = self.as_mut().poll_and_store(cx) {
            return Poll::Ready(Err(e));
        }

        ready!(self.as_mut().core().poll(cx));

        let val = if let Some(value) = self.as_mut().store().take() {
            Either::Left(value)
        } else {
            match self.as_mut().poll_and_store(cx) {
                Ok(true) => Either::Left(self.store().take().unwrap()),
                Ok(false) => {
                    if let Some(fut) = unsafe { self.as_mut().f().get_unchecked_mut().take() } {
                        Either::Right(fut)
                    } else {
                        debug_assert!(self.is_terminated());
                        return Poll::Pending;
                    }
                }
                Err(e) => return Poll::Ready(Err(e)),
            }
        };
        Poll::Ready(Ok(val))
    }
}

#[cfg(test)]
mod controled {

    use crate::{test_utils::*, ContractExt};

    #[tokio::test]
    async fn ok() {
        time::pause();

        let mut f = task::spawn(async { Result::<bool, ()>::Ok(true) }.as_futures(s(4)));

        time::advance(ms(4001)).await;

        match unwrap_ready(f.poll()) {
            Ok(Either::Left(tmd)) => assert!(tmd),
            _ => panic!("expected completed contract"),
        }
    }
    #[tokio::test]
    async fn err_canceled() {
        time::pause();

        let mut f = task::spawn(async { Result::<(), bool>::Err(true) }.as_futures(s(4)));
        time::advance(ms(1)).await;

        assert!(unwrap_ready(f.poll()).is_err());
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

        time::advance(ms(4001)).await;

        // right variant so the contract didn't receive the value
        match unwrap_ready(f.poll()) {
            Ok(Either::Right(tmd)) => assert_pending!(task::spawn(tmd).poll()), // work is still being done
            _ => panic!("expected timed out future"),
        }
    }
    #[tokio::test]
    async fn poll_after_timedout() {
        time::pause();

        let mut f = task::spawn(
            async {
                time::delay_for(s(5)).await;
                Result::<bool, ()>::Ok(true)
            }
                .as_futures(s(4)),
        );

        time::advance(ms(4001)).await;

        // right variant so the contract didn't receive the value
        match unwrap_ready(f.poll()) {
            Ok(Either::Right(tmd)) => assert_pending!(task::spawn(tmd).poll()), // work is still being done
            _ => panic!("expected timed out future"),
        }

        use ::futures::future::FusedFuture;
        use std::task::Poll;
        if let Poll::Ready(_) = f.poll() {
            assert!(f.enter(|_, f| f.is_terminated()));
            panic!("future should return pending after first successful poll")
        }
    }
    #[tokio::test]
    async fn last_second() {
        time::pause();

        // real test
        let mut f = task::spawn(
            async {
                time::delay_for(ms(4000)).await;
                Result::<bool, ()>::Ok(true)
            }
                .as_futures(ms(4000)),
        );
        drop(f.poll());
        time::advance(ms(4001)).await;

        match unwrap_ready(f.poll()) {
            Ok(Either::Left(tmd)) => assert!(tmd),
            Err(_) => panic!("unexpected error"),
            _ => panic!("expected completed contract"),
        }
    }
}

#[cfg(test)]
mod realtime {
    use crate::{test_utils::*, ContractExt};

    #[tokio::test]
    async fn ok() {
        let f = tokio::spawn(async { Result::<bool, ()>::Ok(true) }.as_futures(s(4)));

        match f.await.unwrap() {
            Ok(Either::Left(tmd)) => assert!(tmd),
            _ => panic!("expected completed contract"),
        }
    }
    #[tokio::test]
    async fn err_canceled() {
        let f = tokio::spawn(async { Result::<(), bool>::Err(true) }.as_futures(s(4)));

        assert!(f.await.unwrap().is_err());
    }
    #[tokio::test]
    async fn err_timedout() {
        let task = tokio::spawn(async {
            time::delay_for(s(10)).await;
            Result::<bool, ()>::Ok(true)
        });
        let f = tokio::spawn(task.as_futures(s(4)));

        // right variant so the contract didn't receive the value
        match f.await.unwrap() {
            Ok(Either::Right(tmd)) => assert_pending!(task::spawn(tmd).poll()), // work is still being done
            _ => panic!("expected timed out future"),
        }
    }
    #[tokio::test]
    async fn last_second() {
        let f = tokio::spawn(
            async {
                time::delay_for(ms(4000)).await;
                Result::<bool, ()>::Ok(true)
            }
                .as_futures(ms(4000)),
        );
        match f.await.unwrap() {
            Ok(Either::Left(tmd)) => assert!(tmd),
            Err(_) => panic!("unexpected error"),
            _ => panic!("expected completed contract"),
        }
    }
}
