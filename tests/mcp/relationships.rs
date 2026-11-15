use std::process::Command;
use tempfile::tempdir;

#[test]
fn cli_analyze_file_scans_current_files_without_creating_runtime_state() {
    let project = tempdir().unwrap();
    let config_dir = project.path().join("runtime-config");
    let data_dir = project.path().join("runtime-data");
    let cache_dir = project.path().join("runtime-cache");
    std::fs::create_dir_all(project.path().join("src/api")).unwrap();
    std::fs::write(
        project.path().join("src/api/quotes.ts"),
        "export function getQuotes() {}\n",
    )
    .unwrap();
    std::fs::write(
        project.path().join("src/useQuotes.ts"),
        "import { getQuotes } from './api/quotes';\n",
    )
    .unwrap();
    let executable = std::env::var("CARGO_BIN_EXE_bpx-rag").unwrap();

    let output = Command::new(executable)
        .args([
            "relate",
            "src/api/quotes.ts",
            "--root",
            project.path().to_str().unwrap(),
        ])
        .current_dir(project.path())
        .env("XDG_CONFIG_HOME", &config_dir)
        .env("XDG_DATA_HOME", &data_dir)
        .env("XDG_CACHE_HOME", &cache_dir)
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Incoming (1)"));
    assert!(stdout.contains("src/useQuotes.ts"));
    assert!(
        !data_dir.exists(),
        "CLI relationship scan must not create runtime data"
    );
    assert!(
        !config_dir.exists(),
        "CLI analyzer must not create config files"
    );
    assert!(
        !cache_dir.exists(),
        "CLI analyzer must not create cache files"
    );
}

#[test]
fn cli_exposes_rag_commands_alongside_file_analysis_and_mcp_server() {
    let executable = std::env::var("CARGO_BIN_EXE_bpx-rag").unwrap();
    let output = Command::new(executable).arg("--help").output().unwrap();

    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).unwrap();
    assert!(help.contains("relate"));
    assert!(help.contains("serve"));
    assert!(help.contains("init"));
    assert!(help.contains("search"));
    assert!(help.contains("sync"));
    assert!(help.contains("models"));
    assert!(help.contains("relate"));
    assert!(help.contains("serve"));
}
