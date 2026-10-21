use std::path::{Path, PathBuf};

use tree_sitter::{Node, Parser};

use super::lexer::{lex_code, LexMode, TokenKind};
use super::paths::canonical_file_under;
use super::types::{RawReference, SourceFile};

#[derive(Clone, Copy)]
enum Grammar {
    C,
    Cpp,
    CSharp,
    Go,
    Java,
    Python,
    Php,
    Ruby,
    Bash,
    Lua,
    Dart,
    Swift,
    Kotlin,
}

pub(super) fn polyglot_references(
    source: &SourceFile,
    root: &Path,
    files: &[SourceFile],
) -> Vec<RawReference> {
    let Some(grammar) = grammar_for_path(&source.path) else {
        return Vec::new();
    };
    if matches!(grammar, Grammar::Kotlin) {
        return kotlin_references(source, root, files);
    }
    let Some(tree) = parse(grammar, &source.content) else {
        return Vec::new();
    };

    let mut pending = vec![tree.root_node()];
    let mut references = Vec::new();
    while let Some(node) = pending.pop() {
        for (kind, specifier) in extract_specifiers(grammar, node, &source.content) {
            for target in resolve_specifier(grammar, &source.path, root, &specifier, files) {
                references.push(RawReference {
                    kind,
                    specifier: specifier.clone(),
                    line: node.start_position().row + 1,
                    target: Some(target),
                });
            }
        }
        for index in (0..node.child_count()).rev() {
            if let Some(child) = node.child(index) {
                pending.push(child);
            }
        }
    }
    references
}

fn grammar_for_path(path: &Path) -> Option<Grammar> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    match extension.as_str() {
        "c" | "h" => Some(Grammar::C),
        "cc" | "hh" | "cpp" | "hpp" | "cxx" | "hxx" => Some(Grammar::Cpp),
        "cs" => Some(Grammar::CSharp),
        "go" => Some(Grammar::Go),
        "java" => Some(Grammar::Java),
        "py" | "pyi" => Some(Grammar::Python),
        "php" | "phtml" => Some(Grammar::Php),
        "rb" | "rake" => Some(Grammar::Ruby),
        "sh" | "bash" => Some(Grammar::Bash),
        "lua" => Some(Grammar::Lua),
        "dart" => Some(Grammar::Dart),
        "swift" => Some(Grammar::Swift),
        "kt" | "kts" => Some(Grammar::Kotlin),
        _ => None,
    }
}

fn parse(grammar: Grammar, content: &str) -> Option<tree_sitter::Tree> {
    let mut parser = Parser::new();
    let language = match grammar {
        Grammar::C => tree_sitter_c::LANGUAGE.into(),
        Grammar::Cpp => tree_sitter_cpp::LANGUAGE.into(),
        Grammar::CSharp => tree_sitter_c_sharp::LANGUAGE.into(),
        Grammar::Go => tree_sitter_go::LANGUAGE.into(),
        Grammar::Java => tree_sitter_java::LANGUAGE.into(),
        Grammar::Python => tree_sitter_python::LANGUAGE.into(),
        Grammar::Php => tree_sitter_php::LANGUAGE_PHP.into(),
        Grammar::Ruby => tree_sitter_ruby::LANGUAGE.into(),
        Grammar::Bash => tree_sitter_bash::LANGUAGE.into(),
        Grammar::Lua => tree_sitter_lua::LANGUAGE.into(),
        Grammar::Dart => tree_sitter_dart::LANGUAGE.into(),
        Grammar::Swift => tree_sitter_swift::LANGUAGE.into(),
        Grammar::Kotlin => return None,
    };
    parser.set_language(&language).ok()?;
    parser.parse(content, None)
}

