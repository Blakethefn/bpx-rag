use super::harness::stdout;
use super::{indexed_fixture, search};

#[test]
fn max_per_file_zero_is_rejected_by_the_cli() {
    let fixture = indexed_fixture();

    let output = search(&fixture, &["--max-per-file", "0"]);

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("max-per-file"), "{stderr}");
}

#[test]
fn max_per_file_boundaries_are_enforced_by_the_cli() {
    let fixture = indexed_fixture();

    let accepted_low = search(&fixture, &["--max-per-file", "1"]);
    let accepted_high = search(&fixture, &["--max-per-file", "50"]);
    let rejected_high = search(&fixture, &["--max-per-file", "51"]);

    assert!(accepted_low.status.success(), "{}", stdout(&accepted_low));
    assert!(accepted_high.status.success(), "{}", stdout(&accepted_high));
    assert!(!rejected_high.status.success());
    let stderr = String::from_utf8_lossy(&rejected_high.stderr);
    assert!(
        stderr.contains("max-per-file") && stderr.contains("50"),
        "{stderr}"
    );
}
