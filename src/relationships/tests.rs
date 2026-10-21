use super::types::{FileAnalysis, FileRelationship};
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

    let result = analyze_file(&dir.path().join("src/lib/api/quotes.ts"), Some(dir.path())).unwrap();

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