fn kotlin_references(source: &SourceFile, root: &Path, files: &[SourceFile]) -> Vec<RawReference> {
    let tokens = lex_code(&source.content, LexMode::Script);
    let mut references = Vec::new();
    let mut index = 0usize;
    while index < tokens.len() {
        if tokens[index].kind != TokenKind::Ident || tokens[index].value != "import" {
            index += 1;
            continue;
        }
        let line = tokens[index].line;
        let mut parts = Vec::new();
        let mut cursor = index + 1;
        while let Some(token) = tokens.get(cursor).filter(|token| token.line == line) {
            if token.kind == TokenKind::Ident && token.value == "as" {
                break;
            }
            if token.kind == TokenKind::Ident || token.value == "." || token.value == "*" {
                parts.push(token.value.as_str());
                cursor += 1;
            } else {
                break;
            }
        }
        let specifier = parts.join("").trim_end_matches(".*").to_string();
        if !specifier.is_empty() {
            references.push((line, specifier));
        }
        index += 1;
    }

    references
        .into_iter()
        .flat_map(|(line, specifier)| {
            resolve_specifier(Grammar::Kotlin, &source.path, root, &specifier, files)
                .into_iter()
                .map(move |target| RawReference {
                    kind: "import",
                    specifier: specifier.clone(),
                    line,
                    target: Some(target),
                })
        })
        .collect()
}

