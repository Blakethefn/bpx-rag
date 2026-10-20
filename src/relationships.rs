use anyhow::{bail, Context, Result};
use ignore::WalkBuilder;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

const MAX_FILES: usize = 20_000;
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 128 * 1024 * 1024;
const SOURCE_EXTENSIONS: &[&str] = &["md", "rs", "ts", "tsx", "js", "jsx", "mjs", "cjs"];

#[derive(Debug, Serialize)]
pub struct FileRelationship {
    pub from: String,
    pub to: Option<String>,
    pub kind: String,
    pub specifier: String,
    pub line: usize,
    pub direction: String,
}

#[derive(Debug, Serialize)]
pub struct FileAnalysis {
    pub root: String,
    pub file: String,
    pub scanned_files: usize,
    pub outgoing: Vec<FileRelationship>,
    pub incoming: Vec<FileRelationship>,
    pub warnings: Vec<String>,
}

pub fn analyze_file(path: &Path, root: Option<&Path>) -> Result<FileAnalysis> {
    let unresolved_path = match root {
        Some(root) if !path.is_absolute() => root.join(path),
        _ => path.to_path_buf(),
    };
    let target = unresolved_path
        .canonicalize()
        .with_context(|| format!("could not resolve file {}", path.display()))?;
    if !target.is_file() {
        bail!("{} is not a regular file", target.display());
    }

    let root = match root {
        Some(path) => {
            let unresolved_root = if path.is_absolute() {
                path.to_path_buf()
            } else {
                std::env::current_dir()?.join(path)
            };
            unresolved_root
                .canonicalize()
                .with_context(|| format!("could not resolve project root {}", path.display()))?
        }
        None => infer_root(&target)?,
    };
    if !root.is_dir() {
        bail!("project root {} is not a directory", root.display());
    }
    if !target.starts_with(&root) {
        bail!(
            "file {} is outside project root {}",
            target.display(),
            root.display()
        );
    }
    if !is_supported(&target) {
        bail!("unsupported file type: {}", target.display());
    }

    let mut files = Vec::new();
    let mut warnings = Vec::new();
    let mut skipped = 0usize;
    let mut reached_limit = false;
    let target_metadata = fs::metadata(&target)
        .with_context(|| format!("could not read metadata for {}", target.display()))?;
    if target_metadata.len() > MAX_FILE_BYTES {
        bail!("selected file exceeds the {MAX_FILE_BYTES}-byte analysis limit");
    }
    let target_content = fs::read_to_string(&target)
        .with_context(|| format!("selected file is not readable UTF-8: {}", target.display()))?;
    let mut total_bytes = target_metadata.len();
    files.push(SourceFile {
        path: target.clone(),
        content: target_content,
    });
    let mut builder = WalkBuilder::new(&root);
    builder.hidden(true).git_ignore(true).git_global(true);
    builder.filter_entry(|entry| !is_excluded_directory(entry.path()));

    for entry in builder.build() {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                skipped += 1;
                if warnings.is_empty() {
                    warnings.push(format!("Some files could not be read: {error}"));
                }
                continue;
            }
        };
        if !entry.file_type().map_or(false, |kind| kind.is_file()) || !is_supported(entry.path()) {
            continue;
        }
        if entry.path() == target {
            continue;
        }

        if files.len() >= MAX_FILES {
            reached_limit = true;
            break;
        }

        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(_) => {
                skipped += 1;
                continue;
            }
        };
        if metadata.len() > MAX_FILE_BYTES {
            skipped += 1;
            continue;
        }
        if total_bytes.saturating_add(metadata.len()) > MAX_TOTAL_BYTES {
            reached_limit = true;
            break;
        }
        let content = match fs::read_to_string(entry.path()) {
            Ok(content) => content,
            Err(_) => {
                skipped += 1;
                continue;
            }
        };
        total_bytes = total_bytes.saturating_add(metadata.len());
        files.push(SourceFile {
            path: entry.path().to_path_buf(),
            content,
        });
    }

    if reached_limit {
        warnings.push(format!(
            "Scan reached its file or byte limit ({MAX_FILES} files, {MAX_TOTAL_BYTES} bytes); results may be incomplete."
        ));
    }
    if skipped > 0 && warnings.is_empty() {
        warnings.push(format!("Skipped {skipped} unreadable or oversized files."));
    }

    let mut outgoing = Vec::new();
    let mut incoming = Vec::new();
    let scanned_files = files.len();
    let relative_target = display_relative(&target, &root)?;

    for source in &files {
        for reference in extract_references(source, &root, &files) {
            let relation = FileRelationship {
                from: display_relative(&source.path, &root)?,
                to: reference
                    .target
                    .as_ref()
                    .and_then(|path| display_relative(path, &root).ok()),
                kind: reference.kind.to_string(),
                specifier: reference.specifier,
                line: reference.line,
                direction: if source.path == target {
                    "outgoing".to_string()
                } else {
                    "incoming".to_string()
                },
            };

            if source.path == target {
                outgoing.push(relation);
            } else if reference.target.as_deref() == Some(target.as_path()) {
                incoming.push(relation);
            }
        }
    }

    sort_relationships(&mut outgoing);
    sort_relationships(&mut incoming);

    Ok(FileAnalysis {
        root: root.display().to_string(),
        file: relative_target,
        scanned_files,
        outgoing,
        incoming,
        warnings,
    })
}

