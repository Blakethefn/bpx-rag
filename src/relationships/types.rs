use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Serialize)]
pub struct FileRelationship {
    pub from: String,
    pub to: Option<String>,
    pub kind: String,
    pub specifier: String,
    pub line: usize,
    pub direction: String,
}

#[derive(Debug, Serialize)]
pub struct FileAnalysis {
    pub root: String,
    pub file: String,
    pub scanned_files: usize,
    pub outgoing: Vec<FileRelationship>,
    pub incoming: Vec<FileRelationship>,
    pub warnings: Vec<String>,
}

pub(super) struct SourceFile {
    pub(super) path: PathBuf,
    pub(super) content: String,
}

pub(super) struct RawReference {
    pub(super) kind: &'static str,
    pub(super) specifier: String,
    pub(super) line: usize,
    pub(super) target: Option<PathBuf>,
}
