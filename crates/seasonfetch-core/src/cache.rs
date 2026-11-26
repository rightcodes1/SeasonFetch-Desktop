use std::collections::HashMap;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
struct CacheRecord<T> {
    value: T,
    stored_at: Instant,
    last_accessed: Instant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheLookup<T> {
    pub value: T,
    pub stale: bool,
}

/// Bounded TTL + stale-while-revalidate in-memory cache shared across desktop & future Android clients.
pub struct StaleWhileRevalidateCache<T: Clone> {
    max_entries: usize,
    fresh_ttl: Duration,
    stale_ttl: Duration,
    entries: HashMap<String, CacheRecord<T>>,
}

impl<T: Clone> StaleWhileRevalidateCache<T> {
    pub fn new(max_entries: usize, fresh_ttl: Duration, stale_ttl: Duration) -> Self {
        Self {
            max_entries: max_entries.max(1),
            fresh_ttl,
            stale_ttl: stale_ttl.max(fresh_ttl),
            entries: HashMap::new(),
        }
    }

    pub fn get(&mut self, key: &str) -> Option<CacheLookup<T>> {
        let now = Instant::now();
        let entry = self.entries.get_mut(key)?;
        let age = now.saturating_duration_since(entry.stored_at);
        if age > self.stale_ttl {
            self.entries.remove(key);
            return None;
        }
        entry.last_accessed = now;
        Some(CacheLookup {
            value: entry.value.clone(),
            stale: age > self.fresh_ttl,
        })
    }

    pub fn set(&mut self, key: String, value: T) {
        let now = Instant::now();
        if self.entries.len() >= self.max_entries && !self.entries.contains_key(&key) {
            if let Some(oldest_key) = self
                .entries
                .iter()
                .min_by_key(|(_, record)| record.last_accessed)
                .map(|(k, _)| k.clone())
            {
                self.entries.remove(&oldest_key);
            }
        }
        self.entries.insert(
            key,
            CacheRecord {
                value,
                stored_at: now,
                last_accessed: now,
            },
        );
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enforces_bounds_and_returns_fresh_entries() {
        let mut cache = StaleWhileRevalidateCache::new(
            2,
            Duration::from_secs(60),
            Duration::from_secs(300),
        );
        cache.set("a".into(), 1);
        cache.set("b".into(), 2);
        assert_eq!(cache.get("a"), Some(CacheLookup { value: 1, stale: false }));
        cache.set("c".into(), 3);
        assert_eq!(cache.len(), 2);
        assert!(cache.get("c").is_some());
    }
}
