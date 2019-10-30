use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::context::ContractContext;
use crate::{Contract, ContractExt, Status};

use futures::{
    future::Future,
    task::{Context, Poll},
};

/// Permanent contract that produces a value when it is voided
#[must_use = "contracts do nothing unless polled or awaited"]
pub struct OnKillContract<F, C, R>
where
    C: ContractContext + Clone,
    F: FnOnce(C) -> R + Clone,
{
    interval: Duration,
    context: Arc<Mutex<C>>,
    on_void: F,
}

impl<F, C, R> OnKillContract<F, C, R>
where
    C: ContractContext + Clone,
    F: FnOnce(C) -> R + Clone,
{
    pub fn new(interval: Duration, context: C, on_void: F) -> Self {
        Self {
            interval,
            context: Arc::new(Mutex::new(context)),
            on_void,
        }
    }
}

impl<F, C, R> Contract for OnKillContract<F, C, R>
where
    C: ContractContext + Clone,
    F: FnOnce(C) -> R + Clone,
{
    fn is_valid(&self) -> bool {
        (*self.context.lock().unwrap()).is_valid()
    }

    // This contract cannot expire
    fn is_expired(&self) -> bool {
        false
    }

    fn execute(&self) -> Self::Output {
        Status::Terminated
    }

    // This contract is bound and cannot be voided
    fn void(&self) -> Self::Output {
        let context = self.context.lock().unwrap().clone();
        Status::Completed((self.on_void.clone())(context))
    }
}

impl<F, C, R> ContractExt<C> for OnKillContract<F, C, R>
where
    C: ContractContext + Clone,
    F: FnOnce(C) -> R + Clone,
{
    fn get_context(&self) -> Arc<Mutex<C>> {
        self.context.clone()
    }
}

impl<F, C, R> Future for OnKillContract<F, C, R>
where
    C: ContractContext + Clone,
    F: FnOnce(C) -> R + Clone,
{
    type Output = Status<R>;

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        std::thread::sleep(self.interval);
        cx.waker().clone().wake();

        if !self.is_valid() {
            Poll::Ready(self.void())
        } else {
            Poll::Pending
        }
    }
}

#[cfg(test)]
mod tests {
    use super::OnKillContract;
    use crate::context::cmp::EqContext;
    use crate::{ContractExt, Status};

    use std::time::Duration;

    #[test]
    fn okc_contract() {
        let context = EqContext(2, 2); // Context which is valid while self.0 == self.1

        let c = OnKillContract::new(Duration::new(1, 0), context, |con| -> usize { con.0 + 5 });

        let handle = std::thread::spawn({
            let mcontext = c.get_context();
            move || {
                (*mcontext.lock().unwrap()).0 = 5; // Modify context
            }
        });

        if let Status::Completed(val) = futures::executor::block_on(c) {
            assert_eq!(val, 10); // Contract has been executed since context is invalidated by update
        } else {
            assert!(false);
        }

        handle.join().unwrap();
    }
}
