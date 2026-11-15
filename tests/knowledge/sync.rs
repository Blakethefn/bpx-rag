use std::fs;

use super::atlas::{scene, AMETHYST};
use super::harness::stdout;

#[test]
fn knowledge_sync_works_without_auto_downloading_model() {
    let scene = scene();
    let add = scene.run(&["add", scene.source.to_str().unwrap(), "--source", "atlas"]);
    assert!(
        add.status.success(),
        "{}",
        String::from_utf8_lossy(&add.stderr)
    );
    fs::write(scene.source.join("guide.md"), AMETHYST).unwrap();

    let sync = scene.run(&["sync", "--label", "atlas"]);
    assert!(
        sync.status.success(),
        "{}",
        String::from_utf8_lossy(&sync.stderr)
    );
    assert!(stdout(&sync).contains("updated 1"), "{}", stdout(&sync));

    let updated_search = scene.run(&["search", "amethyst compass mechanism", "--source", "atlas"]);
    assert!(updated_search.status.success());
    assert!(stdout(&updated_search).contains("guide.md"));
    assert!(stdout(&updated_search).contains("amethyst compass"));
    assert!(
        !scene.model.exists(),
        "RAG operations must leave download explicit"
    );
}
