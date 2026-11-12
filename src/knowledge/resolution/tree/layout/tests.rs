use super::super::{ModRef, Symbol, Tree};
use super::module_base;

#[test]
fn directory_module_stems_keep_the_module_base_beside_the_file() {
    assert_eq!(module_base("src/lib.rs"), "src");
    assert_eq!(module_base("src/main.rs"), "src");
    assert_eq!(module_base("src/mod.rs"), "src");
    assert_eq!(module_base("lib.rs"), "");
    assert_eq!(module_base("src/foo.rs"), "src/foo");
    assert_eq!(module_base("src/foo/mod.rs"), "src/foo");
    assert_eq!(module_base("foo.rs"), "foo");
    assert_eq!(module_base("src/Lib.rs"), "src/Lib");
}

#[test]
fn directory_module_files_declare_children_beside_them() {
    let tree = Tree::build(
        vec![
            (1, "src/lib.rs".into()),
            (2, "src/main.rs".into()),
            (3, "src/mod.rs".into()),
            (4, "src/child.rs".into()),
            (5, "src/child/mod.rs".into()),
            (6, "src/lib/child.rs".into()),
            (7, "src/foo.rs".into()),
            (8, "src/foo/child.rs".into()),
            (9, "src/foo/nested/child.rs".into()),
            (10, "src/bar/mod.rs".into()),
            (11, "src/bar/child.rs".into()),
            (12, "src/bar/mod/child.rs".into()),
        ],
        Vec::<Symbol>::new(),
        Vec::<ModRef>::new(),
        Vec::new(),
    );

    for parent in [1_i64, 2, 3] {
        assert!(tree.path_declares_child(parent, 4), "{parent}");
        assert!(tree.path_declares_child(parent, 5), "{parent}");
        assert!(!tree.path_declares_child(parent, 3), "{parent}");
        assert!(!tree.path_declares_child(parent, 6), "{parent}");
        assert!(!tree.path_declares_child(parent, 8), "{parent}");
    }
    assert!(!tree.path_declares_child(1, 1));
    assert!(!tree.path_declares_child(7, 4));
    assert!(tree.path_declares_child(7, 8));
    assert!(tree.path_declares_child(7, 9));
    assert!(tree.path_declares_child(10, 11));
    assert!(!tree.path_declares_child(10, 12));
    assert!(!tree.path_declares_child(10, 10));
}
