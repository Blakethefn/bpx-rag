use rusqlite::Connection;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::tempdir;

pub(crate) struct Home {
    config: PathBuf,
    data: PathBuf,
    cache: PathBuf,
    cwd: PathBuf,
}

impl Home {
    pub(crate) fn new(root: &Path) -> Self {
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

    pub(crate) fn run(&self, args: &[&str]) -> Output {
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

    pub(crate) fn count(&self, sql: &str) -> i64 {
        self.db().query_row(sql, [], |row| row.get(0)).unwrap()
    }
}

pub(crate) fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
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
