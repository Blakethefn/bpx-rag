use super::super::{analysis, finding_rows, resolutions, sync, unused};
use crate::support::{add_source, make_store};
use std::fs;
use tempfile::tempdir;

#[test]
fn a_type_method_does_not_use_another_types_method_of_the_same_name() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("src/main.rs"),
        "mod a;\nmod b;\nuse a::Foo;\nfn main() { Foo::helper(); }\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("src/a.rs"),
        "pub struct Foo;\nimpl Foo { pub fn helper() {} }\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("src/b.rs"),
        "pub struct Foo;\nimpl Foo { pub fn helper() {} }\n",
    )
    .unwrap();
    add_source(&store, "demo", dir.path());
    sync(&store, "demo");

    let helpers = resolutions(&store, "helper");
    assert!(
        helpers
            .iter()
            .any(|(_, _, path)| path.as_deref() == Some("src/a.rs")),
        "{helpers:?}"
    );
    assert!(
        helpers
            .iter()
            .all(|(_, _, path)| path.as_deref() != Some("src/b.rs")),
        "{helpers:?}"
    );
    let findings = finding_rows(&store);
    assert!(!unused(&findings, "src/a.rs", "helper"), "{findings:?}");
    assert!(unused(&findings, "src/b.rs", "helper"), "{findings:?}");
}

#[test]
fn an_external_path_does_not_use_a_method_in_scope() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("lib.rs"),
        "fn foo() {}\nstruct Bar;\nimpl Bar { fn method(&self) {} }\nfn main() { ::foo::method(); }\n",
    )
    .unwrap();
    add_source(&store, "demo", dir.path());
    sync(&store, "demo");

    let methods = resolutions(&store, "method");
    assert!(
        methods
            .iter()
            .all(|(status, symbol, _)| status == "unresolved" && symbol.is_none()),
        "{methods:?}"
    );
    assert!(
        unused(&finding_rows(&store), "lib.rs", "method"),
        "{:?}",
        finding_rows(&store)
    );
}
