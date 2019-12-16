#![warn(rust_2018_idioms)]
#![warn(missing_debug_implementations)]

use std::time::Duration;

use ::futures::future::{Either, TryFuture};

mod core;
pub use crate::core::Core;

mod futures;
pub use crate::futures::Futures;

pub type ContractResult<Fut> =
    Result<Either<<Fut as TryFuture>::Ok, Fut>, <Fut as TryFuture>::Error>;

pub trait ContractExt: TryFuture {
    #[inline]
    fn with_core(self, max: u32, duration: Duration) -> Core<Self>
    where
        Self: Sized,
    {
        Core::new(max, duration, self)
    }
    /*
        #[inline]
        fn as_futures(self, duration: Duration) -> Futures<Self>
        where
            Self: Sized,
        {
            self.with_core(4, duration).as_futures()
        }
    */
    #[inline]
    fn as_futures(self, duration: Duration) -> Futures<Self>
    where
        Self: Sized,
    {
        Futures::new(duration, self)
    }
}

impl<Fut: ?Sized> ContractExt for Fut where Fut: TryFuture {}

#[cfg(test)]
mod test_utils {
    pub(crate) use futures::future::Either;
    pub(crate) use tokio::time;
    pub(crate) use tokio_test::*;

    use std::task::Poll;
    use std::time::Duration;

    pub(crate) fn unwrap_ready<O>(p: Poll<O>) -> O {
        match p {
            Poll::Ready(e) => e,
            _ => panic!("unwrap_ready call failed at Poll::Pending"),
        }
    }
    pub(crate) fn s(t: u64) -> Duration {
        Duration::from_secs(t)
    }
    pub(crate) fn ms(t: u64) -> Duration {
        Duration::from_millis(t)
    }
}
