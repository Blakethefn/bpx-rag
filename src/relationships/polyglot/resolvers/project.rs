use std::path::Path;

use super::super::super::super::metadata::ProjectMetadata;
use super::super::super::super::types::SourceFile;

pub(in crate::relationships::polyglot::resolve) fn go_module_path(
    metadata: &ProjectMetadata,
    specifier: &str,
) -> Option<String> {
    let module = metadata.go_module.as_deref()?;
    if specifier == module {
        return Some(String::new());
    }
    specifier
        .strip_prefix(module)
        .filter(|suffix| suffix.starts_with('/'))
        .map(|suffix| suffix.trim_start_matches('/').to_string())
}

pub(in crate::relationships::polyglot::resolve) fn go_package_files(
    root: &Path,
    normalized: &str,
    files: &[SourceFile],
) -> Vec<std::path::PathBuf> {
    let directory = root.join(normalized);
    files
        .iter()
        .filter(|file| file.path.parent() == Some(directory.as_path()))
        .filter(|file| file.path.extension().and_then(|ext| ext.to_str()) == Some("go"))
        .map(|file| file.path.clone())
        .collect()
}

pub(in crate::relationships::polyglot::resolve) fn dart_package_path(
    metadata: &ProjectMetadata,
    specifier: &str,
) -> Option<String> {
    let (package, path) = specifier.trim_start_matches("package:").split_once('/')?;
    let project_package = metadata.dart_package.as_deref()?;
    (package == project_package).then(|| format!("lib/{path}"))
}
