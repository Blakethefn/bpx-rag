use super::rel::{extension, file_stem, join_rel, parent_dir};
use super::{is_directory_module_stem, is_path_keyword};

pub(super) fn module_candidates(from_rel: &str, specifier: &str) -> Vec<String> {
    let from_rel = from_rel.replace('\\', "/");
    let specifier = specifier.trim().replace('\\', "/");
    if specifier.is_empty() {
        return Vec::new();
    }
    let ext = extension(&from_rel);
    if matches!(ext, "py" | "pyi") {
        return python_candidates(&from_rel, &specifier);
    }
    if ext == "rs" {
        return rust_candidates(&from_rel, &specifier);
    }
    if ext == "go" {
        return go_candidates(&specifier);
    }
    if specifier.starts_with('.') {
        return relative_candidates(&from_rel, &specifier);
    }
    let mut candidates = Vec::new();
    let parent = parent_dir(&from_rel);
    for candidate_ext in ["rs", "py", "ts", "tsx", "js", "jsx", "go"] {
        candidates.push(join_rel(&parent, &format!("{specifier}.{candidate_ext}")));
    }
    candidates
}

pub(super) fn rust_candidates(from_rel: &str, specifier: &str) -> Vec<String> {
    let name = specifier
        .rsplit("::")
        .find(|part| !part.is_empty())
        .unwrap_or(specifier);
    if is_path_keyword(name) {
        return Vec::new();
    }
    let parent = parent_dir(from_rel);
    let stem = file_stem(from_rel);
    let mut bases = Vec::new();
    if !is_directory_module_stem(stem) {
        bases.push(join_rel(&parent, stem));
    }
    bases.push(parent);
    let mut candidates = Vec::new();
    for base in bases {
        candidates.push(join_rel(&base, &format!("{name}.rs")));
        candidates.push(join_rel(&base, &format!("{name}/mod.rs")));
    }
    candidates
}

fn python_candidates(from_rel: &str, specifier: &str) -> Vec<String> {
    let mut level = 0usize;
    let rest = specifier.trim_start_matches(|character| {
        if character == '.' {
            level += 1;
            true
        } else {
            false
        }
    });
    let mut base = parent_dir(from_rel);
    if level > 1 {
        for _ in 1..level {
            base = parent_dir(&base);
        }
    }
    let dotted = rest.trim_matches('.').replace('.', "/");
    if dotted.is_empty() {
        return Vec::new();
    }
    if level == 0 {
        let parent = parent_dir(from_rel);
        vec![
            join_rel(&parent, &format!("{dotted}.py")),
            join_rel(&parent, &format!("{dotted}/__init__.py")),
            format!("{dotted}.py"),
            format!("{dotted}/__init__.py"),
        ]
    } else {
        vec![
            join_rel(&base, &format!("{dotted}.py")),
            join_rel(&base, &format!("{dotted}/__init__.py")),
        ]
    }
}

fn go_candidates(specifier: &str) -> Vec<String> {
    let name = specifier
        .rsplit('/')
        .find(|part| !part.is_empty())
        .unwrap_or(specifier);
    vec![format!("{name}/{name}.go"), format!("{name}.go")]
}

fn relative_candidates(from_rel: &str, specifier: &str) -> Vec<String> {
    let joined = join_rel(&parent_dir(from_rel), specifier);
    let mut candidates = vec![joined.clone()];
    for ext in ["ts", "tsx", "js", "jsx", "mjs", "cjs", "py", "rs"] {
        if !joined.ends_with(&format!(".{ext}")) {
            candidates.push(format!("{joined}.{ext}"));
        }
    }
    for ext in ["ts", "tsx", "js", "jsx"] {
        candidates.push(format!("{joined}/index.{ext}"));
    }
    candidates
}
