use super::*;
use std::path::Path;

use crate::chunk::chunk_content_for_path;

mod rust;

fn specifiers(path: &str, content: &str, kind: &str) -> Vec<String> {
    let result = chunk_content_for_path(Path::new(path), content);
    let mut names = result
        .references
        .iter()
        .filter(|reference| reference.kind == kind)
        .map(|reference| reference.specifier.clone())
        .collect::<Vec<_>>();
    names.sort();
    names
}

fn qualified(path: &str, content: &str, kind: &str) -> Vec<(String, Option<String>)> {
    let result = chunk_content_for_path(Path::new(path), content);
    let mut rows = result
        .references
        .iter()
        .filter(|reference| reference.kind == kind)
        .map(|reference| (reference.specifier.clone(), reference.qualifier.clone()))
        .collect::<Vec<_>>();
    rows.sort();
    rows
}

#[test]
fn python_import_is_a_module_and_a_call_is_a_name() {
    let content = "import b\n\ndef only_a():\n    return helper()\n";
    let result = chunk_content_for_path(Path::new("a.py"), content);
    assert_eq!(result.symbol_status, STATUS_COMPLETE);
    assert_eq!(
        specifiers("a.py", content, KIND_MODULE),
        vec!["b".to_string()]
    );
    assert_eq!(
        specifiers("a.py", content, KIND_NAME),
        vec!["helper".to_string()]
    );
    assert!(result.symbols.iter().any(|symbol| symbol.name == "only_a"));
}

#[test]
fn python_main_guard_is_a_program_entry() {
    let result = chunk_content_for_path(
        Path::new("main.py"),
        "def start():\n    return 1\n\nif __name__ == \"__main__\":\n    start()\n",
    );
    assert!(result.entries.iter().any(|entry| {
        entry.kind == ENTRY_PROGRAM && entry.reason == "if __name__ == \"__main__\""
    }));
    assert_eq!(
        specifiers(
            "main.py",
            "def start():\n    return 1\n\nif __name__ == \"__main__\":\n    start()\n",
            KIND_NAME
        ),
        vec!["start".to_string()]
    );
}

#[test]
fn a_finished_walk_with_no_declarations_is_complete() {
    let result = chunk_content_for_path(Path::new("quiet.rs"), "// no declarations\n");
    assert_eq!(result.symbol_status, STATUS_COMPLETE);
    assert!(result.symbols.is_empty());
    assert!(result.references.is_empty());
}

#[test]
fn a_parse_error_keeps_chunks_and_stores_no_occurrences() {
    let content = "pub fn fine() {}\nfn broken( {\n";
    let result = chunk_content_for_path(Path::new("broken.rs"), content);
    assert_eq!(result.symbol_status, STATUS_PARSE_ERROR);
    assert!(result.symbols.is_empty());
    assert!(result.references.is_empty());
    assert!(!result.chunks.is_empty());
}

#[test]
fn markdown_is_unsupported() {
    let result = chunk_content_for_path(
        Path::new("readme.md"),
        "# Title\n\nfn looks_like_code() {}\n",
    );
    assert_eq!(result.symbol_status, STATUS_UNSUPPORTED);
    assert!(result.symbols.is_empty());
}

#[test]
fn cargo_bin_path_is_an_entry_on_the_manifest() {
    let content = "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n\n[[bin]]\nname = \"demo\"\npath = \"src/main.rs\"\n";
    let result = chunk_content_for_path(Path::new("Cargo.toml"), content);
    assert_eq!(result.symbol_status, STATUS_UNSUPPORTED);
    assert!(result.entries.iter().any(|entry| {
        entry.kind == ENTRY_PROGRAM
            && entry.reason == "Cargo.toml bin"
            && entry.target_rel_path.as_deref() == Some("src/main.rs")
    }));
}

#[test]
fn an_oversized_supported_file_is_too_large() {
    let padding = "x".repeat(2 * 1024 * 1024);
    let content = format!("// {padding}\nfn main() {{}}\n");
    let result = chunk_content_for_path(Path::new("src/main.rs"), &content);
    assert_eq!(result.symbol_status, STATUS_TOO_LARGE);
    assert!(result.symbols.is_empty());
    assert!(result.references.is_empty());
    assert!(result.entries.is_empty());
    assert!(!result.chunks.is_empty());
}
