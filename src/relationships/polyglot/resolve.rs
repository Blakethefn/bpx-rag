#[path = "resolvers/mod.rs"]
mod resolvers;

use std::path::{Path, PathBuf};

use super::super::languages::Grammar;
use super::super::metadata::ProjectMetadata;
use super::super::types::SourceFile;

pub(super) fn resolve_specifier(
    grammar: Grammar,
    source: &Path,
    root: &Path,
    specifier: &str,
    files: &[SourceFile],
    metadata: &ProjectMetadata,
) -> Vec<PathBuf> {
    let extensions = match grammar {
        Grammar::C => &["h", "c"][..],
        Grammar::Cpp => &["hpp", "hh", "hxx", "h", "cpp", "cc", "cxx"][..],
        Grammar::CSharp => &["cs"][..],
        Grammar::JavaScript | Grammar::TypeScript | Grammar::Tsx => {
            &["ts", "tsx", "js", "jsx", "mjs", "cjs", "json"][..]
        }
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
        Grammar::Markdown | Grammar::Rust => &[][..],
    };
    let normalized = match grammar {
        Grammar::Python => resolvers::python_module_path(source, specifier),
        Grammar::Dart if specifier.starts_with("package:") => {
            resolvers::dart_package_path(metadata, specifier)
        }
        Grammar::Lua if specifier.starts_with('.') || specifier.starts_with('/') => {
            Some(specifier.to_string())
        }
        Grammar::Lua => Some(specifier.replace('.', "/")),
        Grammar::Go => resolvers::go_module_path(metadata, specifier),
        Grammar::Java | Grammar::Kotlin => Some(specifier.replace('.', "/")),
        Grammar::CSharp => Some(specifier.replace('.', "/")),
        Grammar::JavaScript | Grammar::TypeScript | Grammar::Tsx => Some(specifier.to_string()),
        Grammar::Markdown | Grammar::Rust => None,
        _ => Some(specifier.to_string()),
    };
    let Some(normalized) = normalized else {
        return Vec::new();
    };
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
        resolvers::add_file_candidates(&base, root, extensions, &mut targets);
        if matches!(
            grammar,
            Grammar::Python | Grammar::Lua | Grammar::CSharp | Grammar::Swift
        ) {
            for extension in extensions {
                resolvers::add_file_candidates(
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
        targets.extend(resolvers::go_package_files(root, normalized, files));
    }

    if targets.is_empty() && matches!(grammar, Grammar::Java | Grammar::Kotlin) {
        targets.extend(resolvers::jvm_targets(
            grammar, path, wildcard, files, extensions,
        ));
    }

    if targets.is_empty() && matches!(grammar, Grammar::CSharp | Grammar::Swift) {
        targets.extend(resolvers::namespace_targets(path, files, extensions));
    }

    targets.sort();
    targets.dedup();
    targets
}
