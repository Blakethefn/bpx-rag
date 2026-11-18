use std::fs::{self, OpenOptions};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static PROBE_ID: AtomicUsize = AtomicUsize::new(0);

pub(super) fn directory(db_path: &Path) -> io::Result<PathBuf> {
    directory_with(db_path, env_directory, probe_writable)
}

fn directory_with(
    db_path: &Path,
    override_dir: impl Fn() -> Option<PathBuf>,
    probe: impl Fn(&Path) -> io::Result<()>,
) -> io::Result<PathBuf> {
    let database_dir = db_path.parent().filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut candidates = Vec::new();
    if let Some(path) = override_dir() {
        candidates.push(path);
    }
    for path in [database_dir.to_path_buf(), std::env::temp_dir()] {
        if !candidates.contains(&path) {
            candidates.push(path);
        }
    }
    let mut last_error = None;
    for candidate in candidates {
        match probe(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or_else(|| io::Error::other("no runtime directory candidates")))
}

fn env_directory() -> Option<PathBuf> {
    std::env::var_os("BPX_RUNTIME_DIR").map(PathBuf::from)
}

fn probe_writable(dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let id = PROBE_ID.fetch_add(1, Ordering::Relaxed);
    let path = dir.join(format!(".bpx-rag-runtime-{}-{id}", std::process::id()));
    OpenOptions::new().write(true).create_new(true).open(&path)?;
    fs::remove_file(path)
}

pub(super) fn permission_denied(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause.downcast_ref::<io::Error>().is_some_and(is_denied)
            || cause.downcast_ref::<notify::Error>().is_some_and(|error| {
                matches!(&error.kind, notify::ErrorKind::Io(error) if is_denied(error))
            })
    })
}

fn is_denied(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::PermissionDenied | io::ErrorKind::ReadOnlyFilesystem
    )
}

pub(super) fn file_path(db_path: &Path, directory: &Path, suffix: &str) -> PathBuf {
    let database_dir = db_path.parent().filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if directory == database_dir {
        let mut name = db_path.as_os_str().to_owned();
        name.push(suffix);
        return PathBuf::from(name);
    }
    let name = db_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    let mut hasher = DefaultHasher::new();
    db_path.to_string_lossy().hash(&mut hasher);
    directory.join(format!("{name}-{:016x}{suffix}", hasher.finish()))
}

#[cfg(test)]
mod tests;
