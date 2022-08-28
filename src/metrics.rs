use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use tokio::time::Instant;

/// Statistics regarding the pool
#[derive(Debug)]
#[must_use]
pub struct PoolMetrics {
    /// The total time active time in microseconds.
    pub total_active: AtomicU64,
    /// The total time active time in microseconds.
    pub total_waiting: AtomicU64,
    /// The number of times an object request failed
    pub failure_count: AtomicUsize,
}

impl Default for PoolMetrics {
    fn default() -> Self {
        Self {
            total_active: AtomicU64::new(0),
            total_waiting: AtomicU64::new(0),
            failure_count: AtomicUsize::new(0),
        }
    }
}

// 64bit microseconds is 580000 years - really not important
#[allow(clippy::cast_possible_truncation)]
impl PoolMetrics {
    pub(crate) fn record_waiting(&self, start: Instant) {
        let waiting = start.elapsed().as_micros() as u64;
        let _ = self.total_waiting.fetch_add(waiting, Ordering::Relaxed);
    }

    pub(crate) fn record_active(&self, start: Instant) {
        let active = start.elapsed().as_micros() as u64;
        let _ = self.total_active.fetch_add(active, Ordering::Relaxed);
    }
}

impl PoolMetrics {
    /// Get the total number of microseconds that items were taken from the pool
    pub fn microseconds_active(&self) -> u64 {
        self.total_active.load(Ordering::Relaxed)
    }
    /// Get the total number of microseconds that tasks were waiting for an item in the pool
    pub fn microseconds_waiting(&self) -> u64 {
        self.total_waiting.load(Ordering::Relaxed)
    }
    /// Get the total number of failures to retrieve an item from the pool
    pub fn failure_count(&self) -> usize {
        self.failure_count.load(Ordering::Relaxed)
    }
}
