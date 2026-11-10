//! Which Rust files may publish conclusions.
//!
//! A `parse_error` or `too_large` file stores no `mod` edges. Its path still marks the child
//! modules Rust would attach to it, and those files stay unsealed. An unsealed parent in the
//! in-memory module tree unseals its children. `unsupported` and a missing entry target do not.

use std::collections::HashSet;

use crate::chunk::evidence::{
    STATUS_COMPLETE, STATUS_PARSE_ERROR, STATUS_TOO_LARGE, STATUS_UNSUPPORTED,
};
use crate::knowledge::analysis::FileRow;
use crate::knowledge::references::REFERENCE_INDEX_VERSION;
use crate::knowledge::resolution::{is_rust_path, Tree};

pub(super) fn unsealed_rust_files(files: &[FileRow], tree: &Tree) -> HashSet<i64> {
    let mut unsealed = HashSet::new();
    for file in files {
        if is_rust_path(&file.rel_path) && file_is_broken(file) {
            unsealed.insert(file.id);
        }
    }
    let broken: Vec<i64> = unsealed.iter().copied().collect();
    for parent in broken {
        for child in tree.rust_file_ids() {
            if tree.path_declares_child(parent, child) {
                unsealed.insert(child);
            }
        }
    }
    loop {
        let mut added = Vec::new();
        for child in tree.rust_file_ids() {
            if unsealed.contains(&child) {
                continue;
            }
            if tree
                .parents_of(child)
                .iter()
                .any(|parent| unsealed.contains(parent))
            {
                added.push(child);
            }
        }
        if added.is_empty() {
            break;
        }
        unsealed.extend(added);
    }
    unsealed
}

pub(super) fn analysis_status(files: &[FileRow], unsealed: &HashSet<i64>) -> &'static str {
    if files
        .iter()
        .any(|file| file.reference_version < REFERENCE_INDEX_VERSION)
    {
        return "incomplete";
    }
    let rust_files: Vec<&FileRow> = files
        .iter()
        .filter(|file| is_rust_path(&file.rel_path))
        .collect();
    if rust_files.is_empty() {
        return if files.iter().any(|file| {
            blocks_conclusion(&file.symbol_status) || blocks_conclusion(&file.reference_status)
        }) {
            "incomplete"
        } else {
            "complete"
        };
    }
    let sealed = rust_files
        .iter()
        .filter(|file| !unsealed.contains(&file.id))
        .count();
    if sealed == 0 {
        "incomplete"
    } else if sealed < rust_files.len() {
        "partial"
    } else {
        "complete"
    }
}

fn file_is_broken(file: &FileRow) -> bool {
    is_broken_status(&file.symbol_status) || is_broken_status(&file.reference_status)
}

fn is_broken_status(status: &str) -> bool {
    status == STATUS_PARSE_ERROR || status == STATUS_TOO_LARGE
}

fn blocks_conclusion(status: &str) -> bool {
    status != STATUS_COMPLETE && status != STATUS_UNSUPPORTED
}