fn extract_specifiers(
    grammar: Grammar,
    node: Node<'_>,
    content: &str,
) -> Vec<(&'static str, String)> {
    if matches!(grammar, Grammar::Python) && node.kind() == "import_statement" {
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
    extract_specifier(grammar, node, content)
        .into_iter()
        .collect()
}

fn extract_specifier(
    grammar: Grammar,
    node: Node<'_>,
    content: &str,
) -> Option<(&'static str, String)> {
    let kind = node.kind();
    let text = node.utf8_text(content.as_bytes()).ok()?.trim();
    match grammar {
        Grammar::C | Grammar::Cpp if kind == "preproc_include" => {
            let remainder = text.strip_prefix("#include")?.trim();
            Some(("include", quoted_value(remainder)?))
        }
        Grammar::Python if kind == "import_statement" => {
            let names = text.strip_prefix("import")?.split(',');
            names
                .map(|name| name.trim().split_whitespace().next().unwrap_or_default())
                .find(|name| !name.is_empty())
                .map(|name| ("import", name.to_string()))
        }
        Grammar::Python if kind == "import_from_statement" => {
            let module = node.child_by_field_name("module_name")?;
            let specifier = module.utf8_text(content.as_bytes()).ok()?.trim();
            (!specifier.is_empty()).then(|| ("import", specifier.to_string()))
        }
        Grammar::Go if kind == "import_spec" => {
            let (specifier, _) = quoted_prefix(text)?;
            Some(("import", specifier))
        }
        Grammar::Java if kind == "import_declaration" => {
            let import = text.strip_prefix("import")?.trim();
            let (import, is_static) = match import.strip_prefix("static") {
                Some(import) => (import.trim(), true),
                None => (import, false),
            };
            let mut path = import.trim_end_matches(';').trim();
            if is_static && !path.ends_with(".*") {
                path = path.rsplit_once('.')?.0;
            }
            (!path.is_empty()).then(|| ("import", path.to_string()))
        }
        Grammar::CSharp if kind == "using_directive" => {
            let using = text.strip_prefix("global").unwrap_or(text).trim();
            let path = using
                .strip_prefix("using")?
                .trim()
                .strip_prefix("static")
                .unwrap_or(using.strip_prefix("using")?.trim())
                .trim()
                .trim_end_matches(';')
                .trim();
            if path.contains('=') || path.is_empty() {
                None
            } else {
                Some(("import", path.to_string()))
            }
        }
        Grammar::Dart if kind == "import_specification" || kind == "library_export" => {
            let uri = node.child_by_field_name("uri")?;
            let uri_text = uri.utf8_text(content.as_bytes()).ok()?.trim();
            let edge_kind = if kind == "library_export" {
                "re-export"
            } else {
                "import"
            };
            Some((edge_kind, quoted_value(uri_text)?))
        }
        Grammar::Swift if kind == "import_declaration" => {
            let path = text.strip_prefix("import")?.trim();
            let path = path.split_whitespace().last()?;
            Some(("import", path.to_string()))
        }
        Grammar::Php
            if matches!(
                kind,
                "include_expression"
                    | "require_expression"
                    | "include_once_expression"
                    | "require_once_expression"
            ) =>
        {
            let quote_offset = text.find(['\'', '"'])?;
            quoted_value(&text[quote_offset..]).map(|specifier| ("include", specifier))
        }
        Grammar::Ruby
            if kind == "call"
                && (text.starts_with("require ")
                    || text.starts_with("require(")
                    || text.starts_with("require_relative ")
                    || text.starts_with("require_relative(")) =>
        {
            let is_relative = text.starts_with("require_relative");
            let specifier = quoted_prefix(text)?.0;
            let specifier =
                if is_relative && !specifier.starts_with('.') && !specifier.starts_with('/') {
                    format!("./{specifier}")
                } else {
                    specifier
                };
            Some(("import", specifier))
        }
        Grammar::Bash if kind == "command" => {
            let command = text.split_whitespace().collect::<Vec<_>>();
            let (kind, operand) = match command.as_slice() {
                ["source", operand, ..] | [".", operand, ..] => ("include", *operand),
                _ => return None,
            };
            let specifier = quoted_value(operand).unwrap_or_else(|| operand.to_string());
            Some((kind, specifier))
        }
        Grammar::Lua if kind == "function_call" && text.starts_with("require") => {
            let argument = text
                .strip_prefix("require")?
                .trim()
                .trim_start_matches('(')
                .trim();
            let specifier = quoted_value(argument)?;
            Some(("import", specifier))
        }
        _ => None,
    }
}

fn quoted_prefix(value: &str) -> Option<(String, &str)> {
    let quote = value.find(['\'', '"', '`'])?;
    let rest = &value[quote..];
    let specifier = if rest.starts_with('`') {
        rest[1..]
            .find('`')
            .map(|end| rest[1..end + 1].to_string())?
    } else {
        quoted_value(rest)?
    };
    Some((specifier, &value[quote..]))
}

fn quoted_value(value: &str) -> Option<String> {
    let trimmed = value.trim();
    let quote = trimmed.chars().next()?;
    if quote != '\'' && quote != '"' {
        return None;
    }
    let end = trimmed[1..].find(quote)? + 1;
    Some(trimmed[1..end].to_string())
}

fn resolve_specifier(
    grammar: Grammar,
    source: &Path,
    root: &Path,
    specifier: &str,
    files: &[SourceFile],
) -> Vec<PathBuf> {
    let extensions = match grammar {
        Grammar::C => &["h", "c"][..],
        Grammar::Cpp => &["hpp", "hh", "hxx", "h", "cpp", "cc", "cxx"][..],
        Grammar::CSharp => &["cs"][..],
        Grammar::Go => &["go"][..],
        Grammar::Java => &["java"][..],
        Grammar::Python => &["py", "pyi"][..],
        Grammar::Php => &["php", "phtml"][..],
        Grammar::Ruby => &["rb", "rake"][..],
        Grammar::Bash => &["sh", "bash"][..],
        Grammar::Lua => &["lua"][..],
        Grammar::Dart => &["dart"][..],
        Grammar::Swift => &["swift"][..],
        Grammar::Kotlin => &["kt", "kts"][..],
    };
    let local_only = matches!(
        grammar,
        Grammar::C | Grammar::Cpp | Grammar::Bash | Grammar::Php
    ) || specifier.starts_with('.')
        || specifier.starts_with('/');
    let normalized = match grammar {
        Grammar::Python => python_module_path(source, specifier),
        Grammar::Dart if specifier.starts_with("package:") => dart_package_path(root, specifier),
        Grammar::Lua if specifier.starts_with('.') || specifier.starts_with('/') => {
            Some(specifier.to_string())
        }
        Grammar::Lua => Some(specifier.replace('.', "/")),
        Grammar::Go => go_module_path(root, specifier),
        Grammar::Java | Grammar::Kotlin => Some(specifier.replace('.', "/")),
        Grammar::CSharp => Some(specifier.replace('.', "/")),
        _ => Some(specifier.to_string()),
    };
    let Some(normalized) = normalized else {
        return Vec::new();
    };
    if local_only && !specifier.starts_with('.') && !specifier.starts_with('/') {
        return Vec::new();
    }

    let wildcard = normalized.ends_with("/*");
    let normalized = normalized.trim_end_matches("/*");
    let path = Path::new(normalized);
    let bases = if path.is_absolute() {
        vec![path.to_path_buf()]
    } else {
        let mut bases = vec![source.parent().unwrap_or(root).join(path), root.join(path)];
        if matches!(grammar, Grammar::Python) && !specifier.starts_with('.') {
            bases.extend(
                ["src", "lib", "app"]
                    .into_iter()
                    .map(|prefix| root.join(prefix).join(path)),
            );
        }
        bases
    };
    let mut targets = Vec::new();
    for base in bases {
        add_file_candidates(&base, root, extensions, &mut targets);
        if matches!(
            grammar,
            Grammar::Python | Grammar::Lua | Grammar::CSharp | Grammar::Swift
        ) {
            for extension in extensions {
                add_file_candidates(
                    &base
                        .join(if *extension == "py" {
                            "__init__"
                        } else {
                            "index"
                        })
                        .with_extension(extension),
                    root,
                    extensions,
                    &mut targets,
                );
            }
        }
    }

    if targets.is_empty() && matches!(grammar, Grammar::Go) {
        let directory = root.join(normalized);
        targets.extend(
            files
                .iter()
                .filter(|file| file.path.parent() == Some(directory.as_path()))
                .filter(|file| file.path.extension().and_then(|ext| ext.to_str()) == Some("go"))
                .map(|file| file.path.clone()),
        );
    }

    if targets.is_empty() && matches!(grammar, Grammar::Java | Grammar::Kotlin) {
        let suffix_path = path.with_extension(match grammar {
            Grammar::Java => "java",
            _ => "kt",
        });
        targets.extend(
            files
                .iter()
                .filter(|file| {
                    wildcard
                        && file
                            .path
                            .parent()
                            .is_some_and(|parent| parent.ends_with(path))
                })
                .chain(
                    files
                        .iter()
                        .filter(|file| !wildcard && file.path.ends_with(&suffix_path)),
                )
                .filter(|file| {
                    file.path
                        .extension()
                        .and_then(|ext| ext.to_str())
                        .is_some_and(|ext| extensions.contains(&ext))
                })
                .map(|file| file.path.clone()),
        );
    }

    if targets.is_empty() && matches!(grammar, Grammar::CSharp | Grammar::Swift) {
        targets.extend(
            files
                .iter()
                .filter(|file| {
                    file.path
                        .parent()
                        .is_some_and(|parent| parent.ends_with(path))
                })
                .filter(|file| {
                    file.path
                        .extension()
                        .and_then(|ext| ext.to_str())
                        .is_some_and(|ext| extensions.contains(&ext))
                })
                .map(|file| file.path.clone()),
        );
    }

    targets.sort();
    targets.dedup();
    targets
}

fn add_file_candidates(base: &Path, root: &Path, extensions: &[&str], output: &mut Vec<PathBuf>) {
    if let Some(path) = canonical_file_under(base, root) {
        output.push(path);
    }
    if base.extension().is_none() {
        for extension in extensions {
            if let Some(path) = canonical_file_under(&base.with_extension(extension), root) {
                output.push(path);
            }
        }
    }
}

fn python_module_path(source: &Path, specifier: &str) -> Option<String> {
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

fn go_module_path(root: &Path, specifier: &str) -> Option<String> {
    let manifest = std::fs::read_to_string(root.join("go.mod")).ok()?;
    let module = manifest.lines().find_map(|line| {
        line.trim()
            .strip_prefix("module ")
            .map(str::trim)
            .map(|value| value.trim_matches('"'))
    })?;
    if specifier == module {
        return Some(String::new());
    }
    specifier
        .strip_prefix(module)
        .filter(|suffix| suffix.starts_with('/'))
        .map(|suffix| suffix.trim_start_matches('/').to_string())
}

fn dart_package_path(root: &Path, specifier: &str) -> Option<String> {
    let (package, path) = specifier.trim_start_matches("package:").split_once('/')?;
    let manifest = std::fs::read_to_string(root.join("pubspec.yaml")).ok()?;
    let project_package = manifest.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        (key.trim() == "name").then(|| value.trim().trim_matches(['\'', '"']))
    })?;
    (package == project_package).then(|| format!("lib/{path}"))
}
