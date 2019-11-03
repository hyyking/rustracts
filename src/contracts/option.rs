use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::context::ContractContext;
use crate::sync::{WaitMessage, WaitThread};
use crate::time::Timer;
use crate::{Contract, ContractExt, Status};

use futures::{
    future::Future,
    task::{Context, Poll},
};

/// Contract that produces a value if secondary context is valid at expiration
#[must_use = "contracts do nothing unless polled or awaited"]
pub struct OptionContract<F, VC, PC, R>
where
    VC: ContractContext + Clone,
    PC: ContractContext + Clone,
    F: FnOnce((VC, PC)) -> R,
{
    runner: WaitThread,
    timer: Timer,

    void_context: Option<Arc<Mutex<VC>>>,
    prod_context: Option<Arc<Mutex<PC>>>,

    on_exe: Option<F>,
}

impl<F, VC, PC, R> OptionContract<F, VC, PC, R>
where
    VC: ContractContext + Clone,
    PC: ContractContext + Clone,
    F: FnOnce((VC, PC)) -> R,
{
    pub fn new(expire: Duration, void_c: VC, prod_c: PC, on_exe: F) -> Self {
        Self {
            runner: WaitThread::new(),
            timer: Timer::new(expire),
            void_context: Some(Arc::new(Mutex::new(void_c))),
            prod_context: Some(Arc::new(Mutex::new(prod_c))),
            on_exe: Some(on_exe),
        }
    }
    fn inner_valid(&self) -> bool {
        match &self.prod_context {
            Some(c) => c.lock().unwrap().poll_valid(),
            None => false,
        }
    }

    pin_utils::unsafe_pinned!(timer: Timer);
    pin_utils::unsafe_unpinned!(void_context: Option<Arc<Mutex<VC>>>);
    pin_utils::unsafe_unpinned!(prod_context: Option<Arc<Mutex<PC>>>);
    pin_utils::unsafe_unpinned!(on_exe: Option<F>);
}

impl<F, VC, PC, R> Contract for OptionContract<F, VC, PC, R>
where
    VC: ContractContext + Clone,
    PC: ContractContext + Clone,
    F: FnOnce((VC, PC)) -> R,
{
    fn is_valid(&self) -> bool {
        match &self.void_context {
            Some(c) => c.lock().unwrap().poll_valid(),
            None => false,
        }
    }

    // This contract cannot expire
    fn is_expired(&self) -> bool {
        self.timer.expired()
    }

    fn execute(mut self: std::pin::Pin<&mut Self>) -> Self::Output {
        let vcontext = match Arc::try_unwrap(
            self.as_mut()
                .void_context()
                .take()
                .expect("Cannot poll after expiration"),
        ) {
            Ok(mutex) => mutex.into_inner().unwrap(), // Safe because it is the only reference to the mutex
            Err(arcmutex) => arcmutex.lock().unwrap().clone(),
        };

        let pcontext = match Arc::try_unwrap(
            self.as_mut()
                .prod_context()
                .take()
                .expect("Cannot poll after expiration"),
        ) {
            Ok(mutex) => mutex.into_inner().unwrap(), // Safe because it is the only reference to the mutex
            Err(arcmutex) => arcmutex.lock().unwrap().clone(),
        };

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
    VC: ContractContext + Clone,
    PC: ContractContext + Clone,
    F: FnOnce((VC, PC)) -> R,
{
    type Context = (Arc<Mutex<VC>>, Arc<Mutex<PC>>);

    fn get_context(&self) -> Self::Context {
        match (&self.void_context, &self.prod_context) {
            (Some(vc), Some(pc)) => (vc.clone(), pc.clone()),
            _ => panic!("Cannot get a reference to an expired context"),
        }
    }
}

impl<F, VC, PC, R> Future for OptionContract<F, VC, PC, R>
where
    VC: ContractContext + Clone,
    PC: ContractContext + Clone,
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
            self.is_valid(),
            self.inner_valid(),
        );
        match mv {
            (Poll::Ready(_), true, true) => Poll::Ready(self.execute()),
            (Poll::Ready(_), true, false) => Poll::Ready(self.void()),
            (Poll::Pending, true, _) => Poll::Pending,
            (_, false, _) => Poll::Ready(self.void()),
        }
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
            let (vcontext, _) = c.get_context();
            move || {
                (*vcontext.lock().unwrap()).0 += 1;
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
            let (_, pcontext) = c.get_context();
            move || {
                (*pcontext.lock().unwrap()).0 += 1;
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
