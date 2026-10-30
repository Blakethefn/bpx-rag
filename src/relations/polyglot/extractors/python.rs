use std::path::Path;

use tree_sitter::Node;

use super::super::super::super::languages::Grammar;
use super::super::super::super::metadata::ProjectMetadata;
use super::super::super::super::types::SourceFile;
use super::super::super::resolve::resolve_specifier;

pub(super) fn extract(
    node: Node<'_>,
    source: &SourceFile,
    root: &Path,
    files: &[SourceFile],
    metadata: &ProjectMetadata,
) -> Vec<(&'static str, String)> {
    let content = &source.content;
    if node.kind() == "import_statement" {
        let Some(text) = node.utf8_text(content.as_bytes()).ok() else {
            return Vec::new();
        };
        return text
            .trim()
            .strip_prefix("import")
            .into_iter()
            .flat_map(|names| names.split(','))
            .filter_map(|name| {
                let name = name.trim().split_whitespace().next().unwrap_or_default();
                (!name.is_empty()).then(|| ("import", name.to_string()))
            })
            .collect();
    }
    if node.kind() != "import_from_statement" {
        return Vec::new();
    }
    let Some(module) = node.child_by_field_name("module_name") else {
        return Vec::new();
    };
    let Some(module_text) = module.utf8_text(content.as_bytes()).ok().map(str::trim) else {
        return Vec::new();
    };
    let mut imports = vec![("import", module_text.to_string())];
    let Ok(child_count) = u32::try_from(node.child_count()) else {
        return imports;
    };
    for child_index in 0..child_count {
        if node.field_name_for_child(child_index) != Some("name") {
            continue;
        }
        let Some(name_node) = node.child(child_index) else {
            continue;
        };
        let Ok(name_text) = name_node.utf8_text(content.as_bytes()) else {
            continue;
        };
        let name = name_text
            .trim()
            .split_whitespace()
            .next()
            .unwrap_or_default();
        if name.is_empty() || name == "*" {
            continue;
        }
        let candidate = if module_text.is_empty() {
            name.to_string()
        } else if module_text.ends_with('.') {
            format!("{module_text}{name}")
        } else {
            format!("{module_text}.{name}")
        };
        if !resolve_specifier(
            Grammar::Python,
            &source.path,
            root,
            &candidate,
            files,
            metadata,
        )
        .is_empty()
        {
            imports.push(("import", candidate));
        }
    }
    imports
}
