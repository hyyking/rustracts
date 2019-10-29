use futures::{
    future::Future,
    task::{Context, Poll},
};
use std::time::{Duration, Instant};

pub trait Contract: Sized {
    type Output;

    fn is_valid(&self) -> bool {
        true
    }
    fn is_met(&self) -> bool;
    fn execute(&self) -> Self::Output;
    fn void(self) {}
}

pub enum Status<R> {
    Completed(R),
    Voided,
}

#[must_use = "contracts do nothing unless polled or awaited"]
pub struct FuturesContract<F, C, R>
where
    C: Copy + Send,
    F: FnOnce(C) -> R + Copy,
{
    creation: Instant,
    expire: Duration,
    context: C,
    on_exe: F,
}

impl<F, C, R> FuturesContract<F, C, R>
where
    C: Copy + Send,
    F: FnOnce(C) -> R + Copy,
{
    pub fn new(expire: Duration, context: C, on_exe: F) -> Self {
        Self {
            creation: Instant::now(),
            expire,
            context,
            on_exe,
        }
    }
}

impl<F, C, R> Contract for FuturesContract<F, C, R>
where
    C: Copy + Send,
    F: FnOnce(C) -> R + Copy,
{
    type Output = R;

    fn is_met(&self) -> bool {
        Instant::now().duration_since(self.creation) > self.expire
    }

    fn execute(&self) -> Self::Output {
        (self.on_exe.clone())(self.context.clone())
    }
}

impl<F, C, R> Future for FuturesContract<F, C, R>
where
    C: Copy + Send,
    F: FnOnce(C) -> R + Copy,
{
    type Output = Status<<Self as Contract>::Output>;

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let mv = (self.is_met(), self.is_valid());

        // wakes up 4 times during it's lifetime to check if it should be voided
        std::thread::sleep(self.expire / 4);
        cx.waker().clone().wake();

        match mv {
            (true, true) => Poll::Ready(Status::Completed(self.execute())),
            (false, true) => Poll::Pending,
            (_, false) => Poll::Ready(Status::Voided),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{FuturesContract, Status};
    use std::time::Duration;

    #[test]
    fn simple_contract() {
        let c = FuturesContract::new(Duration::from_secs(1), 3, |context| -> usize {
            context + 5
        });
        if let Status::Completed(value) = futures::executor::block_on(c) {
            assert_eq!(value, 8)
        } else {
            assert!(false)
        }
    }
}
