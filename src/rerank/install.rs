//! Explicit, opt-in installation of the reranker model. Nothing in this module runs during a search.
//!
//! Every file comes from one pinned Hugging Face revision. Each download is checked against its
//! size and SHA-256 before it is moved into the model directory, so a partial or altered download
//! never reaches the loader.

use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Hugging Face repository of the reranker. Apache-2.0.
pub const REPOSITORY: &str = "jinaai/jina-reranker-v1-turbo-en";
/// The repository commit that every artifact is pinned to.
pub const REVISION: &str = "b8c14f4e723d9e0aab4732a7b7b93741eeeb77c2";
const BASE_URL: &str = "https://huggingface.co";
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(900);
const COPY_BUFFER_BYTES: usize = 64 * 1024;

/// One file of the model, pinned by size and SHA-256.
#[derive(Debug, PartialEq, Eq)]
pub struct Artifact {
    /// Path inside the repository, which is also the path inside the model directory.
    pub path: &'static str,
    pub size: u64,
    pub sha256: &'static str,
}

/// The full-precision model and its tokenizer files. The quantized variants in the repository are not used.
pub const ARTIFACTS: [Artifact; 5] = [
    Artifact {
        path: "onnx/model.onnx",
        size: 151_296_975,
        sha256: "c1296c66c119de645fa9cdee536d8637740efe85224cfa270281e50f213aa565",
    },
    Artifact {
        path: "tokenizer.json",
        size: 2_030_772,
        sha256: "0046da43cc8c424b317f56b092b0512aaaa65c4f925d2f16af9d9eeb4d0ef902",
    },
    Artifact {
        path: "config.json",
        size: 1_206,
        sha256: "e050ff6a15ae9295e84882fa0e98051bd8754856cd5201395ebf00ce9f2d609b",
    },
    Artifact {
        path: "special_tokens_map.json",
        size: 280,
        sha256: "06e405a36dfe4b9604f484f6a1e619af1a7f7d09e34a8555eb0b77b66318067f",
    },
    Artifact {
        path: "tokenizer_config.json",
        size: 1_215,
        sha256: "d291c6652d96d56ffdbcf1ea19d9bae5ed79003f7648c627e725a619227ce8fa",
    },
];

/// What `install` did for one artifact.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The file was already present and matched its pinned hash.
    AlreadyInstalled,
    /// The file was downloaded and verified.
    Downloaded,
}

/// Download and verify each artifact that is not already installed. Returns one outcome per
/// artifact, in `ARTIFACTS` order. Fails on the first artifact that cannot be fetched or verified.
pub fn install(model_dir: &Path) -> Result<Vec<(&'static str, Outcome)>> {
    fs::create_dir_all(model_dir.join("onnx"))
        .with_context(|| format!("failed to create {}", model_dir.display()))?;
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(DOWNLOAD_TIMEOUT))
        .https_only(true)
        .build()
        .into();
    ARTIFACTS
        .iter()
        .map(|artifact| {
            let target = model_dir.join(artifact.path);
            if verify(&target, artifact).is_ok() {
                return Ok((artifact.path, Outcome::AlreadyInstalled));
            }
            fetch(&agent, artifact, &target)?;
            Ok((artifact.path, Outcome::Downloaded))
        })
        .collect()
}

/// Check one installed file against its pinned size and hash. Reads the whole file.
pub fn verify(path: &Path, artifact: &Artifact) -> Result<()> {
    let mut file =
        File::open(path).with_context(|| format!("{} is not readable", path.display()))?;
    let mut hasher = Sha256::new();
    let mut total: u64 = 0;
    let mut buffer = vec![0u8; COPY_BUFFER_BYTES];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        total += read as u64;
    }
    check(total, &hex(hasher.finalize()), artifact)
}

fn fetch(agent: &ureq::Agent, artifact: &Artifact, target: &Path) -> Result<()> {
    let url = format!(
        "{BASE_URL}/{REPOSITORY}/resolve/{REVISION}/{}",
        artifact.path
    );
    let partial = partial_path(target);
    let result = stream_verified(agent, &url, artifact, &partial).and_then(|()| {
        fs::rename(&partial, target).context("failed to move the verified file into place")
    });
    if result.is_err() {
        let _ = fs::remove_file(&partial);
    }
    result
}

/// Stream the response to `partial`, hashing as it goes. The file is checked before it is kept.
fn stream_verified(
    agent: &ureq::Agent,
    url: &str,
    artifact: &Artifact,
    partial: &Path,
) -> Result<()> {
    let mut response = agent
        .get(url)
        .call()
        .with_context(|| format!("failed to download {}", artifact.path))?;
    // One byte past the pinned size, so an oversized response is caught by the size check.
    let mut reader = response.body_mut().as_reader().take(artifact.size + 1);
    let mut file = File::create(partial)?;
    let mut hasher = Sha256::new();
    let mut total: u64 = 0;
    let mut buffer = vec![0u8; COPY_BUFFER_BYTES];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        file.write_all(&buffer[..read])?;
        total += read as u64;
    }
    file.sync_all()?;
    check(total, &hex(hasher.finalize()), artifact)
}

fn check(size: u64, sha256: &str, artifact: &Artifact) -> Result<()> {
    if size != artifact.size {
        bail!(
            "{} has {size} bytes, expected {}",
            artifact.path,
            artifact.size
        );
    }
    if sha256 != artifact.sha256 {
        bail!("{} failed its SHA-256 check", artifact.path);
    }
    Ok(())
}

fn partial_path(target: &Path) -> PathBuf {
    let mut name = target.as_os_str().to_owned();
    name.push(".part");
    PathBuf::from(name)
}

fn hex(digest: impl AsRef<[u8]>) -> String {
    digest
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artifacts_cover_the_files_the_loader_reads() {
        let paths: Vec<&str> = ARTIFACTS.iter().map(|artifact| artifact.path).collect();
        assert!(paths.contains(&super::super::onnx::MODEL_FILE));
        for name in [
            "tokenizer.json",
            "config.json",
            "special_tokens_map.json",
            "tokenizer_config.json",
        ] {
            assert!(paths.contains(&name), "{name} is not pinned");
        }
    }

    #[test]
    fn revision_is_a_full_commit_hash() {
        assert_eq!(REVISION.len(), 40);
        assert!(REVISION.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn verify_accepts_a_matching_file_and_rejects_a_changed_one() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, b"{}").unwrap();
        let artifact = Artifact {
            path: "config.json",
            size: 2,
            sha256: "44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a",
        };
        assert!(verify(&path, &artifact).is_ok());

        std::fs::write(&path, b"{ }").unwrap();
        assert!(verify(&path, &artifact).is_err());
    }

    #[test]
    fn check_rejects_wrong_size_before_hash() {
        let artifact = &ARTIFACTS[2];
        let error = check(artifact.size + 1, artifact.sha256, artifact).unwrap_err();
        assert!(error.to_string().contains("bytes"), "{error}");
    }

    #[test]
    fn partial_path_appends_a_suffix() {
        assert_eq!(
            partial_path(Path::new("/m/onnx/model.onnx")),
            PathBuf::from("/m/onnx/model.onnx.part")
        );
    }
}
