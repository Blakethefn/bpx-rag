use super::*;
use notify::event::{AccessKind, Flag, ModifyKind};

#[test]
fn access_events_are_not_recorded_as_changes() {
    let root = tempdir().unwrap();
    let (_data, mut engine) = engine_watching(root.path());
    let read = notify::Event::new(EventKind::Access(AccessKind::Read))
        .add_path(root.path().join("lib.rs"));

    deliver(&mut engine, Ok(read));

    let entry = &engine.watched[LABEL];
    assert!(entry.pending.is_empty());
    assert!(!entry.need_full);
    assert!(entry.first_event.is_none());
    assert!(entry.last_event.is_none());
}

#[test]
fn modified_file_inside_the_root_becomes_a_pending_relative_path() {
    let root = tempdir().unwrap();
    let (_data, mut engine) = engine_watching(root.path());

    deliver(&mut engine, modify(root.path().join("src").join("lib.rs")));

    let entry = &engine.watched[LABEL];
    assert_eq!(
        entry.pending.iter().cloned().collect::<Vec<_>>(),
        vec!["src/lib.rs".to_string()]
    );
    assert!(!entry.need_full);
    assert!(entry.first_event.is_some());
    assert!(entry.last_event.is_some());
}

#[test]
fn path_outside_the_root_requests_a_full_reconcile() {
    let root = tempdir().unwrap();
    let (_data, mut engine) = engine_watching(root.path());
    let outside = root.path().parent().unwrap().join("elsewhere.rs");

    deliver(&mut engine, modify(outside));

    let entry = &engine.watched[LABEL];
    assert!(entry.pending.is_empty());
    assert!(entry.need_full);
}

#[test]
fn event_on_the_root_itself_requests_a_full_reconcile() {
    let root = tempdir().unwrap();
    let (_data, mut engine) = engine_watching(root.path());

    deliver(&mut engine, modify(root.path().to_path_buf()));

    let entry = &engine.watched[LABEL];
    assert!(entry.pending.is_empty());
    assert!(entry.need_full);
}

#[cfg(windows)]
#[test]
fn backslash_separators_are_normalized_to_forward_slashes() {
    let root = tempdir().unwrap();
    let (_data, mut engine) = engine_watching(root.path());

    deliver(&mut engine, modify(root.path().join("sub\\lib.rs")));

    let entry = &engine.watched[LABEL];
    assert_eq!(
        entry.pending.iter().cloned().collect::<Vec<_>>(),
        vec!["sub/lib.rs".to_string()]
    );
    assert!(!entry.need_full);
}

#[test]
fn backslash_in_filename_follows_platform_rule() {
    let root = tempdir().unwrap();
    let (_data, mut engine) = engine_watching(root.path());

    deliver(&mut engine, modify(root.path().join("sub\\lib.rs")));

    let entry = &engine.watched[LABEL];
    let expected = if cfg!(windows) {
        "sub/lib.rs"
    } else {
        "sub\\lib.rs"
    };
    assert_eq!(
        entry.pending.iter().cloned().collect::<Vec<_>>(),
        vec![expected.to_string()]
    );
    assert!(!entry.need_full);
}

#[test]
fn rescan_flag_requests_a_full_reconcile_and_keeps_the_paths() {
    let root = tempdir().unwrap();
    let (_data, mut engine) = engine_watching(root.path());
    let rescan = notify::Event::new(EventKind::Modify(ModifyKind::Any))
        .add_path(root.path().join("a.rs"))
        .set_flag(Flag::Rescan);

    deliver(&mut engine, Ok(rescan));

    let entry = &engine.watched[LABEL];
    assert!(entry.need_full);
    assert!(entry.pending.contains("a.rs"));
}

#[test]
fn notify_error_requests_a_full_reconcile_and_records_the_error() {
    let root = tempdir().unwrap();
    let (_data, mut engine) = engine_watching(root.path());

    deliver(&mut engine, Err(notify::Error::generic("queue overflow")));

    let entry = &engine.watched[LABEL];
    assert!(entry.need_full);
    assert!(entry.pending.is_empty());
    let message = entry.last_error.as_deref().unwrap();
    assert!(message.starts_with("watch event error:"), "{message}");
    assert!(message.contains("queue overflow"), "{message}");
}
