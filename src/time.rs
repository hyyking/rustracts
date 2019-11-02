use std::time::{Duration, Instant};

use crate::context::ContractContext;

/// Timer Object that can be polled
pub struct Timer {
    creation: Instant,
    pub duration: Duration,
}

impl Timer {
    /// Construct a new ContractTimer from a duration
    pub fn new(duration: Duration) -> Self {
        Self {
            creation: Instant::now(),
            duration,
        }
    }
    /// Check wether the timer has expired.
    pub fn expired(&self) -> bool {
        Instant::now().duration_since(self.creation) > self.duration
    }
}

impl ContractContext for Timer {
    fn poll_valid(&self) -> bool {
        self.expired()
    }
}
