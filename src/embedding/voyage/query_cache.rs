use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub(super) const QUERY_CACHE_CAPACITY: usize = 2048;
const MAX_CACHED_QUERY_BYTES: usize = 8 * 1024;

/// Query-text to embedding map with least-recently-used eviction.
pub(super) struct QueryEmbeddingCache {
    capacity: usize,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    entries: HashMap<String, Entry>,
    clock: u64,
}

struct Entry {
    vector: Arc<[f32]>,
    last_used: u64,
}

impl QueryEmbeddingCache {
    pub(super) fn new() -> Self {
        Self::with_capacity(QUERY_CACHE_CAPACITY)
    }

    pub(super) fn with_capacity(capacity: usize) -> Self {
        Self {
            capacity,
            state: Mutex::new(State::default()),
        }
    }

    pub(super) fn get(&self, text: &str) -> Option<Arc<[f32]>> {
        let mut state = self.lock();
        state.clock += 1;
        let clock = state.clock;
        let entry = state.entries.get_mut(text)?;
        entry.last_used = clock;
        Some(entry.vector.clone())
    }

    /// Stores `vector` for `text` and returns the vector callers should use. When
    /// a concurrent miss already stored one, that one wins, so every caller of a
    /// text sees the same vector.
    pub(super) fn insert(&self, text: &str, vector: Vec<f32>) -> Arc<[f32]> {
        if self.capacity == 0 || text.len() > MAX_CACHED_QUERY_BYTES {
            return vector.into();
        }
        let mut state = self.lock();
        state.clock += 1;
        let clock = state.clock;
        if let Some(entry) = state.entries.get_mut(text) {
            entry.last_used = clock;
            return entry.vector.clone();
        }
        if state.entries.len() >= self.capacity {
            let oldest = state
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.last_used)
                .map(|(key, _)| key.clone());
            if let Some(key) = oldest {
                state.entries.remove(&key);
            }
        }
        let vector: Arc<[f32]> = vector.into();
        state.entries.insert(
            text.to_owned(),
            Entry {
                vector: vector.clone(),
                last_used: clock,
            },
        );
        vector
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f32) -> Vec<f32> {
        vec![x]
    }

    #[test]
    fn oldest_entry_is_evicted_at_capacity() {
        let cache = QueryEmbeddingCache::with_capacity(2);
        cache.insert("a", v(1.0));
        cache.insert("b", v(2.0));
        cache.insert("c", v(3.0));

        assert!(cache.get("a").is_none());
        assert_eq!(&*cache.get("b").unwrap(), &[2.0]);
        assert_eq!(&*cache.get("c").unwrap(), &[3.0]);
    }

    #[test]
    fn a_hit_protects_an_entry_from_eviction() {
        let cache = QueryEmbeddingCache::with_capacity(2);
        cache.insert("a", v(1.0));
        cache.insert("b", v(2.0));
        assert!(cache.get("a").is_some());
        cache.insert("c", v(3.0));

        assert!(cache.get("b").is_none());
        assert!(cache.get("a").is_some());
        assert!(cache.get("c").is_some());
    }

    #[test]
    fn size_never_exceeds_capacity() {
        let cache = QueryEmbeddingCache::with_capacity(8);
        for i in 0..100 {
            cache.insert(&format!("q{i}"), v(i as f32));
        }
        let held = (0..100)
            .filter(|i| cache.get(&format!("q{i}")).is_some())
            .count();
        assert_eq!(held, 8);
    }

    #[test]
    fn concurrent_second_insert_returns_the_first_vector() {
        let cache = QueryEmbeddingCache::with_capacity(2);
        let first = cache.insert("a", v(1.0));
        let second = cache.insert("a", v(9.0));
        assert_eq!(&*first, &[1.0]);
        assert_eq!(&*second, &[1.0]);
    }

    #[test]
    fn oversized_query_is_returned_but_not_stored() {
        let cache = QueryEmbeddingCache::with_capacity(2);
        let text = "x".repeat(MAX_CACHED_QUERY_BYTES + 1);
        let stored = cache.insert(&text, v(1.0));
        assert_eq!(&*stored, &[1.0]);
        assert!(cache.get(&text).is_none());
    }
}
