use tree_sitter::Node;

use super::super::super::super::languages::Grammar;
use super::quoted_literal;

pub(super) fn extract(
    _grammar: Grammar,
    node: Node<'_>,
    content: &str,
) -> Vec<(&'static str, String)> {
    if node.kind() != "preproc_include" {
        return Vec::new();
    }
    let Some(text) = node.utf8_text(content.as_bytes()).ok() else {
        return Vec::new();
    };
    let Some(remainder) = text.trim().strip_prefix("#include") else {
        return Vec::new();
    };
    quoted_literal(remainder)
        .map(|specifier| vec![("include", specifier)])
        .unwrap_or_default()
}
