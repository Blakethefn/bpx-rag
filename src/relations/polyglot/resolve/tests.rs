use std::fs;

use tempfile::tempdir;

use crate::relations::languages::Grammar;
use crate::relations::metadata::ProjectMetadata;

use super::resolve_specifier;

fn canonical_root() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    (dir, root)
}

fn resolve(
    grammar: Grammar,
    root: &std::path::Path,
    source: &str,
    specifier: &str,
) -> Vec<std::path::PathBuf> {
    resolve_specifier(
        grammar,
        &root.join(source),
        root,
        specifier,
        &[],
        &ProjectMetadata::default(),
    )
}

#[test]
fn css_resolution_tries_preprocessor_suffixes() {
    let (_dir, root) = canonical_root();
    fs::write(root.join("theme.scss"), "").unwrap();

    assert_eq!(
        resolve(Grammar::Css, &root, "main.css", "./theme"),
        vec![root.join("theme.scss")]
    );
}

#[test]
fn html_resolution_tries_script_and_style_targets() {
    let (_dir, root) = canonical_root();
    fs::write(root.join("icon.svg"), "").unwrap();
    fs::write(root.join("app.tsx"), "").unwrap();

    assert_eq!(
        resolve(Grammar::Html, &root, "index.html", "./icon"),
        vec![root.join("icon.svg")]
    );
    assert_eq!(
        resolve(Grammar::Html, &root, "index.html", "./app"),
        vec![root.join("app.tsx")]
    );
}

#[test]
fn r_resolution_tries_capital_r() {
    let (_dir, root) = canonical_root();
    fs::write(root.join("plot.R"), "").unwrap();

    assert_eq!(
        resolve(Grammar::R, &root, "main.r", "./plot"),
        vec![root.join("plot.R")]
    );
}

#[test]
fn cpp_resolution_tries_c_headers_without_detecting_them_as_cpp() {
    let (_dir, root) = canonical_root();
    fs::write(root.join("header.h"), "").unwrap();

    assert_eq!(
        resolve(Grammar::Cpp, &root, "main.cpp", "header"),
        vec![root.join("header.h")]
    );
}

#[test]
fn python_resolution_tries_every_detection_extension() {
    let (_dir, root) = canonical_root();
    fs::create_dir_all(root.join("pkg")).unwrap();
    fs::write(root.join("pkg/stubs.pyi"), "").unwrap();

    assert_eq!(
        resolve(Grammar::Python, &root, "main.py", "pkg.stubs"),
        vec![root.join("pkg/stubs.pyi")]
    );
}
