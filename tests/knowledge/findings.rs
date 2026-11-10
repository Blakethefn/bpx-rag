use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use tempfile::tempdir;

fn run(project: &Path, config: &Path, data: &Path, cache: &Path, args: &[&str]) -> Output {
    Command::new(std::env::var("CARGO_BIN_EXE_bpx-rag").unwrap())
        .args(args)
        .current_dir(project)
        .env("XDG_CONFIG_HOME", config)
        .env("XDG_DATA_HOME", data)
        .env("XDG_CACHE_HOME", cache)
        .env_remove("HF_HOME")
        .output()
        .unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

#[test]
fn findings_returns_complete_and_incomplete_analysis_and_rejects_a_missing_label() {
    let temp = tempdir().unwrap();
    let project = temp.path().join("project");
    let complete = temp.path().join("complete");
    let broken = temp.path().join("broken");
    let config = temp.path().join("config");
    let data = temp.path().join("data");
    let cache = temp.path().join("cache");
    fs::create_dir_all(&project).unwrap();
    fs::create_dir_all(&complete).unwrap();
    fs::create_dir_all(&broken).unwrap();
    fs::write(complete.join("lib.rs"), "pub fn alpha() {}\n").unwrap();
    fs::write(broken.join("broken.rs"), "pub fn fine() {}\nfn broken( {\n").unwrap();

    let added = run(
        &project,
        &config,
        &data,
        &cache,
        &["add", complete.to_str().unwrap(), "--source", "ready"],
    );
    assert!(added.status.success(), "{}", stderr(&added));

    let one = run(&project, &config, &data, &cache, &["findings"]);
    let one_text = stdout(&one);
    assert!(one.status.success(), "{}", stderr(&one));
    assert!(one_text.contains("source: ready"), "{one_text}");
    assert!(one_text.contains("status: complete"), "{one_text}");
    assert!(one_text.contains("analysis_version:"), "{one_text}");
    assert!(one_text.contains("fingerprint:"), "{one_text}");
    assert!(one_text.contains("findings:"), "{one_text}");

    let added_broken = run(
        &project,
        &config,
        &data,
        &cache,
        &["add", broken.to_str().unwrap(), "--source", "partial"],
    );
    assert!(added_broken.status.success(), "{}", stderr(&added_broken));

    let missing = run(&project, &config, &data, &cache, &["findings"]);
    let missing_text = stderr(&missing);
    assert!(!missing.status.success(), "{}", stdout(&missing));
    assert!(missing_text.contains("ready"), "{missing_text}");
    assert!(missing_text.contains("partial"), "{missing_text}");

    let incomplete = run(
        &project,
        &config,
        &data,
        &cache,
        &["findings", "--source", "partial"],
    );
    let incomplete_text = stdout(&incomplete);
    assert!(incomplete.status.success(), "{}", stderr(&incomplete));
    assert!(
        incomplete_text.contains("source: partial"),
        "{incomplete_text}"
    );
    assert!(
        incomplete_text.contains("status: incomplete"),
        "{incomplete_text}"
    );
    assert!(incomplete_text.contains("broken.rs"), "{incomplete_text}");
    assert!(
        incomplete_text.contains("fingerprint:"),
        "{incomplete_text}"
    );
    assert!(
        !incomplete_text.contains("possibly_unused_symbol"),
        "{incomplete_text}"
    );
    assert!(
        !incomplete_text.contains("unreachable_file"),
        "{incomplete_text}"
    );

    let mixed = temp.path().join("mixed");
    fs::create_dir_all(mixed.join("src")).unwrap();
    fs::write(
        mixed.join("src/main.rs"),
        "mod util;\nmod other;\nfn main() {}\n",
    )
    .unwrap();
    fs::write(mixed.join("src/util.rs"), "fn hidden() {}\n").unwrap();
    fs::write(mixed.join("src/other.rs"), "fn broken( {\n").unwrap();
    let added_mixed = run(
        &project,
        &config,
        &data,
        &cache,
        &["add", mixed.to_str().unwrap(), "--source", "mixed"],
    );
    assert!(added_mixed.status.success(), "{}", stderr(&added_mixed));

    let partial = run(
        &project,
        &config,
        &data,
        &cache,
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
