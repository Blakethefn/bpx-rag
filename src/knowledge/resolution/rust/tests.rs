use super::super::paths::PATH_KEYWORDS;
use super::super::tree::Tree;
use super::lookup::{find_path, Found};
use crate::knowledge::open_in_memory;

fn empty_tree() -> Tree {
    let store = open_in_memory().expect("in-memory knowledge store");
    store
        .conn()
        .execute(
            "INSERT INTO knowledge_sources (label, path) VALUES ('demo', '/tmp/demo')",
            [],
        )
        .expect("source row");
    Tree::load(store.conn(), 1).expect("empty module tree")
}

#[test]
fn path_keywords_fail_and_count_as_entered() {
    let tree = empty_tree();
    for (name, _) in PATH_KEYWORDS {
        assert!(
            matches!(find_path(&tree, None, 1, name, None), Found::Failed),
            "{name}"
        );
        assert!(super::entered_module(&tree, 1, &format!("{name}::item")));
    }
    assert!(matches!(
        find_path(&tree, None, 1, "missing", None),
        Found::External
    ));
    assert!(!super::entered_module(&tree, 1, "missing::item"));
    assert!(!super::entered_module(&tree, 1, "::crate"));
}
