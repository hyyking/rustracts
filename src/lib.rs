#![deny(clippy::all)]

use std::sync::{Arc, Mutex};

/// A Contract is a structure that can be invalidated or expired, on expiration the execute
/// method is called, depending on the contract it could produce a value or not. If the contract is
/// not valid at the time of the check it will be voided and could produce a value depending on the
/// contract.
/// Contracts are valid futures that can be run to completion on a reactor or awaited in an async
/// block.
pub trait Contract: ::futures::future::Future {
    /// Check wether the contract is still valid
    fn is_valid(&self) -> bool {
        true
    }

    /// Check wether the contract has expired.
    fn is_expired(&self) -> bool;

    /// Produce a status of the contract on expiration.
    fn execute(&self) -> Self::Output;

    /// Produce a status of the contract on cancel.
    fn void(&self) -> Self::Output;
}

/// Extention trait for Contracts.
pub trait ContractExt<C>
where
    C: context::ContractContext,
{
    /// Get a thread-safe handle to a ContractContext.
    fn get_context(&self) -> Arc<Mutex<C>>;
}

/// Status on completion/invalidation of a contract.
pub enum Status<R> {
    /// Contract has successfully produced a value.
    Completed(R),

    /// Contract has ended and did not produce a value.
    Voided,
}

mod futures;

/// Implementation of contexes to put in a contract.
pub mod context;

/// Trait that defines a valid context for a contract.
pub use context::ContractContext;

/// Futures contract produces a value at a point in the future using the available context if it
/// has not been voided before.
pub use crate::futures::FuturesContract;
