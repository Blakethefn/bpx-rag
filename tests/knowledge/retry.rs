use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use super::add::{text, Home};
use tempfile::tempdir;

fn tree_with_unreadable_directory(root: &Path) -> (PathBuf, PathBuf) {
    let tree = root.join("tree");
    let locked = tree.join("locked");
    fs::create_dir_all(&locked).unwrap();
    fs::write(tree.join("readable.md"), "alpha beta gamma delta\n").unwrap();
    fs::write(locked.join("inner.md"), "epsilon zeta eta theta\n").unwrap();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
    (tree, locked)
}

fn restore(locked: &Path) {
    fs::set_permissions(locked, fs::Permissions::from_mode(0o755)).unwrap();
}

struct RestoreOnDrop(PathBuf);

impl Drop for RestoreOnDrop {
    fn drop(&mut self) {
        let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o755));
    }
}

#[test]
fn failed_first_sync_leaves_no_registered_source_and_a_retry_succeeds() {
    let root = tempdir().unwrap();
    let home = Home::new(root.path());
    let (tree, locked) = tree_with_unreadable_directory(root.path());
    let _guard = RestoreOnDrop(locked.clone());
    if fs::read_dir(&locked).is_ok() {
        return;
    }

    let failed = home.run(&["add", tree.to_str().unwrap(), "--label", "t"]);

    assert!(!failed.status.success(), "the first sync must fail here");
    let message = text(&failed.stderr);
    assert!(
        message.contains("failed while traversing knowledge source"),
        "the original error must be returned, got: {message}"
    );
    assert_eq!(
        home.count("SELECT COUNT(*) FROM knowledge_sources"),
        0,
        "a failed add must not leave a registered source"
    );
    assert_eq!(home.count("SELECT COUNT(*) FROM knowledge_files"), 0);
    assert_eq!(home.count("SELECT COUNT(*) FROM knowledge_chunk_meta"), 0);

    restore(&locked);
    let retried = home.run(&["add", tree.to_str().unwrap(), "--label", "t"]);
    assert!(
        retried.status.success(),
        "a retry under the same label must work: {}",
        text(&retried.stderr)
    );
    assert_eq!(home.count("SELECT COUNT(*) FROM knowledge_sources"), 1);
    assert_eq!(home.count("SELECT COUNT(*) FROM knowledge_files"), 2);
}
