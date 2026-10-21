use std::path::{Path, PathBuf};

use super::lexer::{lex_code, LexMode, TokenKind};
use super::paths::canonical_file_under;
use super::types::{RawReference, SourceFile};

pub(super) fn script_references(source: &SourceFile, root: &Path) -> Vec<RawReference> {
    let tokens = lex_code(&source.content, LexMode::Script);
    let mut references = Vec::new();
    let mut index = 0usize;
    while index < tokens.len() {
        let keyword = tokens[index].value.as_str();
        let is_import = keyword == "import";
        let is_export = keyword == "export";
        let is_require = keyword == "require";
        let is_import_meta =
            is_import && tokens.get(index + 1).map(|token| token.value.as_str()) == Some(".");
        if (is_import || is_export) && !is_import_meta {
            let end = tokens[index + 1..]
                .iter()
                .position(|token| token.value == ";")
                .map(|offset| index + offset + 1)
                .unwrap_or(tokens.len());
            let mut candidate = None;
            if is_import {
                if let Some(next) = tokens.get(index + 1) {
                    if next.kind == TokenKind::String {
                        candidate = Some(next);
                    } else if next.value == "(" {
                        candidate = tokens
                            .get(index + 2)
                            .filter(|token| token.kind == TokenKind::String);
                    }
                }
            }
            if candidate.is_none() {
                candidate = tokens[index + 1..end]
                    .windows(2)
                    .find(|pair| pair[0].value == "from" && pair[1].kind == TokenKind::String)
                    .map(|pair| &pair[1]);
            }
            if let Some(specifier) = candidate {
                if is_local_specifier(&specifier.value) {
                    references.push(RawReference {
                        kind: if is_import { "import" } else { "re-export" },
                        specifier: specifier.value.clone(),
                        line: tokens[index].line,
                        target: resolve_script_import(&source.path, root, &specifier.value),
                    });
                }
            }
        } else if is_require && tokens.get(index + 1).map(|token| token.value.as_str()) == Some("(")
        {
            if let Some(specifier) = tokens
                .get(index + 2)
                .filter(|token| token.kind == TokenKind::String)
            {
                if is_local_specifier(&specifier.value) {
                    references.push(RawReference {
                        kind: "import",
                        specifier: specifier.value.clone(),
                        line: tokens[index].line,
                        target: resolve_script_import(&source.path, root, &specifier.value),
                    });
                }
            }
        }
        index += 1;
    }
    references
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
