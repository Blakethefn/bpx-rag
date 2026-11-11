use super::*;

#[test]
fn a_test_attribute_names_each_function() {
    let content = "#[test]\nfn checks_a() {}\n#[test]\nfn checks_b() {}\n";
    let result = chunk_content_for_path(Path::new("src/lib.rs"), content);
    let mut reasons: Vec<&str> = result
        .entries
        .iter()
        .filter(|entry| entry.kind == ENTRY_TEST)
        .map(|entry| entry.reason.as_str())
        .collect();
    reasons.sort();
    assert_eq!(reasons, vec!["#[test] checks_a", "#[test] checks_b"]);
}

#[test]
fn a_cfg_test_attribute_is_not_a_test_entry() {
    let content = "#[cfg(test)]\nfn helper() {}\n";
    let result = chunk_content_for_path(Path::new("src/lib.rs"), content);
    assert!(
        result.entries.iter().all(|entry| entry.kind != ENTRY_TEST),
        "{:?}",
        result.entries
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
