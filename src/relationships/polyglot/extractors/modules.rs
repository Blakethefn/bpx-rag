use tree_sitter::Node;

use super::super::super::super::languages::Grammar;
use super::quoted_prefix;

pub(super) fn extract(
    grammar: Grammar,
    node: Node<'_>,
    content: &str,
) -> Vec<(&'static str, String)> {
    let Some(text) = node.utf8_text(content.as_bytes()).ok().map(str::trim) else {
        return Vec::new();
    };
    let reference = match grammar {
        Grammar::Go if node.kind() == "import_spec" => {
            quoted_prefix(text).map(|specifier| ("import", specifier))
        }
        Grammar::Java if node.kind() == "import_declaration" => {
            let Some(import) = text.strip_prefix("import").map(str::trim) else {
                return Vec::new();
            };
            let (import, is_static) = match import.strip_prefix("static") {
                Some(import) => (import.trim(), true),
                None => (import, false),
            };
            let mut path = import.trim_end_matches(';').trim();
            if is_static && !path.ends_with(".*") {
                let Some(class_path) = path.rsplit_once('.').map(|(path, _)| path) else {
                    return Vec::new();
                };
                path = class_path;
            }
            (!path.is_empty()).then(|| ("import", path.to_string()))
        }
        Grammar::CSharp if node.kind() == "using_directive" => {
            let using = text.strip_prefix("global").unwrap_or(text).trim();
            let Some(using) = using.strip_prefix("using").map(str::trim) else {
                return Vec::new();
            };
            let path = using
                .strip_prefix("static")
                .unwrap_or(using)
                .trim()
                .trim_end_matches(';')
                .trim();
            (!path.contains('=') && !path.is_empty()).then(|| ("import", path.to_string()))
        }
        Grammar::Kotlin if node.kind() == "import" => {
            let Some(path) = text.strip_prefix("import").map(str::trim) else {
                return Vec::new();
            };
            let path = path
                .split_once(" as ")
                .map_or(path, |(path, _)| path)
                .trim();
            let path = path.trim_end_matches(".*");
            (!path.is_empty()).then(|| ("import", path.to_string()))
        }
        _ => None,
    };
    reference.into_iter().collect()
}
