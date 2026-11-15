use std::fs;
use std::path::Path;
use std::process::Output;
use tempfile::tempdir;

#[path = "../harness.rs"]
mod harness;

mod bounds;
mod mcp;
mod path;

fn write_corpus(source: &Path) {
    let filler = "lorem ipsum dolor sit amet ".repeat(35);
    let big: String = (0..16)
        .map(|index| {
            format!("Section {index}. The kestrel route is marked. kestrel kestrel kestrel. {filler}\n\n")
        })
        .collect();
    fs::write(source.join("big.md"), big).unwrap();

    let small = format!(
        "# Notes\n\n{}kestrel appears once here.\n",
        "plain text ".repeat(250)
    );
    fs::write(source.join("small.md"), small).unwrap();
}

struct Fixture {
    _temp: tempfile::TempDir,
    project: std::path::PathBuf,
    config_home: std::path::PathBuf,
    data_home: std::path::PathBuf,
    cache_home: std::path::PathBuf,
}

fn indexed_fixture() -> Fixture {
    let temp = tempdir().unwrap();
    let project = temp.path().join("project");
    let source = temp.path().join("docs");
    let config_home = temp.path().join("config");
    let data_home = temp.path().join("data");
    let cache_home = temp.path().join("cache");
    fs::create_dir_all(&project).unwrap();
    fs::create_dir_all(&source).unwrap();
    write_corpus(&source);

    let add = harness::run_cli(
        &project,
        &config_home,
        &data_home,
        &cache_home,
        &["add", source.to_str().unwrap(), "--label", "docs"],
    );
    assert!(
        add.status.success(),
        "{}",
        String::from_utf8_lossy(&add.stderr)
    );

    Fixture {
        _temp: temp,
        project,
        config_home,
        data_home,
        cache_home,
    }
}

fn search(fixture: &Fixture, extra: &[&str]) -> Output {
    let mut args = vec!["search", "kestrel", "--source", "docs", "--limit", "3"];
    args.extend_from_slice(extra);
    harness::run_cli(
        &fixture.project,
        &fixture.config_home,
        &fixture.data_home,
        &fixture.cache_home,
        &args,
    )
}

#[test]
fn default_search_output_is_identical_to_explicit_max_per_file_one() {
    let fixture = indexed_fixture();

    let default = search(&fixture, &[]);
    let explicit = search(&fixture, &["--max-per-file", "1"]);

    assert!(
        default.status.success(),
        "{}",
        String::from_utf8_lossy(&default.stderr)
    );
    assert!(
        explicit.status.success(),
        "{}",
        String::from_utf8_lossy(&explicit.stderr)
    );
    assert_eq!(harness::stdout(&default), harness::stdout(&explicit));
    assert!(
        harness::stdout(&default).contains("file: small.md"),
        "one passage per file keeps the other file: {}",
        harness::stdout(&default)
    );
}

#[test]
fn max_per_file_three_returns_repeated_passages_from_one_file() {
    let fixture = indexed_fixture();

    let output = search(&fixture, &["--max-per-file", "3"]);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = harness::stdout(&output);
    assert_eq!(text.matches("file: big.md").count(), 3, "{text}");
    assert!(!text.contains("file: small.md"), "{text}");
}
