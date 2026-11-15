pub(in super::super) fn join_rel(parent: &str, child: &str) -> String {
    let combined = if parent.is_empty() {
        child.to_string()
    } else {
        format!("{parent}/{child}")
    };
    normalize_rel(&combined)
}

pub(in super::super) fn normalize_rel(path: &str) -> String {
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

pub(in super::super) fn parent_dir(path: &str) -> String {
    match path.rfind('/') {
        Some(index) => path[..index].to_string(),
        None => String::new(),
    }
}

pub(in super::super) fn file_stem(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.').map(|(stem, _)| stem).unwrap_or(name)
}

pub(in super::super) fn extension(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.').map(|(_, ext)| ext).unwrap_or("")
}
