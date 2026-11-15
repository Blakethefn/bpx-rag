use super::{analysis, finding_rows, resolutions, sync, unused};
use crate::support::{add_source, make_store};
use std::fs;
use tempfile::tempdir;

mod external;

#[test]
fn a_type_path_uses_that_types_method_and_not_a_free_function() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("src/main.rs"),
        "mod other;\nstruct Foo;\nimpl Foo { fn helper() {} }\nfn main() { Foo::helper(); }\n",
    )
    .unwrap();
    fs::write(dir.path().join("src/other.rs"), "fn helper() {}\n").unwrap();
    add_source(&store, "demo", dir.path());
    sync(&store, "demo");

    let helpers = resolutions(&store, "helper");
    assert!(
        helpers.iter().any(|(status, symbol, path)| {
            status == "resolved"
                && symbol.as_deref() == Some("helper")
                && path.as_deref() == Some("src/main.rs")
        }),
        "{helpers:?}"
    );
    assert!(
        helpers
            .iter()
            .all(|(_, _, path)| path.as_deref() != Some("src/other.rs")),
        "{helpers:?}"
    );
    let findings = finding_rows(&store);
    assert!(unused(&findings, "src/other.rs", "helper"), "{findings:?}");
    assert!(!unused(&findings, "src/main.rs", "helper"), "{findings:?}");
}

#[test]
fn an_unknown_receiver_does_not_use_a_free_function() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("lib.rs"),
        "struct Foo;\nimpl Foo { fn helper(&self) {} }\nfn helper() {}\nfn main() { Self::helper(); }\n",
    )
    .unwrap();
    add_source(&store, "demo", dir.path());
    sync(&store, "demo");

    let helpers = resolutions(&store, "helper");
    assert!(
        helpers.iter().any(|(status, _, path)| {
            status == "ambiguous" && path.as_deref() == Some("lib.rs")
        }),
        "{helpers:?}"
    );
    let findings = finding_rows(&store);
    assert!(unused(&findings, "lib.rs", "helper"), "{findings:?}");
}

#[test]
fn an_inline_module_shares_the_file_and_records_the_limitation() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("main.rs"),
        "mod inline { fn hidden() {} }\nfn main() { hidden(); }\n",
    )
    .unwrap();
    add_source(&store, "demo", dir.path());
    sync(&store, "demo");

    let (_, _, _, gaps) = analysis(&store, "demo");
    assert!(gaps.contains("limitation:inline_modules"), "{gaps}");
    assert!(
        !unused(&finding_rows(&store), "main.rs", "hidden"),
        "{:?}",
        finding_rows(&store)
    );
}

#[test]
fn a_macro_records_a_limitation_and_still_publishes_findings() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("main.rs"),
        "macro_rules! demo { () => {}; }\nfn main() {}\nfn kept() {}\n",
    )
    .unwrap();
    add_source(&store, "demo", dir.path());
    sync(&store, "demo");

    let (status, _, _, gaps) = analysis(&store, "demo");
    assert_eq!(status, "complete");
    assert!(gaps.contains("limitation:macros_not_expanded"), "{gaps}");
    assert!(
        unused(&finding_rows(&store), "main.rs", "kept"),
        "{:?}",
        finding_rows(&store)
    );
}

#[test]
fn two_imports_of_the_same_symbol_are_ambiguous() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("src/main.rs"),
        "mod a;\nuse crate::a::helper as h;\nuse crate::a::helper as h;\nfn main() { h(); }\n",
    )
    .unwrap();
    fs::write(dir.path().join("src/a.rs"), "pub(crate) fn helper() {}\n").unwrap();
    add_source(&store, "demo", dir.path());
    sync(&store, "demo");

    let names = resolutions(&store, "h");
    assert!(
        names.iter().any(|(status, path_symbol, path)| {
            status == "ambiguous"
                && path_symbol.as_deref() == Some("helper")
                && path.as_deref() == Some("src/a.rs")
        }),
        "{names:?}"
    );
    assert!(!unused(&finding_rows(&store), "src/a.rs", "helper"));
}
