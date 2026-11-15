pub(super) fn imports(text: &str) -> Vec<String> {
    let Some(import) = text.strip_prefix("import").map(str::trim) else {
        return Vec::new();
    };
    let import = import.trim_end_matches(';').trim();
    let Some((package, selectors)) = import.split_once('{') else {
        let path = import
            .split_once(" as ")
            .or_else(|| import.split_once(" => "))
            .map_or(import, |(path, _)| path)
            .trim();
        if path.is_empty() {
            return Vec::new();
        }
        let wildcard = path.ends_with("._") || path.ends_with(".*");
        let path = path.trim_end_matches("._").trim_end_matches(".*");
        return vec![if wildcard {
            format!("{}/*", path.trim_end_matches('.'))
        } else {
            path.to_string()
        }];
    };

    let package = package.trim().trim_end_matches('.');
    if package.is_empty() {
        return Vec::new();
    }
    selectors
        .trim_end_matches('}')
        .split(',')
        .filter_map(|selector| {
            let selector = selector.trim();
            let selector = selector
                .split_once(" as ")
                .or_else(|| selector.split_once(" => "))
                .map_or(selector, |(name, _)| name)
                .trim();
            if selector.is_empty() {
                None
            } else if selector == "_" || selector == "*" {
                Some(format!("{package}/*"))
            } else {
                Some(format!("{package}.{selector}"))
            }
        })
        .collect()
}
