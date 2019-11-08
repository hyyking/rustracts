use std::time::{Duration, Instant};

use crate::context::ContractContext;

use futures::{
    future::Future,
    task::{Context, Poll},
};

/// Timer future that will finish when it's time is done. Timers are also valid contract clauses.
pub struct Timer {
    creation: Instant,
    pub duration: Duration,
}

impl Timer {
    /// Construct a new ContractTimer from a Duration
    pub fn new(duration: Duration) -> Self {
        Self {
            creation: Instant::now(),
            duration,
        }
    }
    /// Check wether the timer has expired.
    pub fn expired(&self) -> bool {
        Instant::now().duration_since(self.creation) > self.duration
    }
}

impl ContractContext for Timer {
    fn poll_valid(&self) -> bool {
        self.expired()
    }
}

impl Future for Timer {
    type Output = ();

    fn poll(self: std::pin::Pin<&mut Self>, _: &mut Context) -> Poll<Self::Output> {
        if self.expired() {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}
