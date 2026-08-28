use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Version tracking for optimistic concurrency control (OCC)
#[derive(Clone)]
pub struct VersionTracker {
    current: Arc<AtomicU64>,
}

impl VersionTracker {
    pub fn new() -> Self {
        Self {
            current: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn current(&self) -> u64 {
        self.current.load(Ordering::SeqCst)
    }

    pub fn increment(&self) -> u64 {
        self.current.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub fn set(&self, version: u64) {
        self.current.store(version, Ordering::SeqCst);
    }

    pub fn compare_and_swap(&self, expected: u64, new: u64) -> Result<u64, u64> {
        self.current
            .compare_exchange(expected, new, Ordering::SeqCst, Ordering::SeqCst)
    }

    pub fn is_stale(&self, version: u64, allowed_drift: u64) -> bool {
        let current = self.current();
        current.saturating_sub(version) > allowed_drift
    }
}

impl Default for VersionTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_increment() {
        let tracker = VersionTracker::new();
        assert_eq!(tracker.current(), 0);

        let v1 = tracker.increment();
        assert_eq!(v1, 1);
        assert_eq!(tracker.current(), 1);

        let v2 = tracker.increment();
        assert_eq!(v2, 2);
    }

    #[test]
    fn test_compare_and_swap() {
        let tracker = VersionTracker::new();
        tracker.set(10);

        assert!(tracker.compare_and_swap(10, 11).is_ok());
        assert_eq!(tracker.current(), 11);

        // Stale CAS attempt fails
        let fail = tracker.compare_and_swap(10, 12);
        assert!(fail.is_err());
        assert_eq!(fail.unwrap_err(), 11);
    }

    #[test]
    fn test_stale_detection() {
        let tracker = VersionTracker::new();
        tracker.set(10);

        assert!(!tracker.is_stale(9, 2));
        assert!(!tracker.is_stale(8, 2));
        assert!(tracker.is_stale(7, 2));
        assert!(tracker.is_stale(5, 2));
    }
}
