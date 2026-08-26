use chrono::{DateTime, Utc};
use dashmap::DashMap;
use std::sync::Arc;

const MAX_ENTRIES: usize = 10_000;

/// Prevents duplicate webhook events from triggering repeated workload mutations.
pub struct EventDeduper {
    seen: Arc<DashMap<String, DateTime<Utc>>>,
}

impl EventDeduper {
    pub fn new() -> Self {
        Self {
            seen: Arc::new(DashMap::new()),
        }
    }

    pub fn is_duplicate(&self, event_id: &str) -> bool {
        if self.seen.contains_key(event_id) {
            return true;
        }

        if self.seen.len() >= MAX_ENTRIES {
            let cutoff = Utc::now() - chrono::Duration::hours(1);
            self.seen.retain(|_, ts| *ts > cutoff);
        }

        self.seen.insert(event_id.to_string(), Utc::now());
        false
    }

    pub fn count(&self) -> usize {
        self.seen.len()
    }
}

impl Clone for EventDeduper {
    fn clone(&self) -> Self {
        Self {
            seen: Arc::clone(&self.seen),
        }
    }
}
