use std::fs;
use tempfile::tempdir;

use super::harness::{run_cli, stdout};

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
    assert!(!data_home
        .join("bpx-rag/models/jina-embeddings-v2-base-code")
        .exists());

    let evaluation = run_cli(
        &project,
        &config_home,
        &data_home,
        &cache_home,
        &["eval", "retrieval"],
    );
    assert!(!evaluation.status.success());
    assert!(
        String::from_utf8_lossy(&evaluation.stderr)
            .contains("evaluation requires the configured model to be cached"),
        "{}",
        String::from_utf8_lossy(&evaluation.stderr)
    );
    assert!(!data_home
        .join("bpx-rag/models/jina-embeddings-v2-base-code")
        .exists());
}

#[test]
fn init_writes_a_config_once_and_the_flag_does_not_run_another_command() {
    let temp = tempdir().unwrap();
    let project = temp.path().join("project");
    let config_home = temp.path().join("config");
    let data_home = temp.path().join("data");
    let cache_home = temp.path().join("cache");
    fs::create_dir_all(&project).unwrap();
    let config_path = config_home.join("bpx-rag/config.toml");

    let created = run_cli(&project, &config_home, &data_home, &cache_home, &["init"]);
    assert!(
        created.status.success(),
        "{}",
        String::from_utf8_lossy(&created.stderr)
    );
    assert!(
        stdout(&created).contains("Created default config"),
        "{}",
        stdout(&created)
    );
    assert!(config_path.is_file());

    let marker = "device = \"cpu\"\n# keep this line\n";
    fs::write(&config_path, marker).unwrap();
    let again = run_cli(
        &project,
        &config_home,
        &data_home,
        &cache_home,
        &["--init", "remove", "missing"],
    );
    assert!(
        again.status.success(),
        "{}",
        String::from_utf8_lossy(&again.stderr)
    );
    let text = stdout(&again);
    assert!(text.contains("Config already exists"), "{text}");
    assert!(!text.contains("not found"), "{text}");
    assert_eq!(fs::read_to_string(&config_path).unwrap(), marker);
}
