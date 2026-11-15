use std::fs;

use super::atlas::{scene, AMETHYST};
use super::harness::stdout;

#[test]
fn knowledge_read_works_without_auto_downloading_model() {
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

    let read = scene.run(&["read", "atlas"]);
    assert!(
        read.status.success(),
        "{}",
        String::from_utf8_lossy(&read.stderr)
    );
    assert!(stdout(&read).contains("guide.md"), "{}", stdout(&read));
    assert!(
        stdout(&read).contains("amethyst compass"),
        "{}",
        stdout(&read)
    );

    let focused_read = scene.run(&[
        "read",
        "atlas",
        "--path",
        "guide.md",
        "--line-start",
        "1",
        "--line-end",
        "20",
    ]);
    assert!(focused_read.status.success());
    assert!(stdout(&focused_read).contains("amethyst compass"));

    let unsafe_read = scene.run(&["read", "atlas", "--path", "../outside.md"]);
    assert!(!unsafe_read.status.success());
}
