use std::path::Path;
use std::process::{Command, Output};

pub fn run_cli(
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
        .env_remove("HF_HOME")
        .output()
        .unwrap()
}

pub fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}
