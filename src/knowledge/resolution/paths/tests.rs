use super::{is_directory_module_stem, is_path_keyword, rust_candidates, PATH_KEYWORDS};

#[test]
fn path_keywords_are_not_module_file_candidates() {
    assert!(!PATH_KEYWORDS.is_empty());
    for (name, _) in PATH_KEYWORDS {
        assert!(is_path_keyword(name), "{name}");
        assert!(
            rust_candidates("src/lib.rs", name).is_empty(),
            "{name} from a directory module"
        );
        assert!(
            rust_candidates("src/foo.rs", &format!("outer::{name}")).is_empty(),
            "{name} as the last segment"
        );
    }
    assert!(!is_path_keyword("foo"));
    assert!(!is_path_keyword("Self"));
    assert_eq!(
        rust_candidates("src/lib.rs", "crate::child"),
        vec!["src/child.rs".to_string(), "src/child/mod.rs".to_string()]
    );
}

#[test]
fn directory_module_stems_keep_child_candidates_beside_the_file() {
    for stem in ["lib", "main", "mod"] {
        assert!(is_directory_module_stem(stem), "{stem}");
        let from = format!("src/{stem}.rs");
        assert_eq!(
            rust_candidates(&from, "child"),
            vec!["src/child.rs".to_string(), "src/child/mod.rs".to_string()]
        );
    }
    assert!(!is_directory_module_stem("foo"));
    assert!(!is_directory_module_stem("lib.rs"));
    assert_eq!(
        rust_candidates("src/foo.rs", "child"),
        vec![
            "src/foo/child.rs".to_string(),
            "src/foo/child/mod.rs".to_string(),
            "src/child.rs".to_string(),
            "src/child/mod.rs".to_string(),
        ]
    );
    assert_eq!(
        rust_candidates("src/bar/mod.rs", "child"),
        vec![
            "src/bar/child.rs".to_string(),
            "src/bar/child/mod.rs".to_string(),
        ]
    );
}
