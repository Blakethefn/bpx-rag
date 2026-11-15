use super::*;

#[test]
fn sync_updates_path_membership_after_edit_delete_rename_and_move() {
    let store = make_store();
    let root = tempdir().unwrap();
    let original = root.path().join("docs/original.md");
    std::fs::create_dir_all(original.parent().unwrap()).unwrap();
    std::fs::write(&original, "# Edit marker\n\noldquasar content.").unwrap();
    store
        .add_source("repo", root.path().to_str().unwrap(), &[], &[])
        .unwrap();
    let sync_repo = || {
        let source = store.get_source("repo").unwrap().unwrap();
        sync::sync_source(&store, &source, None).unwrap();
    };
    sync_repo();
    assert_eq!(search(&store, "oldquasar", "dir:docs").len(), 1);

    std::fs::write(&original, "# Edit marker\n\nnewnebula content.").unwrap();
    sync_repo();
    assert!(search(&store, "oldquasar", "file:docs/original.md").is_empty());
    assert_eq!(
        search(&store, "newnebula", "file:docs/original.md").len(),
        1
    );

    let renamed = root.path().join("docs/renamed.md");
    std::fs::rename(&original, &renamed).unwrap();
    sync_repo();
    assert!(search(&store, "newnebula", "file:docs/original.md").is_empty());
    assert_eq!(search(&store, "newnebula", "file:docs/renamed.md").len(), 1);

    let moved = root.path().join("archive/renamed.md");
    std::fs::create_dir_all(moved.parent().unwrap()).unwrap();
    std::fs::rename(&renamed, &moved).unwrap();
    sync_repo();
    assert!(search(&store, "newnebula", "dir:docs").is_empty());
    assert_eq!(search(&store, "newnebula", "dir:archive").len(), 1);

    std::fs::remove_file(moved).unwrap();
    sync_repo();
    assert!(search(&store, "newnebula", "dir:archive").is_empty());
}
