pub(crate) fn file_extension(path: &str) -> String {
    std::path::Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default()
}

pub(super) fn validate_relative_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.chars().any(char::is_control)
        && !path
            .split('/')
            .any(|segment| segment.is_empty() || segment == "." || segment == "..")
        && !path
            .split('/')
            .next()
            .is_some_and(|segment| segment.ends_with(':'))
}

pub(super) fn predicate_sql(
    path: &str,
    directory: bool,
    rel_path_column: &str,
    param_index: usize,
) -> (String, Vec<String>) {
    if directory {
        (
            format!(
                "{rel_path_column} >= ?{param_index} AND {rel_path_column} < ?{}",
                param_index + 1
            ),
            vec![format!("{path}/"), format!("{path}0")],
        )
    } else {
        (
            format!("{rel_path_column} = ?{param_index}"),
            vec![path.to_string()],
        )
    }
}
