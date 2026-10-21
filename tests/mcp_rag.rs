use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use tempfile::tempdir;

fn run_cli(
    project_dir: &Path,
    config_home: &Path,
    data_home: &Path,
    cache_home: &Path,
    args: &[&str],
) -> Output {
    Command::new(std::env::var("CARGO_BIN_EXE_bpx-rag").unwrap())
        .args(args)
        .current_dir(project_dir)
        .env("XDG_CONFIG_HOME", config_home)
        .env("XDG_DATA_HOME", data_home)
        .env("XDG_CACHE_HOME", cache_home)
        .output()
        .unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

#[test]
fn knowledge_cli_add_sync_search_and_read_work_without_auto_downloading_model() {
    let temp = tempdir().unwrap();
    let project = temp.path().join("project");
    let source = temp.path().join("docs");
    let config_home = temp.path().join("config");
    let data_home = temp.path().join("data");
    let cache_home = temp.path().join("cache");
    let model_dir = data_home.join("bpcontext/models").join("all-MiniLM-L6-v2");
    fs::create_dir_all(&project).unwrap();
    fs::create_dir_all(&source).unwrap();
    fs::write(
        source.join("guide.md"),
        "# Atlas Notes\n\nThe cobalt lantern protocol governs night navigation across the outer reef.\n",
    )
    .unwrap();

    let status = run_cli(
        &project,
        &config_home,
        &data_home,
        &cache_home,
        &["models", "status"],
    );
    assert!(status.status.success(), "{}", stdout(&status));
    assert!(
        stdout(&status).contains("not installed"),
        "{}",
        stdout(&status)
    );
    assert!(
        !model_dir.exists(),
        "models status must not download the model"
    );

    let add = run_cli(
        &project,
        &config_home,
        &data_home,
        &cache_home,
        &[
            "knowledge",
            "add",
            source.to_str().unwrap(),
            "--label",
            "atlas",
        ],
    );
    assert!(
        add.status.success(),
        "{}",
        String::from_utf8_lossy(&add.stderr)
    );
    assert!(stdout(&add).contains("Source 'atlas'"), "{}", stdout(&add));
    assert!(stdout(&add).contains("added 1"), "{}", stdout(&add));
    assert!(
        !model_dir.exists(),
        "knowledge add must use keyword indexing without downloading"
    );

    let search = run_cli(
        &project,
        &config_home,
        &data_home,
        &cache_home,
        &["search", "cobalt lantern protocol", "--source", "atlas"],
    );
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
    assert!(!model_dir.exists(), "search must not download the model");

    fs::write(
        source.join("guide.md"),
        "# Atlas Notes\n\nThe amethyst compass mechanism guides ships through foggy waters.\n",
    )
    .unwrap();
    let sync = run_cli(
        &project,
        &config_home,
        &data_home,
        &cache_home,
        &["knowledge", "sync", "--label", "atlas"],
    );
    assert!(
        sync.status.success(),
        "{}",
        String::from_utf8_lossy(&sync.stderr)
    );
    assert!(stdout(&sync).contains("updated 1"), "{}", stdout(&sync));

    let read = run_cli(
        &project,
        &config_home,
        &data_home,
        &cache_home,
        &["knowledge", "read", "atlas"],
    );
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

    let focused_read = run_cli(
        &project,
        &config_home,
        &data_home,
        &cache_home,
        &[
            "knowledge",
            "read",
            "atlas",
            "--path",
            "guide.md",
            "--line-start",
            "1",
            "--line-end",
            "20",
        ],
    );
    assert!(focused_read.status.success());
    assert!(stdout(&focused_read).contains("amethyst compass"));

    let unsafe_read = run_cli(
        &project,
        &config_home,
        &data_home,
        &cache_home,
        &["knowledge", "read", "atlas", "--path", "../outside.md"],
    );
    assert!(!unsafe_read.status.success());

    let updated_search = run_cli(
        &project,
        &config_home,
        &data_home,
        &cache_home,
        &["search", "amethyst compass mechanism", "--source", "atlas"],
    );
    assert!(updated_search.status.success());
    assert!(stdout(&updated_search).contains("guide.md"));
    assert!(stdout(&updated_search).contains("amethyst compass"));
    assert!(
        !model_dir.exists(),
        "RAG operations must leave download explicit"
    );
}

#[test]
fn model_download_is_an_explicit_cli_action() {
    let temp = tempdir().unwrap();
    let project = temp.path().join("project");
    fs::create_dir_all(&project).unwrap();
    let config_home = temp.path().join("config");
    let data_home = temp.path().join("data");
    let cache_home = temp.path().join("cache");

    let help = run_cli(
        &project,
        &config_home,
        &data_home,
        &cache_home,
        &["models", "--help"],
    );
    assert!(help.status.success());
    assert!(stdout(&help).contains("download"));
    assert!(stdout(&help).contains("Download the configured embedding model explicitly"));

    let status = run_cli(
        &project,
        &config_home,
        &data_home,
        &cache_home,
        &["models", "status"],
    );
    assert!(status.status.success());
    assert!(stdout(&status).contains("not installed"));
    assert!(!data_home.join("bpcontext/models/all-MiniLM-L6-v2").exists());
}
