//! A Contract is a structure that can be invalidated or expired, on expiration the execute
//! method is called, depending on the contract it could produce a value or not. If the contract is
//! not valid at the time of the check it will be voided and could produce a value depending on the
//! contract.
//!
//! Contracts are valid futures that can be run to completion on a reactor or awaited in an async
//! block.

#![deny(clippy::all)]

/// Contract Trait
pub trait Contract: ::futures::future::Future {
    /// Check wether the contract is still valid. Always true by default.
    fn poll_valid(&self) -> bool {
        true
    }

    /// Produce a status of the contract on expiration.
    fn execute(self: std::pin::Pin<&mut Self>) -> Self::Output;

    /// Produce a status of the contract on cancel.
    fn void(self: std::pin::Pin<&mut Self>) -> Self::Output;
}

/// Extention trait for Contracts.
pub trait ContractExt: Contract {
    type Context;
    /// Get a thread-safe handle to a ContractContext.
    fn get_context(&self) -> Result<Self::Context, ContextError>;
}

/// Status on completion/invalidation of a contract.
pub enum Status<R> {
    /// Contract has successfully produced a value.
    Completed(R),

    /// Contract has ended and did not produce a value.
    Terminated,
}

mod contracts;

#[macro_use]
mod macros;

/// Time utilities
pub mod time;

/// Implementation of contexes to put in a contract.
pub mod context;

/// Contains contract wakers.
pub mod sync;

/// Trait that defines a valid context for a contract.
pub use context::{ContextError, ContractContext};

/// Duration based contract produces a value at a point in the future using the available context if it
/// has not been voided before.
pub use crate::contracts::FuturesContract;

/// Permanent contract that produces a value when it is voided by it's context.
pub use crate::contracts::OnKillContract;

/// Duration based contract produces a value at a point in the future if it has not been voided and
/// secondary context has been realized.
pub use crate::contracts::OptionContract;
