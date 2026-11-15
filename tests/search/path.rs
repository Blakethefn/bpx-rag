use super::{harness, indexed_fixture, search};

#[test]
fn cli_search_accepts_an_exact_file_scope_filter() {
    let fixture = indexed_fixture();
    let output = search(&fixture, &["--filter", "file:big.md"]);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = harness::stdout(&output);
    assert!(text.contains("file: big.md"), "{text}");
    assert!(!text.contains("file: small.md"), "{text}");
}
