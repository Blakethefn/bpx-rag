use super::*;
use std::path::Path;

/// One line per symbol: `kind container::name start-end`, or `kind name start-end` without a container.
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
fn rust_symbols_cover_types_modules_functions_methods_and_consts() {
    let content = "\
pub struct Store { id: u32 }
pub enum Mode { Fast, Slow }
pub trait Runner {
    fn run(&self);
}
mod helpers {
    pub fn assist() -> u8 { 1 }
}
impl Store {
    pub fn open(id: u32) -> Self { Store { id } }
}
const LIMIT: u32 = 4;
";
    let symbols = symbols_for_path(Path::new("src/lib.rs"), content);
    assert_eq!(
        describe(&symbols),
        vec![
            "struct Store 1-1",
            "enum Mode 2-2",
            "trait Runner 3-5",
            "mod helpers 6-8",
            "function helpers::assist 7-7",
            "function Store::open 10-10",
            "const LIMIT 12-12",
        ]
    );
}

#[test]
fn python_symbols_resolve_decorators_and_methods() {
    let content = "\
import os

class Cache:
    def get(self, key):
        return key

@decorator
def load(path):
    return path
";
    let symbols = symbols_for_path(Path::new("app/cache.py"), content);
    assert_eq!(
        describe(&symbols),
        vec![
            "class Cache 3-5",
            "function Cache::get 4-5",
            "function load 8-9",
        ]
    );
}

#[test]
fn typescript_symbols_resolve_exports_arrow_functions_and_methods() {
    let content = "\
export interface Config {
  debug: boolean;
}

export function build(config: Config): string {
  return \"\";
}

const parse = (text: string) => text.trim();

class Service {
  run(): void {}
}
";
    let symbols = symbols_for_path(Path::new("web/service.ts"), content);
    assert_eq!(
        describe(&symbols),
        vec![
            "interface Config 1-3",
            "function build 5-7",
            "variable parse 9-9",
            "class Service 11-13",
            "method Service::run 12-12",
        ]
    );
}

#[test]
fn go_symbols_cover_type_specs_functions_and_methods() {
    let content = "\
package store

type Entry struct {
	Key string
}

type Reader interface {
	Read() string
}

type Alias = string

func Open(path string) (*Entry, error) {
	return nil, nil
}

func (e *Entry) Close() error {
	return nil
}
";
    let symbols = symbols_for_path(Path::new("store/store.go"), content);
    assert_eq!(
        describe(&symbols),
        vec![
            "struct Entry 3-5",
            "interface Reader 7-9",
            "type Alias 11-11",
            "function Open 13-15",
            "method Close 17-19",
        ]
    );
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
