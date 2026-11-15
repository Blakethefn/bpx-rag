use std::fs;

use super::findings::{layout, stderr};
use super::harness::{run_cli, stdout};

#[test]
fn findings_returns_partial_analysis_for_a_mixed_source() {
    let home = layout();
    let mixed = home.root.join("mixed");
    fs::create_dir_all(mixed.join("src")).unwrap();
    fs::write(
        mixed.join("src/main.rs"),
        "mod util;\nmod other;\nfn main() {}\n",
    )
    .unwrap();
    fs::write(mixed.join("src/util.rs"), "fn hidden() {}\n").unwrap();
    fs::write(mixed.join("src/other.rs"), "fn broken( {\n").unwrap();

    let added_mixed = run_cli(
        &home.project,
        &home.config,
        &home.data,
        &home.cache,
        &["add", mixed.to_str().unwrap(), "--source", "mixed"],
    );
    assert!(added_mixed.status.success(), "{}", stderr(&added_mixed));

    let partial = run_cli(
        &home.project,
        &home.config,
        &home.data,
        &home.cache,
        &["findings", "--source", "mixed"],
    );
    let partial_text = stdout(&partial);
    assert!(partial.status.success(), "{}", stderr(&partial));
    assert!(partial_text.contains("status: partial"), "{partial_text}");
    assert!(
        partial_text
            .contains("possibly_unused_symbol src/util.rs hidden no reference in sealed scope"),
        "{partial_text}"
    );
    assert!(
        partial_text.contains("src/other.rs:unsealed"),
        "{partial_text}"
    );
    assert!(
        partial_text.contains("src/other.rs:parse_error"),
        "{partial_text}"
    );
    assert!(
        !partial_text.contains("unreachable_file src/other.rs"),
        "{partial_text}"
    );
    assert!(
        !partial_text.contains("possibly_unused_symbol src/other.rs"),
        "{partial_text}"
    );
}
