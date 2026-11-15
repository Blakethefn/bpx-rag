use super::{IndexScope, RETRIEVAL_ANN_THRESHOLD, index_scope};

#[test]
fn small_unrestricted_scope_uses_exact_vector_scoring() {
    assert_eq!(index_scope(None, 1_000), IndexScope::Exact);
}

#[test]
fn narrow_scope_uses_exact_vector_scoring() {
    assert_eq!(
        index_scope(Some("repo"), RETRIEVAL_ANN_THRESHOLD - 1),
        IndexScope::Exact
    );
}

#[test]
fn large_single_source_scope_uses_source_index() {
    assert_eq!(
        index_scope(Some("repo"), RETRIEVAL_ANN_THRESHOLD),
        IndexScope::Source("repo")
    );
}

#[test]
fn large_filtered_multi_source_scope_uses_filtered_global_index() {
    assert_eq!(
        index_scope(None, RETRIEVAL_ANN_THRESHOLD),
        IndexScope::Global
    );
}

#[test]
fn large_unrestricted_scope_retains_global_index() {
    assert_eq!(
        index_scope(None, RETRIEVAL_ANN_THRESHOLD),
        IndexScope::Global
    );
}
