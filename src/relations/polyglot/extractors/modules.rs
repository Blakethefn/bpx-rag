use tree_sitter::Node;

use super::{has_kind, quoted_prefix};

pub(super) fn extract(node: Node<'_>, content: &str) -> Vec<(&'static str, String)> {
    let Some(text) = node.utf8_text(content.as_bytes()).ok().map(str::trim) else {
        return Vec::new();
    };
    match node.kind() {
        "import_spec" => quoted_prefix(text)
            .map(|specifier| ("import", specifier))
            .into_iter()
            .collect(),
        "import_declaration" => import_declaration(node, text),
        "using_directive" => csharp_using(node, text).into_iter().collect(),
        "import" => {
            let reference = if node.child_by_field_name("module").is_some() {
                haskell_import(node, content)
            } else {
                kotlin_import(text)
            };
            reference.into_iter().collect()
        }
        "import_attribute" => erlang_import(node, content).into_iter().collect(),
        "pp_include" | "pp_include_lib" => quoted_prefix(text)
            .map(|path| ("include", path))
            .into_iter()
            .collect(),
        "import_directive" => solidity_import(node, content).into_iter().collect(),
        _ => Vec::new(),
    }
}

fn import_declaration(node: Node<'_>, text: &str) -> Vec<(&'static str, String)> {
    if has_kind(node, "import_spec") || has_kind(node, "import_spec_list") {
        return Vec::new();
    }
    if node.child_by_field_name("path").is_some()
        || has_kind(node, "as_renamed_identifier")
        || has_kind(node, "namespace_selectors")
        || has_kind(node, "namespace_wildcard")
    {
        return scala_imports(text)
            .into_iter()
            .map(|path| ("import", path))
            .collect();
    }
    java_import(text).into_iter().collect()
}

fn java_import(text: &str) -> Option<(&'static str, String)> {
    let import = text.strip_prefix("import").map(str::trim)?;
    let (import, is_static) = match import.strip_prefix("static") {
        Some(import) => (import.trim(), true),
        None => (import, false),
    };
    let mut path = import.trim_end_matches(';').trim();
    if is_static && !path.ends_with(".*") {
        path = path.rsplit_once('.').map(|(class_path, _)| class_path)?;
    }
    (!path.is_empty()).then(|| ("import", path.to_string()))
}

fn csharp_using(node: Node<'_>, text: &str) -> Option<(&'static str, String)> {
    if node.child_by_field_name("source").is_some() || has_kind(node, "using_directive_key") {
        return None;
    }
    let using = text.strip_prefix("global").unwrap_or(text).trim();
    let using = using.strip_prefix("using").map(str::trim)?;
    let path = using
        .strip_prefix("static")
        .unwrap_or(using)
        .trim()
        .trim_end_matches(';')
        .trim();
    (!path.contains('=') && !path.is_empty()).then(|| ("import", path.to_string()))
}

fn kotlin_import(text: &str) -> Option<(&'static str, String)> {
    let path = text.strip_prefix("import").map(str::trim)?;
    let path = path
        .split_once(" as ")
        .map_or(path, |(path, _)| path)
        .trim();
    let path = path.trim_end_matches(".*");
    (!path.is_empty()).then(|| ("import", path.to_string()))
}

fn haskell_import(node: Node<'_>, content: &str) -> Option<(&'static str, String)> {
    node.child_by_field_name("module")
        .and_then(|module| module.utf8_text(content.as_bytes()).ok())
        .map(str::trim)
        .filter(|module| !module.is_empty())
        .map(|module| ("import", module.to_string()))
}

fn erlang_import(node: Node<'_>, content: &str) -> Option<(&'static str, String)> {
    node.child_by_field_name("module")
        .and_then(|module| module.utf8_text(content.as_bytes()).ok())
        .map(str::trim)
        .map(|module| module.trim_matches(['\'', '"']))
        .filter(|module| !module.is_empty())
        .map(|module| ("import", module.to_string()))
}

fn solidity_import(node: Node<'_>, content: &str) -> Option<(&'static str, String)> {
    node.child_by_field_name("source")
        .and_then(|source| source.utf8_text(content.as_bytes()).ok())
        .and_then(quoted_prefix)
        .map(|path| ("import", path))
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
