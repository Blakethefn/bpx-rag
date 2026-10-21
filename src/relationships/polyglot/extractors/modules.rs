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
    if grammar == Grammar::Scala && node.kind() == "import_declaration" {
        return scala_imports(text)
            .into_iter()
            .map(|path| ("import", path))
            .collect();
    }
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
        Grammar::Haskell if node.kind() == "import" => node
            .child_by_field_name("module")
            .and_then(|module| module.utf8_text(content.as_bytes()).ok())
            .map(str::trim)
            .filter(|module| !module.is_empty())
            .map(|module| ("import", module.to_string())),
        Grammar::Erlang if node.kind() == "import_attribute" => node
            .child_by_field_name("module")
            .and_then(|module| module.utf8_text(content.as_bytes()).ok())
            .map(str::trim)
            .map(|module| module.trim_matches(['\'', '"']))
            .filter(|module| !module.is_empty())
            .map(|module| ("import", module.to_string())),
        Grammar::Erlang if matches!(node.kind(), "pp_include" | "pp_include_lib") => {
            quoted_prefix(text).map(|path| ("include", path))
        }
        Grammar::Solidity if node.kind() == "import_directive" => node
            .child_by_field_name("source")
            .and_then(|source| source.utf8_text(content.as_bytes()).ok())
            .and_then(quoted_prefix)
            .map(|path| ("import", path)),
        _ => None,
    };
    reference.into_iter().collect()
}

fn scala_imports(text: &str) -> Vec<String> {
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
