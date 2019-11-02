use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::context::ContractContext;
use crate::sync::{WaitMessage, WaitThread};
use crate::time::ContractTimer;
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
    F: FnOnce((VC, PC)) -> R + Clone,
{
    runner: WaitThread,
    timer: ContractTimer,

    void_context: Arc<Mutex<VC>>,
    prod_context: Arc<Mutex<PC>>,

    on_void: F,
}

impl<F, VC, PC, R> OptionContract<F, VC, PC, R>
where
    VC: ContractContext + Clone,
    PC: ContractContext + Clone,
    F: FnOnce((VC, PC)) -> R + Clone,
{
    pub fn new(expire: Duration, void_c: VC, prod_c: PC, on_void: F) -> Self {
        Self {
            runner: WaitThread::new(),
            timer: ContractTimer::new(expire),
            void_context: Arc::new(Mutex::new(void_c)),
            prod_context: Arc::new(Mutex::new(prod_c)),
            on_void,
        }
    }
    fn inner_valid(&self) -> bool {
        (*self.prod_context.lock().unwrap()).poll_valid()
    }
}

impl<F, VC, PC, R> Contract for OptionContract<F, VC, PC, R>
where
    VC: ContractContext + Clone,
    PC: ContractContext + Clone,
    F: FnOnce((VC, PC)) -> R + Clone,
{
    fn is_valid(&self) -> bool {
        (*self.void_context.lock().unwrap()).poll_valid()
    }

    // This contract cannot expire
    fn is_expired(&self) -> bool {
        self.timer.expired()
    }

    fn execute(&self) -> Self::Output {
        let vcontext = self.void_context.lock().unwrap().clone();
        let pcontext = self.prod_context.lock().unwrap().clone();
        Status::Completed((self.on_void.clone())((vcontext, pcontext)))
    }

    // This contract is bound and cannot be voided
    fn void(&self) -> Self::Output {
        Status::Terminated
    }
}

impl<F, VC, PC, R> ContractExt for OptionContract<F, VC, PC, R>
where
    VC: ContractContext + Clone,
    PC: ContractContext + Clone,
    F: FnOnce((VC, PC)) -> R + Clone,
{
    type Context = (Arc<Mutex<VC>>, Arc<Mutex<PC>>);

    fn get_context(&self) -> Self::Context {
        (self.void_context.clone(), self.prod_context.clone())
    }
}

impl<F, VC, PC, R> Future for OptionContract<F, VC, PC, R>
where
    VC: ContractContext + Clone,
    PC: ContractContext + Clone,
    F: FnOnce((VC, PC)) -> R + Clone,
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

        let mv = (self.is_expired(), self.is_valid(), self.inner_valid());
        match mv {
            (true, true, true) => Poll::Ready(self.execute()),
            (true, true, false) => Poll::Ready(self.void()),
            (false, true, _) => Poll::Pending,
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

        let handle = std::thread::spawn({
            let (_, pcontext) = c.get_context();
            move || {
                (*pcontext.lock().unwrap()).0 += 1;
            }
        });

        if let Status::Completed(val) = futures::executor::block_on(c) {
            assert_ne!(val, 6); // Contract has been voided since context is invalidated by update
        } else {
            assert!(true);
        }

        handle.join().unwrap();
    }
}
