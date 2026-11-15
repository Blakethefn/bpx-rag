use std::sync::atomic::Ordering;
use std::sync::Arc;

use super::{
    search, search_fingerprint, seeded_store, vector_layer_failed, CountingEmbedder, Output, MODEL,
    OTHER_MODEL,
};

#[test]
fn repeated_query_embeds_once_and_reuses_the_vector() {
    let (store, calls) = seeded_store(Output::Semantic, MODEL);

    search(&store, "ocean migration");
    search(&store, "ocean migration");

    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let stats = store.query_embedding_cache().stats();
    assert_eq!(stats.hits, 1);
    assert_eq!(stats.misses, 1);
}

#[test]
fn distinct_queries_each_embed_once() {
    let (store, calls) = seeded_store(Output::Semantic, MODEL);

    search(&store, "ocean migration");
    search(&store, "whale song");

    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn zero_capacity_embeds_on_every_search() {
    let (mut store, calls) = seeded_store(Output::Semantic, MODEL);
    store.set_query_embedding_cache_capacity(0);

    search(&store, "ocean migration");
    search(&store, "ocean migration");

    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(store.query_embedding_cache().stats().entries, 0);
}

#[test]
fn changing_the_model_misses_the_cache() {
    let (mut store, calls) = seeded_store(Output::Semantic, MODEL);
    search(&store, "ocean migration");

    store.set_embedder(Arc::new(CountingEmbedder {
        model: OTHER_MODEL,
        output: Output::Semantic,
        calls: Arc::clone(&calls),
    }));
    search(&store, "ocean migration");

    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "the new model must embed the query itself"
    );
}

#[test]
fn failed_query_embedding_is_not_cached() {
    let (store, calls) = seeded_store(Output::Failure, MODEL);

    let first = search(&store, "ocean migration");
    let second = search(&store, "ocean migration");

    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert!(vector_layer_failed(&first));
    assert!(vector_layer_failed(&second));
    assert_eq!(store.query_embedding_cache().stats().entries, 0);
}

#[test]
fn invalid_query_vector_is_not_cached() {
    let (store, calls) = seeded_store(Output::NonFinite, MODEL);

    let first = search(&store, "ocean migration");
    let second = search(&store, "ocean migration");

    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert!(vector_layer_failed(&first));
    assert!(vector_layer_failed(&second));
    assert_eq!(store.query_embedding_cache().stats().entries, 0);
}

#[test]
fn cached_and_uncached_searches_return_identical_results() {
    let (cached, _) = seeded_store(Output::Semantic, MODEL);
    let (mut uncached, _) = seeded_store(Output::Semantic, MODEL);
    uncached.set_query_embedding_cache_capacity(0);

    for query in ["ocean migration", "whale song", "queue acknowledgement"] {
        let expected = search_fingerprint(&uncached, query);
        assert_eq!(
            search_fingerprint(&cached, query),
            expected,
            "miss path: {query}"
        );
        assert_eq!(
            search_fingerprint(&cached, query),
            expected,
            "hit path: {query}"
        );
    }
    assert!(cached.query_embedding_cache().stats().hits >= 3);
}
