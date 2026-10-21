use std::path::{Component, Path, PathBuf};

use super::lexer::{lex_code, LexMode, Token, TokenKind};
use super::paths::canonical_file_under;
use super::types::{RawReference, SourceFile};

pub(super) fn rust_references(source: &SourceFile, root: &Path) -> Vec<RawReference> {
    let tokens = lex_code(&source.content, LexMode::Rust);
    let mut references = Vec::new();
    let mut index = 0usize;
    while index < tokens.len() {
        if tokens[index].value == "mod" {
            if let Some(name) = tokens
                .get(index + 1)
                .filter(|token| token.kind == TokenKind::Ident)
            {
                let terminator = tokens[index + 2..]
                    .iter()
                    .find(|token| token.value == ";" || token.value == "{");
                if terminator.map(|token| token.value.as_str()) == Some(";") {
                    let target = resolve_rust_mod(&source.path, &name.value, root);
                    if target.is_some() {
                        references.push(RawReference {
                            kind: "module",
                            specifier: name.value.clone(),
                            line: tokens[index].line,
                            target,
                        });
                    }
                }
            }
        }

        if tokens[index].value == "use" {
            let statement = tokens[index + 1..]
                .iter()
                .take_while(|token| token.value != ";")
                .cloned()
                .collect::<Vec<_>>();
            for path_segments in rust_use_paths(&statement) {
                let specifier = path_segments.join("::");
                if let Some(target) = resolve_rust_use(&source.path, &path_segments, root) {
                    references.push(RawReference {
                        kind: "import",
                        specifier,
                        line: tokens[index].line,
                        target: Some(target),
                    });
                }
            }
        }
        index += 1;
    }
    references
}

fn rust_use_paths(tokens: &[Token]) -> Vec<Vec<String>> {
    fn collect(tokens: &[Token], prefix: &[String], output: &mut Vec<Vec<String>>) {
        let group_start = tokens.iter().position(|token| token.value == "{");
        let path_tokens = &tokens[..group_start.unwrap_or(tokens.len())];
        let mut current = prefix.to_vec();
        current.extend(rust_path_segments(path_tokens));
        let Some(open) = group_start else {
            if !current.is_empty() {
                output.push(current);
            }
            return;
        };

        let mut depth = 1usize;
        let mut close = open + 1;
        while close < tokens.len() && depth > 0 {
            match tokens[close].value.as_str() {
                "{" => depth += 1,
                "}" => depth -= 1,
                _ => {}
            }
            close += 1;
        }
        if depth != 0 {
            return;
        }
        let body = &tokens[open + 1..close - 1];
        let mut start = 0usize;
        let mut nested_depth = 0usize;
        for (index, token) in body.iter().enumerate() {
            match token.value.as_str() {
                "{" => nested_depth += 1,
                "}" => nested_depth = nested_depth.saturating_sub(1),
                "," if nested_depth == 0 => {
                    collect(&body[start..index], &current, output);
                    start = index + 1;
                }
                _ => {}
            }
        }
        if start < body.len() {
            collect(&body[start..], &current, output);
        }
    }

    let mut output = Vec::new();
    collect(tokens, &[], &mut output);
    output
}

fn rust_path_segments(tokens: &[Token]) -> Vec<String> {
    let mut segments = Vec::new();
    let mut expect_ident = true;
    for token in tokens {
        if token.kind == TokenKind::Ident && expect_ident {
            if token.value == "as" {
                break;
            }
            segments.push(token.value.clone());
            expect_ident = false;
        } else if token.value == "::" {
            expect_ident = true;
        } else if token.value == "*" || token.value == "," {
            break;
        }
    }
    segments
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
