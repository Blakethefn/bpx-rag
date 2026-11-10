use super::*;
use std::path::Path;

use crate::chunk::chunk_content_for_path;

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
fn rust_module_import_and_main_are_separate_from_the_declaration() {
    let content = "mod util;\nfn main() {\n    helper();\n}\n";
    let result = chunk_content_for_path(Path::new("src/main.rs"), content);
    assert_eq!(result.symbol_status, STATUS_COMPLETE);
    assert_eq!(
        specifiers("src/main.rs", content, KIND_MODULE),
        vec!["util".to_string()]
    );
    assert_eq!(
        specifiers("src/main.rs", content, KIND_NAME),
        vec!["helper".to_string()]
    );
    assert!(result
        .entries
        .iter()
        .any(|entry| entry.kind == ENTRY_PROGRAM && entry.reason == "fn main"));
    assert!(result.symbols.iter().any(|symbol| symbol.name == "main"));
}

#[test]
fn rust_use_and_mod_references_keep_the_bound_name_and_path() {
    let content = "\
use foo::bar;
use foo::bar as baz;
use foo::*;
use foo::{a, b as c};
use crate::util::helper;
use super::item;
use foo::r#match as r#type;
use bar::foo::{self};
#[path = \"other.rs\"]
mod util;
mod plain;
mod inline {
    fn inside() {}
}
fn demo(value: crate::util::Helper) {
    crate::util::helper();
}
";
    assert_eq!(
        qualified("src/lib.rs", content, KIND_IMPORT),
        vec![
            ("*".to_string(), Some("foo".to_string())),
            ("a".to_string(), Some("foo".to_string())),
            ("bar".to_string(), Some("foo".to_string())),
            ("baz".to_string(), Some("foo::bar".to_string())),
            ("c".to_string(), Some("foo::b".to_string())),
            ("foo".to_string(), Some("bar".to_string())),
            ("helper".to_string(), Some("crate::util".to_string())),
            ("item".to_string(), Some("super".to_string())),
            ("type".to_string(), Some("foo::match".to_string())),
        ]
    );
    assert_eq!(
        qualified("src/lib.rs", content, KIND_MODULE),
        vec![
            ("plain".to_string(), None),
            ("util".to_string(), Some("other.rs".to_string())),
        ]
    );
    assert_eq!(
        qualified("src/lib.rs", content, KIND_NAME),
        vec![("helper".to_string(), Some("crate::util".to_string()))]
    );
    assert_eq!(
        qualified("src/lib.rs", content, KIND_TYPE),
        vec![("Helper".to_string(), Some("crate::util".to_string()))]
    );
}

#[test]
fn an_absolute_use_keeps_the_leading_colons() {
    let content = "\
use ::std::mem;
use ::foo;
use ::{a, b as c};
use ::*;
";
    assert_eq!(
        qualified("src/lib.rs", content, KIND_IMPORT),
        vec![
            ("*".to_string(), Some("::".to_string())),
            ("a".to_string(), Some("::".to_string())),
            ("c".to_string(), Some("::b".to_string())),
            ("foo".to_string(), Some("::".to_string())),
            ("mem".to_string(), Some("::std".to_string())),
        ]
    );
}

#[test]
fn a_path_attribute_survives_a_comment_and_keeps_escape_contents() {
    let commented = "#[path = \"other.rs\"]\n/// docs\nmod util;\n";
    assert_eq!(
        qualified("src/lib.rs", commented, KIND_MODULE),
        vec![("util".to_string(), Some("other.rs".to_string()))]
    );
    let escaped = "#[path = \"a\\u{2f}b.rs\"]\nmod util;\n";
    assert_eq!(
        qualified("src/lib.rs", escaped, KIND_MODULE),
        vec![("util".to_string(), Some("a/b.rs".to_string()))]
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
