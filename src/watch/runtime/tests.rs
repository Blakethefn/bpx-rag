use super::*;
use tempfile::tempdir;

#[test]
fn configured_runtime_directory_has_priority_when_writable() {
    let root = tempdir().unwrap();
    let db = root.path().join("db/knowledge.db");
    let configured = root.path().join("runtime");
    let selected = directory_with(
        &db,
        || Some(configured.clone()),
        |path| {
            assert_eq!(path, configured);
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(selected, configured);
}

#[test]
fn permission_denied_database_directory_falls_back_to_temp_directory() {
    let root = tempdir().unwrap();
    let db_dir = root.path().join("read-only");
    let fallback = std::env::temp_dir();
    let db = db_dir.join("knowledge.db");
    let selected = directory_with(
        &db,
        || None,
        |path| {
            if path == db_dir {
                Err(io::Error::from(io::ErrorKind::PermissionDenied))
            } else if path == fallback {
                Ok(())
            } else {
                Err(io::Error::other("unexpected directory"))
            }
        },
    )
    .unwrap();
    assert_eq!(selected, fallback);
}

#[test]
fn actual_probe_creates_and_removes_its_probe_file() {
    let dir = tempdir().unwrap();
    probe_writable(dir.path()).unwrap();
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}
