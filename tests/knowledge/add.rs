//! `add` registers a source and runs its first sync. If that sync fails, the source must not
//! stay registered with no content: a retry under the same label would be refused.

use rusqlite::Connection;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::tempdir;

struct Home {
    config: PathBuf,
    data: PathBuf,
    cache: PathBuf,
    cwd: PathBuf,
}

impl Home {
    fn new(root: &Path) -> Self {
        let home = Self {
            config: root.join("config"),
            data: root.join("data"),
            cache: root.join("cache"),
            cwd: root.join("cwd"),
        };
        for dir in [&home.config, &home.data, &home.cache, &home.cwd] {
            fs::create_dir_all(dir).unwrap();
        }
        home
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(std::env::var("CARGO_BIN_EXE_bpx-rag").unwrap())
            .args(args)
            .current_dir(&self.cwd)
            .env("XDG_CONFIG_HOME", &self.config)
            .env("XDG_DATA_HOME", &self.data)
            .env("XDG_CACHE_HOME", &self.cache)
            .env_remove("HF_HOME")
            .output()
            .unwrap()
    }

    fn db(&self) -> Connection {
        Connection::open(self.data.join("bpx-rag").join("knowledge.db")).unwrap()
    }

    fn count(&self, sql: &str) -> i64 {
        self.db().query_row(sql, [], |row| row.get(0)).unwrap()
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// A tree with one readable file and one directory the walk cannot enter. The walk itself
/// fails, which no per-file policy skips.
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

/// Makes the directory removable again even when an assertion panics.
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
        // Running as root: permissions do not block the walk, so the failure cannot be forced.
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

#[test]
fn successful_add_registers_and_indexes_the_source() {
    let root = tempdir().unwrap();
    let home = Home::new(root.path());
    let tree = root.path().join("tree");
    fs::create_dir_all(&tree).unwrap();
    fs::write(tree.join("note.md"), "alpha beta gamma delta\n").unwrap();

    let added = home.run(&["add", tree.to_str().unwrap(), "--label", "t"]);

    assert!(added.status.success(), "{}", text(&added.stderr));
    assert_eq!(home.count("SELECT COUNT(*) FROM knowledge_sources"), 1);
    assert_eq!(home.count("SELECT COUNT(*) FROM knowledge_files"), 1);
    assert!(home.count("SELECT COUNT(*) FROM knowledge_chunk_meta") > 0);
}

#[test]
fn duplicate_label_is_refused_with_the_existing_message_and_keeps_the_first_source() {
    let root = tempdir().unwrap();
    let home = Home::new(root.path());
    let tree = root.path().join("tree");
    fs::create_dir_all(&tree).unwrap();
    fs::write(tree.join("note.md"), "alpha beta gamma delta\n").unwrap();
    assert!(home
        .run(&["add", tree.to_str().unwrap(), "--label", "t"])
        .status
        .success());
    let chunks_before = home.count("SELECT COUNT(*) FROM knowledge_chunk_meta");

    let duplicate = home.run(&["add", tree.to_str().unwrap(), "--label", "t"]);

    assert!(!duplicate.status.success());
    assert!(
        text(&duplicate.stderr).contains("a knowledge source named 't' already exists"),
        "got: {}",
        text(&duplicate.stderr)
    );
    assert_eq!(home.count("SELECT COUNT(*) FROM knowledge_sources"), 1);
    assert_eq!(
        home.count("SELECT COUNT(*) FROM knowledge_chunk_meta"),
        chunks_before,
        "the original source's content must be untouched"
    );
}
