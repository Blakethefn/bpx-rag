use std::path::Path;

pub(in crate::relationships::polyglot::resolve) fn module_path(
    source: &Path,
    specifier: &str,
) -> Option<String> {
    let dots = specifier
        .chars()
        .take_while(|character| *character == '.')
        .count();
    let remainder = specifier[dots..].replace('.', "/");
    if dots == 0 {
        return Some(remainder);
    }
    let mut base = source.parent()?.to_path_buf();
    for _ in 1..dots {
        base.pop();
    }
    Some(base.join(remainder).to_string_lossy().to_string())
}
