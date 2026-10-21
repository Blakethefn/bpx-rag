use std::path::Path;

#[derive(Debug, Default)]
pub(super) struct ProjectMetadata {
    pub(super) go_module: Option<String>,
    pub(super) dart_package: Option<String>,
}

impl ProjectMetadata {
    pub(super) fn read(root: &Path) -> Self {
        Self {
            go_module: read_go_module(root),
            dart_package: read_dart_package(root),
        }
    }
}

fn read_go_module(root: &Path) -> Option<String> {
    let manifest = std::fs::read_to_string(root.join("go.mod")).ok()?;
    manifest.lines().find_map(|line| {
        let line = line.trim();
        let module = line.strip_prefix("module")?;
        if !module.starts_with(char::is_whitespace) {
            return None;
        }
        let module = module.split_once("//").map_or(module, |(module, _)| module);
        let value = module.trim().split_whitespace().next()?;
        Some(value.trim_matches(['"', '`']).to_string())
    })
}

fn read_dart_package(root: &Path) -> Option<String> {
    let manifest = std::fs::read_to_string(root.join("pubspec.yaml")).ok()?;
    manifest.lines().find_map(|line| {
        if line.starts_with(char::is_whitespace) {
            return None;
        }
        let (key, value) = line.split_once(':')?;
        if key.trim() != "name" {
            return None;
        }
        let value = value.trim();
        let value = if value.starts_with(['\'', '"']) {
            let quote = value.chars().next()?;
            let end = value[1..].find(quote)? + 1;
            &value[1..end]
        } else {
            value
                .split_once('#')
                .map_or(value, |(value, _)| value)
                .trim()
        };
        (!value.is_empty()).then(|| value.to_string())
    })
}
