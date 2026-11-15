use super::super::{analysis, finding_rows, resolutions, sync, unused};
use crate::support::{add_source, make_store};
use std::fs;
use tempfile::tempdir;

#[test]
fn a_glob_uses_visible_items_and_an_unresolved_glob_does_not_blank_the_source() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("src/main.rs"),
        "mod a;\nuse a::*;\nuse missing::*;\nfn main() {}\nfn kept() {}\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("src/a.rs"),
        "pub fn helper() {}\nfn hidden() {}\n",
    )
    .unwrap();
    add_source(&store, "demo", dir.path());
    sync(&store, "demo");

    let (status, _, _, _) = analysis(&store, "demo");
    assert_eq!(status, "complete");
    let findings = finding_rows(&store);
    assert!(!unused(&findings, "src/a.rs", "helper"), "{findings:?}");
    assert!(unused(&findings, "src/a.rs", "hidden"), "{findings:?}");
    assert!(unused(&findings, "src/main.rs", "kept"), "{findings:?}");
}

#[test]
fn a_prelude_name_does_not_use_a_function_in_another_module() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("src/main.rs"),
        "mod other;\nfn main() { let _ = Some; }\n",
    )
    .unwrap();
    fs::write(dir.path().join("src/other.rs"), "fn Some() {}\n").unwrap();
    add_source(&store, "demo", dir.path());
    sync(&store, "demo");

    let some = resolutions(&store, "Some");
    assert!(
        some.iter()
            .all(|(status, symbol, _)| status == "unresolved" && symbol.is_none()),
        "{some:?}"
    );
    assert!(
        unused(&finding_rows(&store), "src/other.rs", "Some"),
        "{:?}",
        finding_rows(&store)
    );
}

#[test]
fn two_imports_of_one_local_name_are_ambiguous() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("src/main.rs"),
        "mod a;\nmod b;\nuse crate::a::helper as h;\nuse crate::b::helper as h;\nfn main() { h(); }\n",
    )
    .unwrap();
    fs::write(dir.path().join("src/a.rs"), "pub(crate) fn helper() {}\n").unwrap();
    fs::write(dir.path().join("src/b.rs"), "pub(crate) fn helper() {}\n").unwrap();
    add_source(&store, "demo", dir.path());
    sync(&store, "demo");

    let ambiguous: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM knowledge_resolutions res
             JOIN knowledge_references kr ON kr.id = res.reference_id
             WHERE kr.specifier = 'h' AND kr.kind = 'name' AND res.status = 'ambiguous'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(ambiguous, 2);
    let findings = finding_rows(&store);
    assert!(!unused(&findings, "src/a.rs", "helper"), "{findings:?}");
    assert!(!unused(&findings, "src/b.rs", "helper"), "{findings:?}");
}
