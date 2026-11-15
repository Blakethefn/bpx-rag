use super::sha256_hex;
use anyhow::{Context, Result};
use std::io::Read;
use std::path::Path;
use std::{fs, io};

pub(in crate::knowledge::sync) fn read_classified_file(
    abs_path: &Path,
    expected_hash: &str,
) -> Result<String> {
    let bytes =
        fs::read(abs_path).with_context(|| format!("failed to read {}", abs_path.display()))?;
    let content = String::from_utf8(bytes)
        .with_context(|| format!("file is not valid UTF-8: {}", abs_path.display()))?;
    if sha256_hex(content.as_bytes()) != expected_hash {
        anyhow::bail!(
            "source file changed before preparation: {}; retry sync",
            abs_path.display()
        );
    }
    Ok(content)
}

pub(in crate::knowledge::sync) fn is_binary(path: &Path) -> io::Result<bool> {
    let mut file = fs::File::open(path)?;
    let mut buf = [0u8; 1024];
    let bytes_read = file.read(&mut buf)?;
    Ok(buf[..bytes_read].contains(&0u8))
}
