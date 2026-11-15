use crate::support::make_store;

#[test]
fn remove_source_deletes_record() {
    let store = make_store();
    store.add_source("gone", "/tmp/gone", &[], &[]).unwrap();
    store.remove_source("gone").expect("remove");
    assert!(store.get_source("gone").unwrap().is_none());
}

#[test]
fn remove_source_returns_zero_counts_when_no_files() {
    let store = make_store();
    store.add_source("empty", "/tmp/empty", &[], &[]).unwrap();
    let result = store.remove_source("empty").unwrap();
    assert_eq!(result.files_removed, 0);
    assert_eq!(result.chunks_removed, 0);
}

#[test]
fn remove_source_nonexistent_errors() {
    let store = make_store();
    assert!(store.remove_source("nosuchsource").is_err());
}

#[test]
fn get_related_chunks_unknown_file_is_empty() {
    let store = make_store();
    let result = store.get_related_chunks(999, 10, None).unwrap();
    assert!(result.is_empty());
}
