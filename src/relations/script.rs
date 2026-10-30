use std::path::{Path, PathBuf};

use super::languages::grammar_for_path;
use super::parsers::ParserCache;
use super::paths::canonical_file_under;
use super::types::{RawReference, SourceFile};
use tree_sitter::Node;

pub(super) fn script_references(
    source: &SourceFile,
    root: &Path,
    parsers: &mut ParserCache,
) -> Vec<RawReference> {
    let Some(grammar) = grammar_for_path(&source.path) else {
        return Vec::new();
    };
    let Some(tree) = parsers.parse(grammar, &source.content) else {
        return Vec::new();
    };
    let mut pending = vec![tree.root_node()];
    let mut references = Vec::new();
    while let Some(node) = pending.pop() {
        if let Some((kind, specifier)) = script_import(node, &source.content) {
            if is_local_specifier(&specifier) {
                references.push(RawReference {
                    kind,
                    target: resolve_script_import(&source.path, root, &specifier),
                    specifier,
                    line: node.start_position().row + 1,
                });
            }
        }
        for index in (0..node.named_child_count() as u32).rev() {
            if let Some(child) = node.named_child(index) {
                pending.push(child);
            }
        }
    }
    references
}

fn script_import(node: Node<'_>, content: &str) -> Option<(&'static str, String)> {
    match node.kind() {
        "import_statement" => {
            let source = node.child_by_field_name("source")?;
            string_value(source, content).map(|specifier| ("import", specifier))
        }
        "export_statement" => {
            let source = node.child_by_field_name("source")?;
            string_value(source, content).map(|specifier| ("re-export", specifier))
        }
        "call_expression" => {
            let function = node.child_by_field_name("function")?;
            let function_name = function.utf8_text(content.as_bytes()).ok()?;
            let (kind, is_dynamic_import) = match function_name {
                "import" => ("import", true),
                "require" => ("import", false),
                _ => return None,
            };
            let arguments = node.child_by_field_name("arguments")?;
            let argument = arguments.named_child(0)?;
            if !is_dynamic_import && argument.kind() != "string" {
                return None;
            }
            string_value(argument, content).map(|specifier| (kind, specifier))
        }
        _ => None,
    }
}

fn string_value(node: Node<'_>, content: &str) -> Option<String> {
    let text = node.utf8_text(content.as_bytes()).ok()?.trim();
    let quote = text.chars().next()?;
    if !matches!(quote, '\'' | '"' | '`') || !text.ends_with(quote) {
        return None;
    }
    Some(text[quote.len_utf8()..text.len() - quote.len_utf8()].to_string())
}

fn is_local_specifier(specifier: &str) -> bool {
    specifier.starts_with('.') || specifier.starts_with('/')
}

fn resolve_script_import(source: &Path, root: &Path, specifier: &str) -> Option<PathBuf> {
    let spec_path = Path::new(specifier);
    let base = if spec_path.is_absolute() {
        root.join(spec_path.strip_prefix("/").ok()?)
    } else {
        source.parent()?.join(spec_path)
    };
    let mut candidates = vec![base.clone()];
    if let Some(extension) = base.extension().and_then(|extension| extension.to_str()) {
        if extension == "js" || extension == "jsx" || extension == "mjs" || extension == "cjs" {
            for replacement in ["ts", "tsx", "js", "jsx", "mjs", "cjs"] {
                candidates.push(base.with_extension(replacement));
            }
        }
    } else {
        for extension in ["ts", "tsx", "js", "jsx", "mjs", "cjs", "json"] {
            candidates.push(base.with_extension(extension));
        }
        for index in ["ts", "tsx", "js", "jsx", "mjs", "cjs", "json"] {
            candidates.push(base.join(format!("index.{index}")));
        }
    }
    candidates
        .iter()
        .filter_map(|candidate| canonical_file_under(candidate, root))
        .next()
}
