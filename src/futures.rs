use futures::{
    future::Future,
    task::{Context, Poll},
};

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::{Contract, ContractContext, ContractExt, Status};

#[must_use = "contracts do nothing unless polled or awaited"]
pub struct FuturesContract<F, C, R>
where
    C: ContractContext + Copy,
    F: FnOnce(C) -> R + Copy,
{
    creation: Instant,
    expire: Duration,
    context: Arc<Mutex<C>>,
    on_exe: F,
}

impl<F, C, R> FuturesContract<F, C, R>
where
    C: ContractContext + Copy,
    F: FnOnce(C) -> R + Copy,
{
    pub fn new(expire: Duration, context: C, on_exe: F) -> Self {
        Self {
            creation: Instant::now(),
            expire,
            context: Arc::new(Mutex::new(context)),
            on_exe,
        }
    }
}

impl<F, C, R> Contract for FuturesContract<F, C, R>
where
    C: ContractContext + Copy,
    F: FnOnce(C) -> R + Copy,
{
    type Output = R;

    fn is_valid(&self) -> bool {
        (*self.context.lock().unwrap()).is_valid()
    }

    fn is_expired(&self) -> bool {
        Instant::now().duration_since(self.creation) > self.expire
    }

    fn execute(&self) -> Status<Self::Output> {
        let context = self.context.lock().unwrap();
        Status::Completed((self.on_exe)(*context))
    }

    fn void(&self) -> Status<Self::Output> {
        Status::Voided
    }
}

impl<F, C, R> ContractExt<C> for FuturesContract<F, C, R>
where
    C: ContractContext + Copy,
    F: FnOnce(C) -> R + Copy,
{
    fn get_context(&self) -> Arc<Mutex<C>> {
        self.context.clone()
    }
}

impl<F, C, R> Future for FuturesContract<F, C, R>
where
    C: ContractContext + Copy,
    F: FnOnce(C) -> R + Copy,
{
    type Output = Status<<Self as Contract>::Output>;

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let mv = (self.is_expired(), self.is_valid());

        // wakes up 4 times during it's lifetime to check if it should be voided
        std::thread::sleep(self.expire / 4);
        cx.waker().clone().wake();

        match mv {
            (true, true) => Poll::Ready(self.execute()),
            (false, true) => Poll::Pending,
            (_, false) => Poll::Ready(self.void()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::FuturesContract;
    use crate::context::cmp::GtContext;
    use crate::{ContractExt, Status};

    use std::time::Duration;

    #[test]
    fn simple_contract() {
        let context: usize = 3;
        let c = FuturesContract::new(Duration::from_secs(1), context, |con| -> usize { con + 5 });

        if let Status::Completed(value) = futures::executor::block_on(c) {
            assert_eq!(value, 8)
        } else {
            assert!(false)
        }
    }

    #[test]
    fn voided_contract() {
        let context = GtContext(3, 2); // Context is true if self.0 > self.1

        let c = FuturesContract::new(Duration::from_secs(4), context, |con| -> usize {
            con.0 + 5
        });

        let handle = std::thread::spawn({
            let mcontext = c.get_context();
            move || {
                (*mcontext.lock().unwrap()).0 = 1; // Modify context before contract ends
            }
        });

        if let Status::Completed(val) = futures::executor::block_on(c) {
            assert_ne!(val, 1);
        } else {
            assert!(true); // Contract should be voided because updated value is 1 which is < 2
        }

        handle.join().unwrap();
    }

    #[test]
    fn updated_contract() {
        let context = GtContext(3, 2); // Context is valid if self.0 > self.1

        let c = FuturesContract::new(Duration::from_secs(1), context, |con| -> usize {
            con.0 + 5
        });

        let handle = std::thread::spawn({
            let mcontext = c.get_context();
            move || {
                (*mcontext.lock().unwrap()).0 += 2;
            }
        });

        if let Status::Completed(value) = futures::executor::block_on(c) {
            assert_eq!(value, 10);
        } else {
            assert!(false);
        }

        handle.join().unwrap();
    }
}
