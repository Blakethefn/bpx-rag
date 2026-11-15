use super::super::super::index::resolve_wikilink;
use std::collections::HashMap;

#[test]
fn resolve_exact_path() {
    let files: HashMap<String, i64> = [("01-projects/bpcontext.md".into(), 1)]
        .into_iter()
        .collect();
    assert_eq!(
        resolve_wikilink("01-projects/bpcontext.md", &files),
        Some(1)
    );
}

#[test]
fn resolve_appends_md_extension() {
    let files: HashMap<String, i64> = [("notes/idea.md".into(), 2)].into_iter().collect();
    assert_eq!(resolve_wikilink("notes/idea", &files), Some(2));
}

#[test]
fn resolve_basename_match() {
    let files: HashMap<String, i64> = [("deep/path/my-note.md".into(), 3)].into_iter().collect();
    assert_eq!(resolve_wikilink("my-note", &files), Some(3));
}

#[test]
fn resolve_basename_without_extension() {
    let files: HashMap<String, i64> = [("folder/config".into(), 4)].into_iter().collect();
    assert_eq!(resolve_wikilink("config", &files), Some(4));
}

#[test]
fn resolve_returns_none_for_missing() {
    let files: HashMap<String, i64> = [("a.md".into(), 1)].into_iter().collect();
    assert_eq!(resolve_wikilink("nonexistent", &files), None);
}
