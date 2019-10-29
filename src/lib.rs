use futures::{
    future::Future,
    task::{Context, Poll},
};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub trait ValidContract {
    fn is_valid(&self) -> bool {
        true
    }
}

pub trait Contract: ValidContract + Sized {
    type Output;

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
    context: Arc<Mutex<C>>,
    on_exe: F,
}

impl<F, C, R> FuturesContract<F, C, R>
where
    C: Copy + Send,
    F: FnOnce(C) -> R + Copy,
{
    pub fn new(expire: Duration, context: Arc<Mutex<C>>, on_exe: F) -> Self {
        Self {
            creation: Instant::now(),
            expire,
            context,
            on_exe,
        }
    }
}

// TODO: Find a better way to validate contracts in a general way
impl<F, R> ValidContract for FuturesContract<F, usize, R>
where
    F: FnOnce(usize) -> R + Copy,
{
    fn is_valid(&self) -> bool {
        *self.context.lock().unwrap() > 2
    }
}

impl<F, R> Contract for FuturesContract<F, usize, R>
where
    F: FnOnce(usize) -> R + Copy,
{
    type Output = R;

    fn is_met(&self) -> bool {
        Instant::now().duration_since(self.creation) > self.expire
    }

    fn execute(&self) -> Self::Output {
        let context = self.context.lock().unwrap();
        (self.on_exe)(*context)
    }
}

impl<F, R> Future for FuturesContract<F, usize, R>
where
    F: FnOnce(usize) -> R + Copy,
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
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    #[test]
    fn simple_contract() {
        let context = Arc::new(Mutex::new(3));
        let c = FuturesContract::new(Duration::from_secs(1), context, |con| -> usize { con + 5 });
        if let Status::Completed(value) = futures::executor::block_on(c) {
            assert_eq!(value, 8)
        } else {
            assert!(false)
        }
    }

    #[test]
    fn voided_contract() {
        let context = Arc::new(Mutex::new(3 as usize));

        let handle = std::thread::spawn({
            let mcontext = context.clone();
            move || {
                *mcontext.lock().unwrap() = 1;
            }
        });
        // Contract will be voided since 1 < 2
        let c = FuturesContract::new(Duration::from_secs(4), context, |con| -> usize { con + 5 });

        if let Status::Completed(_) = futures::executor::block_on(c) {
            assert!(false);
        } else {
            assert!(true);
        }

        handle.join().unwrap();
    }

    #[test]
    fn updated_contract() {
        let context = Arc::new(Mutex::new(3 as usize));

        let handle = std::thread::spawn({
            let mcontext = context.clone();
            move || {
                *mcontext.lock().unwrap() += 2;
            }
        });
        // Contract will be voided since 1 < 2
        let c = FuturesContract::new(Duration::from_secs(1), context, |con| -> usize { con + 5 });

        if let Status::Completed(value) = futures::executor::block_on(c) {
            assert_eq!(value, 10);
        } else {
            assert!(true);
        }

        handle.join().unwrap();
    }
}
