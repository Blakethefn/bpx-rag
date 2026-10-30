use std::path::{Component, Path, PathBuf};

use super::parsers::ParserCache;
use super::paths::canonical_file_under;
use super::types::{RawReference, SourceFile};
use tree_sitter::Node;

pub(super) fn rust_references(
    source: &SourceFile,
    root: &Path,
    parsers: &mut ParserCache,
) -> Vec<RawReference> {
    let Some(tree) = parsers.parse(super::languages::Grammar::Rust, &source.content) else {
        return Vec::new();
    };
    let mut pending = vec![tree.root_node()];
    let mut references = Vec::new();
    while let Some(node) = pending.pop() {
        if node.kind() == "mod_item" && node.child_by_field_name("body").is_none() {
            if let Some(name) = node.child_by_field_name("name") {
                if let Ok(specifier) = name.utf8_text(source.content.as_bytes()) {
                    references.push(RawReference {
                        kind: "module",
                        specifier: specifier.to_string(),
                        line: node.start_position().row + 1,
                        target: resolve_rust_mod(&source.path, specifier, root),
                    });
                }
            }
        } else if node.kind() == "use_declaration" {
            if let Some(argument) = node.child_by_field_name("argument") {
                for path_segments in rust_use_paths(argument, &source.content) {
                    let specifier = path_segments.join("::");
                    references.push(RawReference {
                        kind: "import",
                        target: resolve_rust_use(&source.path, &path_segments, root),
                        specifier,
                        line: node.start_position().row + 1,
                    });
                }
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

fn rust_use_paths(argument: Node<'_>, content: &str) -> Vec<Vec<String>> {
    fn path_segments(node: Node<'_>, content: &str) -> Vec<String> {
        node.utf8_text(content.as_bytes())
            .unwrap_or_default()
            .split("::")
            .map(str::trim)
            .filter(|segment| !segment.is_empty() && *segment != "*")
            .map(|segment| segment.strip_prefix("r#").unwrap_or(segment).to_string())
            .collect()
    }

    fn collect(node: Node<'_>, prefix: &[String], content: &str, output: &mut Vec<Vec<String>>) {
        match node.kind() {
            "scoped_use_list" => {
                let mut current = prefix.to_vec();
                if let Some(path) = node.child_by_field_name("path") {
                    current.extend(path_segments(path, content));
                }
                if let Some(list) = node.child_by_field_name("list") {
                    collect(list, &current, content, output);
                }
            }
            "use_list" => {
                for index in 0..node.named_child_count() {
                    if let Some(child) = node.named_child(index as u32) {
                        collect(child, prefix, content, output);
                    }
                }
            }
            "use_as_clause" => {
                if let Some(path) = node.child_by_field_name("path") {
                    let mut current = prefix.to_vec();
                    current.extend(path_segments(path, content));
                    if !current.is_empty() {
                        output.push(current);
                    }
                }
            }
            "use_wildcard" => {
                let mut current = prefix.to_vec();
                if let Some(path) = node.named_child(0) {
                    current.extend(path_segments(path, content));
                }
                if !current.is_empty() {
                    output.push(current);
                }
            }
            _ => {
                let mut current = prefix.to_vec();
                current.extend(path_segments(node, content));
                if !current.is_empty() {
                    output.push(current);
                }
            }
        }
    }

    let mut output = Vec::new();
    collect(argument, &[], content, &mut output);
    output
}

fn resolve_rust_use(source: &Path, segments: &[String], root: &Path) -> Option<PathBuf> {
    let first = segments.first()?.as_str();
    let source_root = rust_source_root(source, root)?;
    let (base, tail) = if first == "crate" {
        (Vec::new(), segments[1..].to_vec())
    } else if first == "self" || first == "super" {
        let mut current = rust_module_path(source, &source_root)?;
        let mut index = 0usize;
        while segments.get(index).map(String::as_str) == Some("super") {
            current.pop()?;
            index += 1;
        }
        if segments.get(index).map(String::as_str) == Some("self") {
            index += 1;
        }
        (current, segments[index..].to_vec())
    } else {
        (Vec::new(), segments.to_vec())
    };
    resolve_rust_module_segments(&source_root, &base, &tail, root)
}

fn resolve_rust_module_segments(
    source_root: &Path,
    base: &[String],
    tail: &[String],
    root: &Path,
) -> Option<PathBuf> {
    for length in (1..=tail.len()).rev() {
        let segments = base.iter().chain(tail[..length].iter()).collect::<Vec<_>>();
        let mut path = source_root.to_path_buf();
        for segment in &segments {
            path.push(segment);
        }
        for candidate in [path.with_extension("rs"), path.join("mod.rs")] {
            if let Some(resolved) = canonical_file_under(&candidate, root) {
                return Some(resolved);
            }
        }
    }
    None
}

fn rust_source_root(source: &Path, root: &Path) -> Option<PathBuf> {
    for ancestor in source.parent()?.ancestors() {
        if ancestor.starts_with(root) && ancestor.join("Cargo.toml").is_file() {
            let src = ancestor.join("src");
            if src.is_dir() {
                return Some(src);
            }
        }
    }
    let src = root.join("src");
    src.is_dir().then_some(src)
}

fn rust_module_path(source: &Path, source_root: &Path) -> Option<Vec<String>> {
    let relative = source.strip_prefix(source_root).ok()?;
    let file_name = relative.file_name()?.to_str()?;
    let mut segments = relative
        .parent()
        .into_iter()
        .flat_map(Path::components)
        .filter_map(|component| match component {
            Component::Normal(name) => name.to_str().map(str::to_string),
            _ => None,
        })
        .collect::<Vec<_>>();
    if file_name != "lib.rs" && file_name != "main.rs" && file_name != "mod.rs" {
        segments.push(Path::new(file_name).file_stem()?.to_str()?.to_string());
    }
    Some(segments)
}

fn resolve_rust_mod(source: &Path, module: &str, root: &Path) -> Option<PathBuf> {
    let parent = source.parent()?;
    let stem = source.file_stem()?.to_str()?;
    let mut bases = vec![parent.to_path_buf()];
    if stem != "lib" && stem != "main" && stem != "mod" {
        bases.insert(0, parent.join(stem));
    }
    bases
        .iter()
        .flat_map(|base| {
            [
                base.join(format!("{module}.rs")),
                base.join(module).join("mod.rs"),
            ]
        })
        .filter_map(|candidate| canonical_file_under(&candidate, root))
        .next()
}
