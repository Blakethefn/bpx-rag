use super::atlas::scene;
use super::harness::stdout;

#[test]
fn knowledge_search_works_without_auto_downloading_model() {
    let scene = scene();
    let add = scene.run(&["add", scene.source.to_str().unwrap(), "--source", "atlas"]);
    assert!(
        add.status.success(),
        "{}",
        String::from_utf8_lossy(&add.stderr)
    );

    let search = scene.run(&["search", "cobalt lantern protocol", "--source", "atlas"]);
    assert!(
        search.status.success(),
        "{}",
        String::from_utf8_lossy(&search.stderr)
    );
    assert!(stdout(&search).contains("guide.md"), "{}", stdout(&search));
    assert!(
        stdout(&search).contains("cobalt lantern"),
        "{}",
        stdout(&search)
    );
    assert!(
        stdout(&search).contains("Semantic retrieval is unavailable"),
        "keyword fallback notice missing: {}",
        stdout(&search)
    );
    assert!(!scene.model.exists(), "search must not download the model");
}
