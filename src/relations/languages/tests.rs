use std::path::Path;

use super::{
    candidate_extensions, extensions_for, grammar_for_path, is_resolution_extra, Grammar,
    LANGUAGE_EXTENSIONS, RESOLVED_ELSEWHERE,
};

#[test]
fn candidates_include_every_detection_extension() {
    for (extensions, grammar) in LANGUAGE_EXTENSIONS {
        let candidates = candidate_extensions(*grammar);
        if RESOLVED_ELSEWHERE.contains(grammar) {
            assert!(candidates.is_empty(), "{grammar:?}");
            continue;
        }
        for extension in *extensions {
            assert!(
                candidates.contains(extension),
                "{grammar:?} drops detection extension {extension}"
            );
        }
    }
}

#[test]
fn plain_languages_reuse_their_detection_row() {
    for grammar in [
        Grammar::Python,
        Grammar::Go,
        Grammar::Java,
        Grammar::Kotlin,
        Grammar::Scala,
        Grammar::C,
    ] {
        assert_eq!(
            candidate_extensions(grammar),
            extensions_for(grammar),
            "{grammar:?}"
        );
    }
}

#[test]
fn script_family_shares_one_candidate_list() {
    let javascript = candidate_extensions(Grammar::JavaScript);
    assert_eq!(javascript, candidate_extensions(Grammar::TypeScript));
    assert_eq!(javascript, candidate_extensions(Grammar::Tsx));
    assert_eq!(javascript, ["ts", "tsx", "js", "jsx", "mjs", "cjs", "json"]);
    for grammar in [Grammar::JavaScript, Grammar::TypeScript, Grammar::Tsx] {
        for extension in extensions_for(grammar) {
            assert!(javascript.contains(extension), "{extension}");
        }
    }
    assert!(is_resolution_extra(Grammar::JavaScript, "json"));
    assert!(!is_resolution_extra(Grammar::JavaScript, "ts"));
    assert!(!is_resolution_extra(Grammar::JavaScript, "mjs"));
}

#[test]
fn resolution_only_suffixes_stay_out_of_detection() {
    for extension in ["json", "svg", "scss", "sass", "less"] {
        let path = Path::new("file").with_extension(extension);
        assert_eq!(grammar_for_path(&path), None, "{extension}");
    }
    assert_eq!(grammar_for_path(Path::new("file.h")), Some(Grammar::C));
    assert_eq!(
        candidate_extensions(Grammar::Cpp),
        ["cc", "hh", "cpp", "hpp", "cxx", "hxx", "h"]
    );
    assert!(!candidate_extensions(Grammar::Cpp).contains(&"c"));
    assert!(is_resolution_extra(Grammar::Cpp, "h"));
    assert!(!is_resolution_extra(Grammar::C, "h"));
    assert_eq!(candidate_extensions(Grammar::R), ["r", "R"]);
    assert_eq!(
        candidate_extensions(Grammar::Html),
        ["html", "htm", "js", "jsx", "ts", "tsx", "css", "json", "svg"]
    );
    assert_eq!(
        candidate_extensions(Grammar::Css),
        ["css", "scss", "sass", "less"]
    );
    assert!(is_resolution_extra(Grammar::Css, "scss"));
    assert!(is_resolution_extra(Grammar::Html, "svg"));
    assert!(!is_resolution_extra(Grammar::Html, "html"));
    assert_eq!(candidate_extensions(Grammar::Markdown), [] as [&str; 0]);
    assert_eq!(candidate_extensions(Grammar::Rust), [] as [&str; 0]);
}
