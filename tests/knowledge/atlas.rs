use std::fs;
use std::path::PathBuf;
use std::process::Output;
use tempfile::{tempdir, TempDir};

use super::harness::run_cli;

pub(crate) const COBALT: &str =
    "# Atlas Notes\n\nThe cobalt lantern protocol governs night navigation across the outer reef.\n";

pub(crate) const AMETHYST: &str =
    "# Atlas Notes\n\nThe amethyst compass mechanism guides ships through foggy waters.\n";

pub(crate) struct Scene {
    _temp: TempDir,
    pub(crate) project: PathBuf,
    pub(crate) source: PathBuf,
    pub(crate) config: PathBuf,
    pub(crate) data: PathBuf,
    pub(crate) cache: PathBuf,
    pub(crate) model: PathBuf,
}

pub(crate) fn scene() -> Scene {
    let temp = tempdir().unwrap();
    let project = temp.path().join("project");
    let source = temp.path().join("docs");
    let data = temp.path().join("data");
    fs::create_dir_all(&project).unwrap();
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("guide.md"), COBALT).unwrap();
    Scene {
        model: data.join("bpx-rag/models/jina-embeddings-v2-base-code"),
        project,
        source,
        config: temp.path().join("config"),
        data,
        cache: temp.path().join("cache"),
        _temp: temp,
    }
}

impl Scene {
    pub(crate) fn run(&self, args: &[&str]) -> Output {
        run_cli(&self.project, &self.config, &self.data, &self.cache, args)
    }
}
