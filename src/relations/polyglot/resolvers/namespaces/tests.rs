use std::path::{Path, PathBuf};

use super::jvm_targets;
use crate::relations::types::SourceFile;

#[test]
fn jvm_suffixes_come_from_the_extension_slice() {
    let files = [SourceFile {
        path: PathBuf::from("com/acme/Quote.java"),
        content: String::new(),
    }];

    let found = jvm_targets(Path::new("com/acme/Quote"), false, &files, &["java"]);

    assert_eq!(found, vec![PathBuf::from("com/acme/Quote.java")]);
}
