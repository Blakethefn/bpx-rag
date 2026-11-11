//! Bounded, thread-safe LRU cache for query embeddings.
//!
//! Entries are keyed by the embedding model id and the exact query text, so a vector is
//! only reused for the same model and the same embedder input. The cache holds vectors
//! that already passed normalization. Failed or invalid embeddings are never inserted.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

/// Default number of query embeddings kept per store.
pub const DEFAULT_CAPACITY: usize = 256;

/// Queries longer than this are not cached, which bounds the memory held by keys.
pub const MAX_CACHED_QUERY_BYTES: usize = 4096;

/// Counters for the query embedding cache.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub entries: usize,
    pub capacity: usize,
}

pub struct QueryEmbeddingCache {
    capacity: usize,
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    entries: HashMap<(String, String), Entry>,
    clock: u64,
    hits: u64,
    misses: u64,
}

struct Entry {
    vector: Vec<f32>,
    last_used: u64,
}

impl Default for QueryEmbeddingCache {
    fn default() -> Self {
        Self::new(DEFAULT_CAPACITY)
    }
}

impl QueryEmbeddingCache {
    /// Create a cache holding at most `capacity` entries. Zero disables caching.
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            inner: Mutex::new(Inner::default()),
        }
    }

    /// Return a copy of the cached vector for this model and query, if present.
    pub fn get(&self, model_id: &str, query: &str) -> Option<Vec<f32>> {
        if self.capacity == 0 {
            return None;
        }
        let mut guard = self.lock();
        let inner = &mut *guard;
        inner.clock += 1;
        let clock = inner.clock;
        match inner.entries.get_mut(&key(model_id, query)) {
            Some(entry) => {
                entry.last_used = clock;
                inner.hits += 1;
                Some(entry.vector.clone())
            }
            None => {
                inner.misses += 1;
                None
            }
        }
    }

    /// Store a normalized vector. Evicts the least recently used entry when full.
    pub fn insert(&self, model_id: &str, query: &str, vector: Vec<f32>) {
        if self.capacity == 0 || query.len() > MAX_CACHED_QUERY_BYTES {
            return;
        }
        let mut guard = self.lock();
        let inner = &mut *guard;
        inner.clock += 1;
        let entry_key = key(model_id, query);
        if !inner.entries.contains_key(&entry_key) && inner.entries.len() >= self.capacity {
            evict_least_recently_used(&mut inner.entries);
        }
        inner.entries.insert(
            entry_key,
            Entry {
                vector,
                last_used: inner.clock,
            },
        );
    }

    /// Counters for tests and benchmarks. Search does not report them.
    #[cfg(test)]
    pub fn stats(&self) -> CacheStats {
        let inner = self.lock();
        CacheStats {
            hits: inner.hits,
            misses: inner.misses,
            entries: inner.entries.len(),
            capacity: self.capacity,
        }
    }

    /// Entries are only inserted or removed whole, so a panic on another thread cannot leave
    /// one half-updated. Recover the guard rather than failing the search.
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn key(model_id: &str, query: &str) -> (String, String) {
    (model_id.to_string(), query.to_string())
}

fn evict_least_recently_used(entries: &mut HashMap<(String, String), Entry>) {
    let oldest = entries
        .iter()
        .min_by_key(|(_, entry)| entry.last_used)
        .map(|(entry_key, _)| entry_key.clone());
    if let Some(oldest) = oldest {
        entries.remove(&oldest);
    }
}

#[cfg(test)]
mod tests;
