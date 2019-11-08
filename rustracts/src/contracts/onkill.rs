use std::sync::Mutex;
use std::time::Duration;

use crate::context::{ContextError, ContextErrorKind, ContractContext};
use crate::park::{WaitMessage, WaitThread};
use crate::{Contract, ContractExt, Status};

use futures::{
    future::{FusedFuture, Future},
    task::{Context, Poll},
};
use parc::{LockWeak, ParentArc};

/// Permanent contract that produces a value when it is voided by the underlying context.
#[must_use = "contracts do nothing unless polled or awaited"]
pub struct OnKillContract<F, C, R>
where
    C: ContractContext,
    F: FnOnce(C) -> R,
{
    runner: WaitThread,

    context: Option<ParentArc<Mutex<C>>>,

    on_void: Option<F>,
}

impl<F, C, R> OnKillContract<F, C, R>
where
    C: ContractContext,
    F: FnOnce(C) -> R,
{
    pub fn new(context: C, on_void: F) -> Self {
        Self {
            runner: WaitThread::new(),
            context: Some(ParentArc::new(Mutex::new(context))),
            on_void: Some(on_void),
        }
    }

    pin_utils::unsafe_unpinned!(context: Option<ParentArc<Mutex<C>>>);
    pin_utils::unsafe_unpinned!(on_void: Option<F>);
}

impl<F, C, R> Contract for OnKillContract<F, C, R>
where
    C: ContractContext,
    F: FnOnce(C) -> R,
{
    fn poll_valid(&self) -> bool {
        match &self.context {
            Some(c) => c.as_ref().lock().unwrap().poll_valid(),
            None => false,
        }
    }

    fn execute(self: std::pin::Pin<&mut Self>) -> Self::Output {
        Status::Terminated
    }

    // This contract is bound and cannot be voided
    fn void(mut self: std::pin::Pin<&mut Self>) -> Self::Output {
        let lockarc = self
            .as_mut()
            .context()
            .take()
            .expect("Cannot poll after expiration");
        let context = lockarc.block_into_inner().into_inner().unwrap();

        let f = self
            .as_mut()
            .on_void()
            .take()
            .expect("Cannot poll after expiration");

        Status::Completed(f(context))
    }
}

impl<F, C, R> ContractExt for OnKillContract<F, C, R>
where
    C: ContractContext,
    F: FnOnce(C) -> R,
{
    type Context = LockWeak<Mutex<C>>;

    fn get_context(&self) -> Result<Self::Context, ContextError> {
        match &self.context {
            Some(ref c) => Ok(ParentArc::downgrade(c)),
            None => Err(ContextError::from(ContextErrorKind::ExpiredContext)),
        }
    }
}

impl<F, C, R> Future for OnKillContract<F, C, R>
where
    C: ContractContext,
    F: FnOnce(C) -> R,
{
    type Output = Status<R>;

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        self.runner
            .sender()
            .send(WaitMessage::WakeIn {
                waker: cx.waker().clone(),
                duration: Duration::new(0, 100),
            })
            .unwrap();

        if !self.poll_valid() {
            Poll::Ready(self.void())
        } else {
            Poll::Pending
        }
    }
}

impl<F, C, R> FusedFuture for OnKillContract<F, C, R>
where
    C: ContractContext,
    F: FnOnce(C) -> R,
{
    fn is_terminated(&self) -> bool {
        self.context.is_none() || self.on_void.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::OnKillContract;
    use crate::context::cmp::EqContext;
    use crate::{ContractExt, Status};

    #[test]
    fn okc_contract() {
        let context = EqContext(2, 2); // Context which is valid while self.0 == self.1

        let c = OnKillContract::new(context, |con| -> usize { con.0 + 5 });

        let _ = std::thread::spawn({
            let mcontext = c.get_context().unwrap();
            move || {
                match mcontext.upgrade() {
                    Some(mutex) => mutex.lock().unwrap().0 = 5, // Modify Context
                    None => {}
                }
            }
        })
        .join();

        if let Status::Completed(val) = futures::executor::block_on(c) {
            assert_eq!(val, 10); // Contract has been executed since context is invalidated by update
        } else {
            assert!(false);
        }
    }
}
