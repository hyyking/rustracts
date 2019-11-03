use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::sync::{WaitMessage, WaitThread};
use crate::time::Timer;
use crate::{Contract, ContractContext, ContractExt, Status};

use futures::{
    future::Future,
    task::{Context, Poll},
};

/// A FuturesContract produces a value from it's context at it's expire time if it has not been voided
/// before.
#[must_use = "contracts do nothing unless polled or awaited"]
pub struct FuturesContract<F, C, R>
where
    C: ContractContext + Clone,
    F: FnOnce(C) -> R + Clone,
{
    runner: WaitThread,
    timer: Timer,

    context: Arc<Mutex<C>>,

    on_exe: F,
}

impl<F, C, R> FuturesContract<F, C, R>
where
    C: ContractContext + Clone,
    F: FnOnce(C) -> R + Clone,
{
    pub fn new(expire: Duration, context: C, on_exe: F) -> Self {
        Self {
            runner: WaitThread::new(),
            timer: Timer::new(expire),
            context: Arc::new(Mutex::new(context)),
            on_exe,
        }
    }
}

impl<F, C, R> Contract for FuturesContract<F, C, R>
where
    C: ContractContext + Clone,
    F: FnOnce(C) -> R + Clone,
{
    fn is_valid(&self) -> bool {
        (*self.context.lock().unwrap()).poll_valid()
    }

    fn is_expired(&self) -> bool {
        self.timer.expired()
    }

    fn execute(self: std::pin::Pin<&mut Self>) -> Self::Output {
        let context = self.context.lock().unwrap().clone();
        Status::Completed((self.on_exe.clone())(context))
    }
    fn void(self: std::pin::Pin<&mut Self>) -> Self::Output {
        Status::Terminated
    }
}

impl<F, C, R> ContractExt for FuturesContract<F, C, R>
where
    C: ContractContext + Clone,
    F: FnOnce(C) -> R + Clone,
{
    type Context = Arc<Mutex<C>>;

    fn get_context(&self) -> Self::Context {
        self.context.clone()
    }
}

impl<F, C, R> Future for FuturesContract<F, C, R>
where
    C: ContractContext + Clone,
    F: FnOnce(C) -> R + Clone,
{
    type Output = Status<R>;

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        self.runner
            .sender()
            .send(WaitMessage::WakeIn {
                waker: cx.waker().clone(),
                duration: Duration::new(0, 1000),
            })
            .unwrap();

        let mv = (self.is_expired(), self.is_valid());
        match mv {
            (true, true) => Poll::Ready(self.execute()),
            (false, true) => Poll::Pending,
            (_, false) => Poll::Ready(self.void()),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{context::cmp::GtContext, ContractExt, FuturesContract, Status};

    use std::time::Duration;

    #[test]
    fn fut_simple_contract() {
        let c = FuturesContract::new(Duration::from_secs(1), (), |_| -> usize { 5 });

        if let Status::Completed(value) = futures::executor::block_on(c) {
            assert_eq!(value, 5)
        } else {
            assert!(false)
        }
    }

    #[test]
    fn fut_voided_contract() {
        let context = GtContext(3, 2); // Context is true while self.0 > self.1

        let c = FuturesContract::new(Duration::from_secs(4), context, |con| -> usize {
            con.0 + 5
        });

        let _ = std::thread::spawn({
            let mcontext = c.get_context();
            move || {
                (*mcontext.lock().unwrap()).0 = 1; // Modify context before contract ends
            }
        })
        .join();

        if let Status::Completed(val) = futures::executor::block_on(c) {
            assert_ne!(val, 1);
        } else {
            assert!(true); // Contract should be voided because updated value is 1 which is < 2
        }
    }

    #[test]
    fn fut_updated_contract() {
        let context = GtContext(3, 2); // Context is valid while self.0 > self.1

        let c = FuturesContract::new(Duration::from_secs(1), context, |con| -> usize {
            con.0 + 5
        });

        let _ = std::thread::spawn({
            let mcontext = c.get_context();
            move || {
                (*mcontext.lock().unwrap()).0 += 2;
            }
        })
        .join();

        if let Status::Completed(value) = futures::executor::block_on(c) {
            assert_eq!(value, 10);
        } else {
            assert!(false);
        }
    }
}
