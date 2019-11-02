use std::time::{Duration, Instant};

pub struct ContractTimer {
    creation: Instant,
    duration: Duration,
}

impl ContractTimer {
    pub fn new(duration: Duration) -> Self {
        Self {
            creation: Instant::now(),
            duration,
        }
    }
    pub fn expired(&self) -> bool {
        Instant::now().duration_since(self.creation) > self.duration
    }
}
