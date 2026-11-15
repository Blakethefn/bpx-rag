use super::atlas::scene;
use super::harness::stdout;

#[test]
fn knowledge_add_works_without_auto_downloading_model() {
    let scene = scene();

    let status = scene.run(&["models", "status"]);
    assert!(status.status.success(), "{}", stdout(&status));
    assert!(
        stdout(&status).contains("not installed"),
        "{}",
        stdout(&status)
    );
    assert!(
        !scene.model.exists(),
        "models status must not download the model"
    );

    let add = scene.run(&["add", scene.source.to_str().unwrap(), "--source", "atlas"]);
    assert!(
        add.status.success(),
        "{}",
        String::from_utf8_lossy(&add.stderr)
    );
    assert!(stdout(&add).contains("Source 'atlas'"), "{}", stdout(&add));
    assert!(stdout(&add).contains("added 1"), "{}", stdout(&add));
    assert!(
        !scene.model.exists(),
        "knowledge add must use keyword indexing without downloading"
    );
}
