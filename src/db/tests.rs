use super::*;
use tempfile::tempdir;

#[cfg(unix)]
#[test]
fn open_db_restricts_the_database_file_to_its_owner() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempdir().unwrap();
    let path = dir.path().join("store.db");
    let _conn = open_db(&path).unwrap();

    let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "database file must be owner-only");
}

#[cfg(unix)]
#[test]
fn create_private_dir_is_owner_only() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempdir().unwrap();
    let target = dir.path().join("nested").join("store");
    create_private_dir(&target).unwrap();

    let mode = fs::metadata(&target).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o700, "store directory must be owner-only");
}

#[test]
fn legacy_bpcontext_directory_is_never_silently_ignored() {
    let base = tempdir().unwrap();
    fs::create_dir(base.path().join(LEGACY_APP_NAME)).unwrap();

    let error = app_dir_under(base.path()).unwrap_err().to_string();

    assert!(
        error.contains("ln -s"),
        "error names the migration command: {error}"
    );
    assert!(
        error.contains("mv"),
        "error names the move alternative: {error}"
    );
}

#[test]
fn new_directory_wins_once_it_exists() {
    let base = tempdir().unwrap();
    fs::create_dir(base.path().join(LEGACY_APP_NAME)).unwrap();
    fs::create_dir(base.path().join(APP_NAME)).unwrap();

    let resolved = app_dir_under(base.path()).unwrap();

    assert_eq!(resolved, base.path().join(APP_NAME));
}
