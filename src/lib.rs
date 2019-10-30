use std::sync::{Arc, Mutex};

pub trait ContractContext: Copy {
    fn is_valid(&self) -> bool {
        true
    }
}

pub trait Contract: Sized {
    type Output;

    fn is_valid(&self) -> bool {
        true
    }
    fn is_expired(&self) -> bool;
    fn execute(&self) -> Status<Self::Output>;
    fn void(&self) -> Status<Self::Output>;
}

pub trait ContractExt<C> {
    fn get_context(&self) -> Arc<Mutex<C>>;
}

pub enum Status<R> {
    Completed(R),
    Voided,
}

mod futures;

pub use crate::futures::FuturesContract;
