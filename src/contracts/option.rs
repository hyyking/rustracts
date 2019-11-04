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

/// Contract that produces a value if secondary context is valid at expiration and it has not been
/// voided by the first context.
#[must_use = "contracts do nothing unless polled or awaited"]
pub struct OptionContract<F, VC, PC, R>
where
    VC: ContractContext,
    PC: ContractContext,
    F: FnOnce((VC, PC)) -> R,
{
    runner: WaitThread,
    timer: Timer,

    void_context: Option<LockArc<Mutex<VC>>>,
    prod_context: Option<LockArc<Mutex<PC>>>,

    on_exe: Option<F>,
}

impl<F, VC, PC, R> OptionContract<F, VC, PC, R>
where
    VC: ContractContext,
    PC: ContractContext,
    F: FnOnce((VC, PC)) -> R,
{
    pub fn new(expire: Duration, void_c: VC, prod_c: PC, on_exe: F) -> Self {
        Self {
            runner: WaitThread::new(),
            timer: Timer::new(expire),
            void_context: Some(LockArc::new(Mutex::new(void_c))),
            prod_context: Some(LockArc::new(Mutex::new(prod_c))),
            on_exe: Some(on_exe),
        }
    }

    fn poll_prod(&self) -> bool {
        match &self.prod_context {
            Some(c) => c.lock().unwrap().poll_valid(),
            None => false,
        }
    }

    pin_utils::unsafe_pinned!(timer: Timer);
    pin_utils::unsafe_unpinned!(void_context: Option<LockArc<Mutex<VC>>>);
    pin_utils::unsafe_unpinned!(prod_context: Option<LockArc<Mutex<PC>>>);
    pin_utils::unsafe_unpinned!(on_exe: Option<F>);
}

impl<F, VC, PC, R> Contract for OptionContract<F, VC, PC, R>
where
    VC: ContractContext,
    PC: ContractContext,
    F: FnOnce((VC, PC)) -> R,
{
    fn poll_valid(&self) -> bool {
        match &self.void_context {
            Some(c) => c.lock().unwrap().poll_valid(),
            None => false,
        }
    }

    fn execute(mut self: std::pin::Pin<&mut Self>) -> Self::Output {
        let vlockarc = self
            .as_mut()
            .void_context()
            .take()
            .expect("Cannot poll after expiration");
        let plockarc = self
            .as_mut()
            .prod_context()
            .take()
            .expect("Cannot poll after expiration");

        let vcontext = vlockarc.consumme().into_inner().unwrap();
        let pcontext = plockarc.consumme().into_inner().unwrap();

        let f = self
            .as_mut()
            .on_exe()
            .take()
            .expect("Cannot run a contract after expiration");

        Status::Completed(f((vcontext, pcontext)))
    }

    // This contract is bound and cannot be voided
    fn void(self: std::pin::Pin<&mut Self>) -> Self::Output {
        Status::Terminated
    }
}

impl<F, VC, PC, R> ContractExt for OptionContract<F, VC, PC, R>
where
    VC: ContractContext,
    PC: ContractContext,
    F: FnOnce((VC, PC)) -> R,
{
    type Context = (LockWeak<Mutex<VC>>, LockWeak<Mutex<PC>>);

    fn get_context(&self) -> Result<Self::Context, ContextError> {
        match (&self.void_context, &self.prod_context) {
            (Some(ref vc), Some(ref pc)) => Ok((LockWeak::from(vc), LockWeak::from(pc))),
            _ => Err(ContextError::from(ContextErrorKind::ExpiredContext)),
        }
    }
}

impl<F, VC, PC, R> Future for OptionContract<F, VC, PC, R>
where
    VC: ContractContext,
    PC: ContractContext,
    F: FnOnce((VC, PC)) -> R,
{
    type Output = Status<R>;

    fn poll(mut self: std::pin::Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        self.runner
            .sender()
            .send(WaitMessage::WakeIn {
                waker: cx.waker().clone(),
                duration: Duration::new(0, 100),
            })
            .unwrap();

        let mv = (
            self.as_mut().timer().poll(cx),
            self.poll_valid(),
            self.poll_prod(),
        );
        match mv {
            (Poll::Ready(_), true, true) => Poll::Ready(self.execute()),
            (Poll::Ready(_), true, false) => Poll::Ready(self.void()),
            (Poll::Pending, true, _) => Poll::Pending,
            (_, false, _) => Poll::Ready(self.void()),
        }
    }
}

impl<F, VC, PC, R> FusedFuture for OptionContract<F, VC, PC, R>
where
    VC: ContractContext,
    PC: ContractContext,
    F: FnOnce((VC, PC)) -> R,
{
    fn is_terminated(&self) -> bool {
        self.void_context.is_none() || self.prod_context.is_none() || self.on_exe.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::OptionContract;
    use crate::context::cmp::EqContext;
    use crate::{ContractExt, Status};

    use std::time::Duration;

    #[test]
    fn prod_option_contract() {
        let vcontext = EqContext(2, 2); // Context which is valid while self.0 == self.1
        let pcontext = EqContext(2, 2); // Context which is valid while self.0 == self.1

        let c = OptionContract::new(
            Duration::new(1, 0),
            vcontext,
            pcontext,
            |(vcon, pcon)| -> usize { vcon.0 + pcon.0 + 1 },
        );

        if let Status::Completed(val) = futures::executor::block_on(c) {
            assert_eq!(val, 5); // Contract has been executed since context is invalidated by update
        } else {
            assert!(false);
        }
    }

    #[test]
    fn void_option_contract() {
        let vcontext = EqContext(2, 2); // Context which is valid while self.0 == self.1
        let pcontext = EqContext(2, 2); // Context which is valid while self.0 == self.1

        let c = OptionContract::new(
            Duration::new(1, 0),
            vcontext,
            pcontext,
            |(vcon, pcon)| -> usize { vcon.0 + pcon.0 + 1 },
        );

        let handle = std::thread::spawn({
            let (vcontext, _) = c.get_context().unwrap();
            move || match vcontext.upgrade() {
                Some(vc) => vc.lock().unwrap().0 += 1,
                None => {}
            }
        });

        if let Status::Completed(val) = futures::executor::block_on(c) {
            assert_ne!(val, 6); // Contract has been voided since context is invalidated by update
        } else {
            assert!(true);
        }

        handle.join().unwrap();
    }

    #[test]
    fn noprod_option_contract() {
        let vcontext = EqContext(2, 2); // Context which is valid while self.0 == self.1
        let pcontext = EqContext(2, 2); // Context which is valid while self.0 == self.1

        let c = OptionContract::new(
            Duration::new(1, 0),
            vcontext,
            pcontext,
            |(vcon, pcon)| -> usize { vcon.0 + pcon.0 + 1 },
        );

        let _ = std::thread::spawn({
            let (_, pcontext) = c.get_context().unwrap();
            move || match pcontext.upgrade() {
                Some(pc) => pc.lock().unwrap().0 += 1,
                None => {}
            }
        })
        .join();

        if let Status::Completed(val) = futures::executor::block_on(c) {
            assert_ne!(val, 6); // Contract has been voided since context is invalidated by update
        } else {
            assert!(true);
        }
    }
}