pub fn format_analysis(analysis: &FileAnalysis) -> String {
    let mut output = format!(
        "File relationships for `{}`\nRoot: `{}`\nScanned {} supported files (no database used).\n\n",
        analysis.file, analysis.root, analysis.scanned_files
    );
    append_relationship_section(&mut output, "Outgoing", &analysis.outgoing);
    output.push('\n');
    append_relationship_section(&mut output, "Incoming", &analysis.incoming);
    if !analysis.warnings.is_empty() {
        output.push_str("\nWarnings:\n");
        for warning in &analysis.warnings {
            output.push_str(&format!("- {warning}\n"));
        }
    }
    output
}

fn append_relationship_section(
    output: &mut String,
    title: &str,
    relationships: &[FileRelationship],
) {
    let mut grouped =
        BTreeMap::<(String, Option<String>, String, String), Vec<&FileRelationship>>::new();
    for relationship in relationships {
        let unresolved_specifier = if relationship.to.is_none() {
            relationship.specifier.clone()
        } else {
            String::new()
        };
        grouped
            .entry((
                relationship.from.clone(),
                relationship.to.clone(),
                relationship.kind.clone(),
                unresolved_specifier,
            ))
            .or_default()
            .push(relationship);
    }

    output.push_str(&format!("{title} ({}):\n", grouped.len()));
    if grouped.is_empty() {
        output.push_str("- None found.\n");
        return;
    }
    for ((from, to, kind, _), evidence) in grouped {
        let destination = to.as_deref().unwrap_or("unresolved local reference");
        let lines = evidence
            .iter()
            .map(|relationship| relationship.line)
            .collect::<BTreeSet<_>>();
        let specifiers = evidence
            .iter()
            .map(|relationship| relationship.specifier.as_str())
            .collect::<BTreeSet<_>>();
        let line_label = if lines.len() == 1 { "line" } else { "lines" };
        let specifier_label = if specifiers.len() == 1 { "ref" } else { "refs" };
        output.push_str(&format!(
            "- `{from}` -> `{destination}` ({kind}, {line_label} {}, {specifier_label} {})\n",
            lines
                .iter()
                .map(usize::to_string)
                .collect::<Vec<_>>()
                .join(", "),
            specifiers
                .iter()
                .map(|specifier| format!("`{specifier}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
}

struct SourceFile {
    path: PathBuf,
    content: String,
}

struct RawReference {
    kind: &'static str,
    specifier: String,
    line: usize,
    target: Option<PathBuf>,
}

fn infer_root(target: &Path) -> Result<PathBuf> {
    let parent = target.parent().context("file has no parent directory")?;
    for ancestor in parent.ancestors() {
        if [".git", "Cargo.toml", "package.json", "pyproject.toml"]
            .iter()
            .any(|marker| ancestor.join(marker).exists())
        {
            return ancestor.canonicalize().with_context(|| {
                format!("could not resolve inferred root {}", ancestor.display())
            });
        }
    }
    parent
        .canonicalize()
        .with_context(|| format!("could not resolve file directory {}", parent.display()))
}

fn is_supported(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| SOURCE_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

fn is_excluded_directory(path: &Path) -> bool {
    const EXCLUDED: &[&str] = &[".git", "node_modules", "target", "dist", "build", ".next"];
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| EXCLUDED.contains(&name))
        .unwrap_or(false)
}

fn display_relative(path: &Path, root: &Path) -> Result<String> {
    path.strip_prefix(root)
        .map(|relative| relative.to_string_lossy().replace('\\', "/"))
        .with_context(|| {
            format!(
                "{} is outside project root {}",
                path.display(),
                root.display()
            )
        })
}

fn sort_relationships(relationships: &mut [FileRelationship]) {
    relationships.sort_by(|left, right| {
        left.from
            .cmp(&right.from)
            .then(left.line.cmp(&right.line))
            .then(left.specifier.cmp(&right.specifier))
    });
}

fn extract_references(source: &SourceFile, root: &Path, files: &[SourceFile]) -> Vec<RawReference> {
    let extension = source
        .path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    match extension.as_deref() {
        Some("md") => markdown_references(source, root, files),
        Some("rs") => rust_references(source, root),
        Some("ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs") => script_references(source, root),
        _ => Vec::new(),
    }
}

fn markdown_references(
    source: &SourceFile,
    root: &Path,
    files: &[SourceFile],
) -> Vec<RawReference> {
    let mut references = Vec::new();
    let mut in_fence = false;

    for (line_index, line) in source.content.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }

        let bytes = line.as_bytes();
        let mut cursor = 0usize;
        let mut in_inline_code = false;
        while cursor < bytes.len() {
            if bytes[cursor] == b'`' {
                in_inline_code = !in_inline_code;
                cursor += 1;
                continue;
            }
            if !in_inline_code
                && cursor + 1 < bytes.len()
                && bytes[cursor] == b'['
                && bytes[cursor + 1] == b'['
            {
                let start = cursor + 2;
                if let Some(end) = line[start..].find("]]") {
                    let specifier = line[start..start + end]
                        .split('|')
                        .next()
                        .unwrap_or_default()
                        .split('#')
                        .next()
                        .unwrap_or_default()
                        .trim()
                        .to_string();
                    if !specifier.is_empty() {
                        let target = resolve_wikilink(&source.path, root, &specifier, files);
                        references.push(RawReference {
                            kind: "wikilink",
                            specifier,
                            line: line_index + 1,
                            target,
                        });
                    }
                    cursor = start + end + 2;
                    continue;
                }
            }
            cursor += 1;
        }
    }
    references
}

fn resolve_wikilink(
    source: &Path,
    root: &Path,
    specifier: &str,
    files: &[SourceFile],
) -> Option<PathBuf> {
    let target = Path::new(specifier);
    if target.is_absolute() {
        return None;
    }
    let mut candidates = Vec::new();
    let source_parent = source.parent()?;
    candidates.push(source_parent.join(target));
    candidates.push(root.join(target));
    if target.extension().is_none() {
        candidates.push(source_parent.join(target).with_extension("md"));
        candidates.push(root.join(target).with_extension("md"));
    }

    for candidate in candidates {
        if let Some(resolved) = canonical_file_under(&candidate, root) {
            return Some(resolved);
        }
    }

    let wanted = target
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(specifier)
        .to_ascii_lowercase();
    let matches = files
        .iter()
        .filter(|file| file.path.extension().and_then(|ext| ext.to_str()) == Some("md"))
        .filter(|file| {
            file.path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .map(|stem| stem.eq_ignore_ascii_case(&wanted))
                .unwrap_or(false)
        })
        .map(|file| file.path.clone())
        .collect::<Vec<_>>();
    if matches.len() == 1 {
        matches.into_iter().next()
    } else {
        None
    }
}

fn rust_references(source: &SourceFile, root: &Path) -> Vec<RawReference> {
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

fn script_references(source: &SourceFile, root: &Path) -> Vec<RawReference> {
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum LexMode {
    Rust,
    Script,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TokenKind {
    Ident,
    String,
    Punctuation,
}

#[derive(Clone)]
struct Token {
    value: String,
    line: usize,
    kind: TokenKind,
}

fn lex_code(source: &str, mode: LexMode) -> Vec<Token> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0usize;
    let mut line = 1usize;
    while index < bytes.len() {
        if bytes[index].is_ascii_whitespace() {
            if bytes[index] == b'\n' {
                line += 1;
            }
            index += 1;
            continue;
        }
        if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'/') {
            index += 2;
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
            continue;
        }
        if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'*') {
            index += 2;
            let mut depth = 1usize;
            while index < bytes.len() && depth > 0 {
                if bytes[index] == b'\n' {
                    line += 1;
                    index += 1;
                } else if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'*') {
                    depth += 1;
                    index += 2;
                } else if bytes[index] == b'*' && bytes.get(index + 1) == Some(&b'/') {
                    depth -= 1;
                    index += 2;
                } else {
                    index += 1;
                }
            }
            continue;
        }

        let quote = bytes[index];
        if quote == b'"' || (mode == LexMode::Script && (quote == b'\'' || quote == b'`')) {
            let token_line = line;
            index += 1;
            let mut value = String::new();
            while index < bytes.len() && bytes[index] != quote {
                if bytes[index] == b'\\' && index + 1 < bytes.len() {
                    value.push(bytes[index + 1] as char);
                    index += 2;
                } else {
                    if bytes[index] == b'\n' {
                        line += 1;
                    }
                    value.push(bytes[index] as char);
                    index += 1;
                }
            }
            if index < bytes.len() {
                index += 1;
            }
            tokens.push(Token {
                value,
                line: token_line,
                kind: TokenKind::String,
            });
            continue;
        }
        if mode == LexMode::Rust && quote == b'\'' {
            tokens.push(Token {
                value: "'".to_string(),
                line,
                kind: TokenKind::Punctuation,
            });
            index += 1;
            continue;
        }
        if is_ident_start(bytes[index]) {
            let start = index;
            index += 1;
            while index < bytes.len() && is_ident_continue(bytes[index]) {
                index += 1;
            }
            tokens.push(Token {
                value: String::from_utf8_lossy(&bytes[start..index]).to_string(),
                line,
                kind: TokenKind::Ident,
            });
            continue;
        }
        if bytes[index] == b':' && bytes.get(index + 1) == Some(&b':') {
            tokens.push(Token {
                value: "::".to_string(),
                line,
                kind: TokenKind::Punctuation,
            });
            index += 2;
            continue;
        }
        tokens.push(Token {
            value: (bytes[index] as char).to_string(),
            line,
            kind: TokenKind::Punctuation,
        });
        index += 1;
    }
    tokens
}

fn is_ident_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_' || byte == b'$'
}

fn is_ident_continue(byte: u8) -> bool {
    is_ident_start(byte) || byte.is_ascii_digit()
}

fn canonical_file_under(path: &Path, root: &Path) -> Option<PathBuf> {
    let canonical = path.canonicalize().ok()?;
    if canonical.starts_with(root) && canonical.is_file() {
        Some(canonical)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn markdown_wikilinks_return_outgoing_and_incoming_line_evidence() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("alpha.md"), "# Alpha\nSee [[beta|Beta]].\n").unwrap();
        fs::write(dir.path().join("beta.md"), "Back to [[alpha]].\n").unwrap();

        let result = analyze_file(&dir.path().join("alpha.md"), Some(dir.path())).unwrap();

        assert_eq!(result.outgoing.len(), 1);
        assert_eq!(result.outgoing[0].to.as_deref(), Some("beta.md"));
        assert_eq!(result.outgoing[0].line, 2);
        assert_eq!(result.incoming.len(), 1);
        assert_eq!(result.incoming[0].from, "beta.md");
        assert_eq!(result.incoming[0].line, 1);
    }

    #[test]
    fn typescript_relative_imports_find_incoming_files_without_a_database() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src/lib/api")).unwrap();
        fs::create_dir_all(dir.path().join("src/hooks")).unwrap();
        fs::write(dir.path().join("package.json"), "{}\n").unwrap();
        fs::write(
            dir.path().join("src/lib/api/quotes.ts"),
            "export function getQuotes() {}\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("src/hooks/useQuotes.ts"),
            "import { getQuotes } from '../lib/api/quotes';\n",
        )
        .unwrap();

        let result =
            analyze_file(&dir.path().join("src/lib/api/quotes.ts"), Some(dir.path())).unwrap();

        assert_eq!(result.incoming.len(), 1);
        assert_eq!(result.incoming[0].from, "src/hooks/useQuotes.ts");
        assert_eq!(result.incoming[0].kind, "import");
    }

    #[test]
    fn typescript_import_meta_does_not_create_a_false_import_edge() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("package.json"), "{}\n").unwrap();
        fs::write(
            dir.path().join("src/quotes.ts"),
            "export const quote = 1;\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("src/useQuotes.ts"),
            "const url = import.meta.url\nimport { quote } from './quotes';\n",
        )
        .unwrap();

        let result = analyze_file(&dir.path().join("src/quotes.ts"), Some(dir.path())).unwrap();

        assert_eq!(result.incoming.len(), 1);
        assert_eq!(result.incoming[0].from, "src/useQuotes.ts");
        assert_eq!(result.incoming[0].line, 2);
    }

    #[test]
    fn rust_module_declarations_find_the_declared_file() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src/api")).unwrap();
        fs::write(dir.path().join("Cargo.toml"), "[package]\nname='fixture'\n").unwrap();
        fs::write(dir.path().join("src/lib.rs"), "pub mod api;\n").unwrap();
        fs::write(dir.path().join("src/api.rs"), "pub mod quotes;\n").unwrap();
        fs::write(dir.path().join("src/api/quotes.rs"), "pub struct Quote;\n").unwrap();

        let result = analyze_file(&dir.path().join("src/api/quotes.rs"), Some(dir.path())).unwrap();

        assert_eq!(result.incoming.len(), 1);
        assert_eq!(result.incoming[0].from, "src/api.rs");
        assert_eq!(result.incoming[0].kind, "module");
    }

    #[test]
    fn next_analysis_reads_current_filesystem_state() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("alpha.md"), "No links yet.\n").unwrap();
        fs::write(dir.path().join("beta.md"), "\n").unwrap();

        let before = analyze_file(&dir.path().join("alpha.md"), Some(dir.path())).unwrap();
        fs::write(dir.path().join("alpha.md"), "[[beta]]\n").unwrap();
        let after = analyze_file(&dir.path().join("alpha.md"), Some(dir.path())).unwrap();

        assert!(before.outgoing.is_empty());
        assert_eq!(after.outgoing.len(), 1);
    }

    #[test]
    fn file_outside_root_is_rejected() {
        let dir = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let target = outside.path().join("outside.md");
        fs::write(&target, "text\n").unwrap();

        assert!(analyze_file(&target, Some(dir.path())).is_err());
    }

    #[test]
    fn formatted_output_groups_repeated_references_to_the_same_file() {
        let analysis = FileAnalysis {
            root: "/tmp/project".to_string(),
            file: "src/quotes.ts".to_string(),
            scanned_files: 2,
            outgoing: vec![
                FileRelationship {
                    from: "src/quotes.ts".to_string(),
                    to: Some("src/types.ts".to_string()),
                    kind: "import".to_string(),
                    specifier: "./types".to_string(),
                    line: 1,
                    direction: "outgoing".to_string(),
                },
                FileRelationship {
                    from: "src/quotes.ts".to_string(),
                    to: Some("src/types.ts".to_string()),
                    kind: "import".to_string(),
                    specifier: "./types".to_string(),
                    line: 2,
                    direction: "outgoing".to_string(),
                },
            ],
            incoming: Vec::new(),
            warnings: Vec::new(),
        };

        let rendered = format_analysis(&analysis);

        assert!(rendered.contains("Outgoing (1)"));
        assert!(rendered.contains("lines 1, 2"));
        assert!(rendered.contains("Incoming (0)"));
    }

    #[test]
    fn rust_use_paths_resolve_to_module_files() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src/api")).unwrap();
        fs::write(dir.path().join("Cargo.toml"), "[package]\nname='fixture'\n").unwrap();
        fs::write(dir.path().join("src/api.rs"), "pub mod quotes;\n").unwrap();
        fs::write(dir.path().join("src/api/quotes.rs"), "pub struct Quote;\n").unwrap();
        fs::write(
            dir.path().join("src/service.rs"),
            "use crate::api::quotes::Quote;\n",
        )
        .unwrap();

        let result = analyze_file(&dir.path().join("src/api/quotes.rs"), Some(dir.path())).unwrap();

        assert_eq!(result.incoming.len(), 2);
        assert!(result.incoming.iter().any(|relationship| {
            relationship.from == "src/service.rs"
                && relationship.line == 1
                && relationship.kind == "import"
        }));
        assert!(result.incoming.iter().any(|relationship| {
            relationship.from == "src/api.rs" && relationship.kind == "module"
        }));
    }

    #[test]
    fn rust_grouped_use_paths_resolve_nested_modules() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src/api")).unwrap();
        fs::write(dir.path().join("Cargo.toml"), "[package]\nname='fixture'\n").unwrap();
        fs::write(dir.path().join("src/api.rs"), "pub mod quotes;\n").unwrap();
        fs::write(dir.path().join("src/api/quotes.rs"), "pub struct Quote;\n").unwrap();
        fs::write(
            dir.path().join("src/service.rs"),
            "use crate::{api::quotes::Quote};\n",
        )
        .unwrap();

        let result = analyze_file(&dir.path().join("src/api/quotes.rs"), Some(dir.path())).unwrap();

        assert!(result.incoming.iter().any(|relationship| {
            relationship.from == "src/service.rs" && relationship.kind == "import"
        }));
    }

    #[test]
    fn ignored_directories_are_not_scanned_for_incoming_references() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::create_dir_all(dir.path().join("node_modules/pkg")).unwrap();
        fs::write(dir.path().join(".gitignore"), "node_modules/\n").unwrap();
        fs::write(dir.path().join("src/alpha.md"), "# Alpha\n").unwrap();
        fs::write(
            dir.path().join("node_modules/pkg/ref.md"),
            "[[../../src/alpha]]\n",
        )
        .unwrap();

        let result = analyze_file(&dir.path().join("src/alpha.md"), Some(dir.path())).unwrap();

        assert!(result.incoming.is_empty());
    }

    #[test]
    fn selected_ignored_file_still_returns_its_own_outgoing_links() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join(".gitignore"), "alpha.md\n").unwrap();
        fs::write(dir.path().join("alpha.md"), "See [[beta]].\n").unwrap();
        fs::write(dir.path().join("beta.md"), "# Beta\n").unwrap();

        let result = analyze_file(&dir.path().join("alpha.md"), Some(dir.path())).unwrap();

        assert_eq!(result.outgoing.len(), 1);
        assert_eq!(result.outgoing[0].to.as_deref(), Some("beta.md"));
    }

    #[test]
    fn unresolved_markdown_links_are_reported_without_guessing() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("alpha.md"), "See [[missing-note]].\n").unwrap();

        let result = analyze_file(&dir.path().join("alpha.md"), Some(dir.path())).unwrap();

        assert_eq!(result.outgoing.len(), 1);
        assert_eq!(result.outgoing[0].to, None);
        assert_eq!(result.outgoing[0].specifier, "missing-note");
    }

    #[test]
    fn relative_parent_wikilinks_resolve_when_they_stay_inside_the_root() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("docs")).unwrap();
        fs::write(dir.path().join("alpha.md"), "# Alpha\n").unwrap();
        fs::write(dir.path().join("docs/guide.md"), "See [[../alpha]].\n").unwrap();

        let result = analyze_file(&dir.path().join("docs/guide.md"), Some(dir.path())).unwrap();

        assert_eq!(result.outgoing[0].to.as_deref(), Some("alpha.md"));
    }
}
