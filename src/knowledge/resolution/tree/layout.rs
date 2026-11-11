impl super::Tree {
    /// The parent path would still name this child after the parent's `mod` edges are gone.
    ///
    /// `lib.rs`, `main.rs`, and `mod.rs` name the direct module files beside them.
    /// Any other Rust file names the tree under its module directory.
    pub(crate) fn path_declares_child(&self, parent_id: i64, child_id: i64) -> bool {
        if parent_id == child_id {
            return false;
        }
        if !self.rust_files.contains(&parent_id) || !self.rust_files.contains(&child_id) {
            return false;
        }
        if self.packages.get(&parent_id) != self.packages.get(&child_id) {
            return false;
        }
        let (Some(parent_rel), Some(child_rel)) =
            (self.paths.get(&parent_id), self.paths.get(&child_id))
        else {
            return false;
        };
        let base = module_base(parent_rel);
        if matches!(file_stem(parent_rel), "lib" | "main" | "mod") {
            direct_module_file(&base, child_rel)
        } else {
            descendant_module_file(&base, child_rel)
        }
    }
}

/// `lib.rs`, `main.rs`, and `mod.rs` keep children beside the file. Every other module keeps
/// them in a directory named after the file stem.
pub(super) fn module_base(rel_path: &str) -> String {
    let stem = file_stem(rel_path);
    let parent = parent_dir(rel_path);
    if matches!(stem, "lib" | "main" | "mod") {
        parent
    } else {
        join_rel(&parent, stem)
    }
}

/// `{base}/{name}.rs` or `{base}/{name}/mod.rs`. `mod.rs` beside the parent is the parent
/// directory's own file, not one of its child modules.
fn direct_module_file(base: &str, child_rel: &str) -> bool {
    let Some(rest) = strip_base(base, child_rel) else {
        return false;
    };
    if let Some(name) = rest.strip_suffix("/mod.rs") {
        return !name.is_empty() && !name.contains('/');
    }
    if let Some(name) = rest.strip_suffix(".rs") {
        return !name.is_empty() && !name.contains('/') && name != "mod";
    }
    false
}

fn descendant_module_file(base: &str, child_rel: &str) -> bool {
    if base.is_empty() {
        return false;
    }
    child_rel.starts_with(&format!("{base}/"))
}

fn strip_base<'a>(base: &str, child_rel: &'a str) -> Option<&'a str> {
    if base.is_empty() {
        return Some(child_rel);
    }
    child_rel.strip_prefix(&format!("{base}/"))
}

pub(super) fn parent_dir(path: &str) -> String {
    match path.rfind('/') {
        Some(index) => path[..index].to_string(),
        None => String::new(),
    }
}

fn file_stem(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.').map(|(stem, _)| stem).unwrap_or(name)
}

pub(super) fn extension(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.').map(|(_, ext)| ext).unwrap_or("")
}

pub(super) fn join_rel(parent: &str, child: &str) -> String {
    let combined = if parent.is_empty() {
        child.to_string()
    } else {
        format!("{parent}/{child}")
    };
    normalize_rel(&combined)
}

pub(super) fn normalize_rel(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    let mut parts = Vec::new();
    for part in normalized.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}
