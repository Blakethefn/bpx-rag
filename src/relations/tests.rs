use super::types::{FileAnalysis, FileRelationship};
use super::*;
use std::fs;
use tempfile::tempdir;

mod javascript;
mod rust;
mod python;
mod cfamily;
mod modules;
mod scripting;
mod scan;
mod project;

fn write_file(root: &std::path::Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

fn incoming_from(result: &FileAnalysis, from: &str, specifier: &str, kind: &str) -> bool {
    result.incoming.iter().any(|relationship| {
        relationship.from == from
            && relationship.specifier == specifier
            && relationship.kind == kind
    })
}

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

#[test]
fn unresolved_local_polyglot_imports_remain_outgoing_without_targets() {
    let dir = tempdir().unwrap();
    write_file(dir.path(), "src/main.c", "#include \"missing.h\"\n");
    write_file(
        dir.path(),
        "ruby/main.rb",
        "require_relative 'missing_file'\n",
    );

    let c = analyze_file(&dir.path().join("src/main.c"), Some(dir.path())).unwrap();
    let ruby = analyze_file(&dir.path().join("ruby/main.rb"), Some(dir.path())).unwrap();

    assert_eq!(c.outgoing.len(), 1);
    assert_eq!(c.outgoing[0].specifier, "missing.h");
    assert_eq!(c.outgoing[0].to, None);
    assert_eq!(ruby.outgoing.len(), 1);
    assert_eq!(ruby.outgoing[0].specifier, "./missing_file");
    assert_eq!(ruby.outgoing[0].to, None);
}
