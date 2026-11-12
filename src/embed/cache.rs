use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

pub const DEFAULT_CAPACITY: usize = 256;

pub const MAX_CACHED_QUERY_BYTES: usize = 4096;

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
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            inner: Mutex::new(Inner::default()),
        }
    }

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
