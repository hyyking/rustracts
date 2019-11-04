use std::sync::Mutex;
use std::time::Duration;

use crate::context::{ContextError, ContextErrorKind, ContractContext};
use crate::park::{WaitMessage, WaitThread};
use crate::sync::{LockArc, LockWeak};
use crate::time::Timer;
use crate::{Contract, ContractExt, Status};

use futures::{
    future::{FusedFuture, Future},
    task::{Context, Poll},
};

/// A FuturesContract produces a value from it's context at it's expire time if it has not been voided
/// before.
#[must_use = "contracts do nothing unless polled or awaited"]
pub struct FuturesContract<F, C, R>
where
    C: ContractContext,
    F: FnOnce(C) -> R,
{
    runner: WaitThread,
    timer: Timer,

    context: Option<LockArc<Mutex<C>>>,

    on_exe: Option<F>,
}

impl<F, C, R> FuturesContract<F, C, R>
where
    C: ContractContext,
    F: FnOnce(C) -> R,
{
    pub fn new(expire: Duration, context: C, on_exe: F) -> Self {
        Self {
            runner: WaitThread::new(),
            timer: Timer::new(expire),
            context: Some(LockArc::new(Mutex::new(context))),
            on_exe: Some(on_exe),
        }
    }

    pin_utils::unsafe_pinned!(timer: Timer);
    pin_utils::unsafe_unpinned!(on_exe: Option<F>);
    pin_utils::unsafe_unpinned!(context: Option<LockArc<Mutex<C>>>);
}

impl<F, C, R> Contract for FuturesContract<F, C, R>
where
    C: ContractContext,
    F: FnOnce(C) -> R,
{
    fn poll_valid(&self) -> bool {
        match &self.context {
            Some(c) => c.lock().unwrap().poll_valid(),
            None => false,
        }
    }

    fn execute(mut self: std::pin::Pin<&mut Self>) -> Self::Output {
        let lockarc = self
            .as_mut()
            .context()
            .take()
            .expect("Cannot poll after return");

        // Consumme LockArc to return the mutex
        let context = lockarc.consumme().into_inner().unwrap();

        let f = self
            .as_mut()
            .on_exe()
            .take()
            .expect("Cannot poll after return");

        Status::Completed(f(context))
    }

    fn void(self: std::pin::Pin<&mut Self>) -> Self::Output {
        Status::Terminated
    }
}

impl<F, C, R> ContractExt for FuturesContract<F, C, R>
where
    C: ContractContext,
    F: FnOnce(C) -> R,
{
    type Context = LockWeak<Mutex<C>>;

    fn get_context(&self) -> Result<Self::Context, ContextError> {
        match &self.context {
            Some(ref c) => Ok(LockArc::downgrade(c)),
            None => Err(ContextError::from(ContextErrorKind::ExpiredContext)),
        }
    }
}

impl<F, C, R> Future for FuturesContract<F, C, R>
where
    C: ContractContext,
    F: FnOnce(C) -> R,
{
    type Output = Status<R>;

    fn poll(mut self: std::pin::Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        self.runner
            .sender()
            .send(WaitMessage::WakeIn {
                waker: cx.waker().clone(),
                duration: Duration::new(0, 1000),
            })
            .unwrap();

        let mv = (self.as_mut().timer().poll(cx), self.poll_valid());
        match mv {
            (Poll::Ready(_), true) => Poll::Ready(self.execute()),
            (Poll::Pending, true) => Poll::Pending,
            (_, false) => Poll::Ready(self.void()),
        }
    }
}

impl<F, C, R> FusedFuture for FuturesContract<F, C, R>
where
    C: ContractContext,
    F: FnOnce(C) -> R,
{
    fn is_terminated(&self) -> bool {
        self.context.is_none() || self.on_exe.is_none()
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
            let mcontext = c.get_context().unwrap();
            move || {
                match mcontext.upgrade() {
                    Some(strong) => strong.lock().unwrap().0 = 1, // Modify context before contract ends
                    None => {}
                }
                // mcontext goes out of scope so strong count goes back to one
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
            let mcontext = c.get_context().unwrap();
            move || match mcontext.upgrade() {
                Some(strong) => strong.lock().unwrap().0 += 2,
                None => {}
            }
        })
        .join();

        if let Status::Completed(value) = futures::executor::block_on(c) {
            assert_eq!(value, 10);
        } else {
            assert!(false);
        }
    }

    #[test]
    fn loop_fut_updated_contract() {
        let context = GtContext(3, 2); // Context is valid while self.0 > self.1

        let c = FuturesContract::new(Duration::from_secs(1), context, |con| -> usize {
            con.0 + 5
        });

        let handle = std::thread::spawn({
            let mcontext = c.get_context().unwrap();
            move || loop {
                match mcontext.upgrade() {
                    Some(strong) => {
                        strong.lock().unwrap().0;
                    }
                    None => break,
                }
            }
        });

        if let Status::Completed(_) = futures::executor::block_on(c) {
            assert!(true);
        } else {
            assert!(false);
        }
        let _ = handle.join();
    }
}
