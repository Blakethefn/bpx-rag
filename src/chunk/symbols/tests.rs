use super::*;
use std::path::Path;

mod go;
mod python;
mod rust;
mod typescript;

fn describe(symbols: &[Symbol]) -> Vec<String> {
    symbols
        .iter()
        .map(|symbol| {
            let qualified = match &symbol.container {
                Some(container) => format!("{container}::{}", symbol.name),
                None => symbol.name.clone(),
            };
            format!(
                "{} {} {}-{}",
                symbol.kind, qualified, symbol.line_start, symbol.line_end
            )
        })
        .collect()
}

#[test]
fn non_rust_declarations_store_null_visibility() {
    let python = symbols_for_path(Path::new("app.py"), "def load():\n    return 1\n");
    assert!(python.iter().any(|symbol| symbol.name == "load"));
    assert!(python.iter().all(|symbol| symbol.visibility.is_none()));

    let go = symbols_for_path(Path::new("store.go"), "package store\n\nfunc Open() {}\n");
    assert!(go.iter().any(|symbol| symbol.name == "Open"));
    assert!(go.iter().all(|symbol| symbol.visibility.is_none()));
}

#[test]
fn unsupported_language_reports_no_symbols() {
    let symbols = symbols_for_path(
        Path::new("notes/readme.md"),
        "# Title\n\nfn looks_like_code() {}\n",
    );
    assert!(symbols.is_empty());
}

#[test]
fn parse_error_file_reports_no_symbols_but_keeps_its_chunks() {
    let content = "fn ok() -> u8 { 1 }\nfn broken( {\n";
    assert!(symbols_for_path(Path::new("src/broken.rs"), content).is_empty());

    let result = crate::chunk::chunk_content_for_path(Path::new("src/broken.rs"), content);
    assert!(
        !result.chunks.is_empty(),
        "a parse error must keep the file's chunks"
    );
    assert!(result.symbols.is_empty());
}

#[test]
fn chunking_result_carries_the_symbols_from_the_same_parse() {
    let content = "pub fn alpha() -> u8 { 1 }\n\npub fn beta() -> u8 { 2 }\n";
    let result = crate::chunk::chunk_content_for_path(Path::new("src/lib.rs"), content);
    assert!(result.used_tree_sitter);
    assert_eq!(
        describe(&result.symbols),
        vec!["function alpha 1-1", "function beta 3-3"]
    );
}
