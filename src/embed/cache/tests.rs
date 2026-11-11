use super::*;

#[test]
fn stored_vector_is_returned_and_counted_as_a_hit() {
    let cache = QueryEmbeddingCache::new(4);
    cache.insert("model-a", "parse config", vec![0.6, 0.8]);

    assert_eq!(
        cache.get("model-a", "parse config"),
        Some(vec![0.6, 0.8]),
        "a stored vector must be returned"
    );
    assert_eq!(cache.stats().hits, 1);
    assert_eq!(cache.stats().misses, 0);
    assert_eq!(cache.stats().entries, 1);
}

#[test]
fn unknown_query_is_a_miss() {
    let cache = QueryEmbeddingCache::new(4);
    cache.insert("model-a", "parse config", vec![1.0]);

    assert_eq!(cache.get("model-a", "unrelated"), None);
    assert_eq!(cache.stats().misses, 1);
    assert_eq!(cache.stats().hits, 0);
}

#[test]
fn least_recently_used_entry_is_evicted_first() {
    let cache = QueryEmbeddingCache::new(2);
    cache.insert("m", "a", vec![1.0]);
    cache.insert("m", "b", vec![2.0]);
    // Touch "a" so "b" becomes the least recently used entry.
    assert_eq!(cache.get("m", "a"), Some(vec![1.0]));
    cache.insert("m", "c", vec![3.0]);

    assert_eq!(cache.get("m", "b"), None, "b was least recently used");
    assert_eq!(cache.get("m", "a"), Some(vec![1.0]));
    assert_eq!(cache.get("m", "c"), Some(vec![3.0]));
    assert_eq!(cache.stats().entries, 2);
}

#[test]
fn same_query_under_another_model_is_a_miss() {
    let cache = QueryEmbeddingCache::new(4);
    cache.insert("model-a", "parse config", vec![1.0]);

    assert_eq!(cache.get("model-b", "parse config"), None);
    assert_eq!(cache.get("model-a", "parse config"), Some(vec![1.0]));
}

#[test]
fn zero_capacity_stores_nothing_and_counts_nothing() {
    let cache = QueryEmbeddingCache::new(0);
    cache.insert("m", "q", vec![1.0]);

    assert_eq!(cache.get("m", "q"), None);
    let stats = cache.stats();
    assert_eq!(stats.entries, 0);
    assert_eq!(stats.hits, 0);
    assert_eq!(stats.misses, 0);
    assert_eq!(stats.capacity, 0);
}

#[test]
fn oversized_query_is_not_stored() {
    let cache = QueryEmbeddingCache::new(4);
    let long_query = "x".repeat(MAX_CACHED_QUERY_BYTES + 1);
    cache.insert("m", &long_query, vec![1.0]);

    assert_eq!(cache.get("m", &long_query), None);
    assert_eq!(cache.stats().entries, 0);
}

#[test]
fn reinserting_an_existing_key_does_not_evict_another_entry() {
    let cache = QueryEmbeddingCache::new(2);
    cache.insert("m", "a", vec![1.0]);
    cache.insert("m", "b", vec![2.0]);
    cache.insert("m", "a", vec![1.5]);

    assert_eq!(cache.stats().entries, 2);
    assert_eq!(cache.get("m", "b"), Some(vec![2.0]));
    assert_eq!(cache.get("m", "a"), Some(vec![1.5]));
}

#[test]
fn keys_are_exact_text_so_case_variants_do_not_share_a_vector() {
    let cache = QueryEmbeddingCache::new(4);
    cache.insert("m", "Parse Config", vec![1.0]);

    // The embedder may tokenize case differently, so a variant must not reuse the vector.
    assert_eq!(cache.get("m", "parse config"), None);
}
