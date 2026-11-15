use super::super::super::*;

#[test]
fn parse_directory_and_file_paths() {
    assert_eq!(
        parse_filter(Some("dir:src/lib file:src/lib/main.rs")).unwrap(),
        vec![
            FilterPredicate::Directory("src/lib".to_string()),
            FilterPredicate::File("src/lib/main.rs".to_string()),
        ]
    );
    assert_eq!(
        parse_filter(Some("directory:src/lib")).unwrap(),
        vec![FilterPredicate::Directory("src/lib".to_string())]
    );
}

#[test]
fn parse_rejects_non_relative_or_unnormalized_paths() {
    for filter in [
        "dir:/src/lib",
        "dir:C:/src/lib",
        "dir:src//lib",
        "dir:src/./lib",
        "dir:src/../lib",
        "dir:src\\lib",
        "file:src/",
        "file:.",
        "file:..",
    ] {
        assert!(parse_filter(Some(filter)).is_err(), "accepted {filter}");
    }
}

#[test]
fn parse_supports_quoted_path_values_with_spaces() {
    assert_eq!(
        parse_filter(Some(
            "file:\"docs/user guide.md\" dir:\"docs/source files\""
        ))
        .unwrap(),
        vec![
            FilterPredicate::File("docs/user guide.md".to_string()),
            FilterPredicate::Directory("docs/source files".to_string()),
        ]
    );
}

#[test]
fn parse_rejects_unterminated_quoted_path_values() {
    assert!(parse_filter(Some("file:\"docs/user guide.md")).is_err());
}

#[test]
fn parse_keeps_wildcard_characters_as_literal_path_data() {
    assert_eq!(
        parse_filter(Some("file:src/%_*.rs")).unwrap(),
        vec![FilterPredicate::File("src/%_*.rs".to_string())]
    );
}
