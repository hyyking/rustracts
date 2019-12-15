use ::core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use crate::{core::ContractCore, ContractResult};

use ::futures::{
    future::{Either, FusedFuture, TryFuture},
    ready,
    stream::Stream,
};
use ::pin_utils::{unsafe_pinned, unsafe_unpinned};

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
    unsafe_unpinned!(f: Option<Fut>);
    unsafe_pinned!(core: ContractCore);

    unsafe_unpinned!(store: Option<Fut::Ok>);

    pub(super) fn new(core: ContractCore, f: Fut) -> Self {
        let f = Some(f);
        Self {
            core,
            f,
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
    type Output = ContractResult<Fut>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // poll core and trigger on end (core will initiate delays after first poll)
        if let None = ready!(self.as_mut().core().poll_next(cx)) {
            // if the store valued is there return it instead
            if let Some(s) = self.as_mut().store().take() {
                return Poll::Ready(Ok(Either::Left(s)));
            }

            // if the future is stil there (meaning the store is empty since on reception we empty the
            // future slot) try to poll it one last time.
            if let Some(mut f) = self.f().take() {
                let p = unsafe { Pin::new_unchecked(&mut f).try_poll(cx) };
                return match p {
                    Poll::Ready(Ok(ok)) => Poll::Ready(Ok(Either::Left(ok))),
                    Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
                    Poll::Pending => Poll::Ready(Ok(Either::Right(f))),
                };
            }
            // stream has ended so go in pending state
            return Poll::Pending;
        }

        // future has produced a value, wake to trigger next core tick
        if self.store.is_some() {
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }

        // on core tick poll the future to check for an error
        let fut = self.as_mut().f().as_mut().unwrap();
        match ready!(unsafe { Pin::new_unchecked(fut) }.try_poll(cx)) {
            Ok(value) => {
                debug_assert!(self.as_mut().store().replace(value).is_none());
                cx.waker().wake_by_ref();
                return Poll::Pending;
            }
            Err(e) => {
                let _ = self.f().take().unwrap();
                return Poll::Ready(Err(e));
            }
        }
    }
}

#[cfg(test)]
mod controled {

    use crate::{test_utils::*, ContractExt};

    #[tokio::test]
    async fn ok() {
        time::pause();

        let mut f = task::spawn(async { Result::<bool, ()>::Ok(true) }.as_futures(s(4)));

        // simulate the ticks
        f.enter(|_, f| unsafe {
            f.get_unchecked_mut().core.tick += 4;
        });
        time::advance(s(4)).await;

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

        // simulate the ticks
        f.enter(|_, f| unsafe {
            f.get_unchecked_mut().core.tick += 4;
        });
        time::advance(s(4)).await;

        // right variant so the contract didn't receive the value
        match unwrap_ready(f.poll()) {
            Ok(Either::Right(tmd)) => assert_pending!(task::spawn(tmd).poll()), // work is still being done
            _ => panic!("expected timed out future"),
        }
    }
    /*
        #[tokio::test]
        async fn last_second() {
            time::pause();

            // real test
            let fut = async {
                time::delay_until(time::Instant::now() + ms(4000)).await;
                Result::<bool, ()>::Ok(true)
            };
            let mut f = task::spawn(fut.as_futures(ms(4000)));

            drop(f.poll());
            f.enter(|_, f| unsafe {
                f.get_unchecked_mut().core.tick += 4;
            });
            time::advance(ms(4000)).await;

            match unwrap_ready(f.poll()) {
                Ok(Either::Left(tmd)) => assert!(tmd),
                Err(_) => panic!("unexpected error"),
                _ => panic!("expected completed contract"),
            }
        }
    */
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
                println!("BEFORE DELAY {:?}", time::Instant::now());
                time::delay_until(time::Instant::now() + ms(4000)).await;
                println!("AFTER DELAY {:?}", time::Instant::now());
                Result::<bool, ()>::Ok(true)
            }
                .as_futures(ms(5000)),
        );
        let f = f.await.unwrap();
        println!("FUT AWAITED {:?}", time::Instant::now());
        match f {
            Ok(Either::Left(tmd)) => assert!(tmd),
            Err(_) => panic!("unexpected error"),
            _ => panic!("expected completed contract"),
        }
    }
}
