use super::*;

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
fn project_edges_resolve_rust_modules_in_one_pass() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/lib.rs"), "mod util;\n").unwrap();
    fs::write(dir.path().join("src/util.rs"), "pub fn helper() {}\n").unwrap();
    fs::write(dir.path().join("src/secret.rs"), "mod never_read;\n").unwrap();
    let root = dir.path().canonicalize().unwrap();
    let paths = vec![root.join("src/lib.rs"), root.join("src/util.rs")];

    let edges = project_edges(&root, &paths).unwrap();

    assert!(edges
        .iter()
        .any(|edge| edge.from == "src/lib.rs" && edge.to == "src/util.rs"));
    assert!(
        edges.iter().all(|edge| edge.from != "src/secret.rs"),
        "files outside the given list are never read"
    );
}
