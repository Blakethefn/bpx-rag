use clap::{Args, ValueHint};
use std::path::PathBuf;

use super::super::{DEFAULT_POOL_DEPTH, DEFAULT_POOL_SEED};

#[derive(Debug, Args)]
pub struct PoolCreateArgs {
    #[arg(help = "Dataset directory containing queries.jsonl. Defaults to the bundled fixture")]
    #[arg(long, value_hint = ValueHint::DirPath)]
    pub dataset: Option<PathBuf>,
    #[arg(help = "Corpus tree to index. Defaults to <dataset>/corpus")]
    #[arg(long, value_hint = ValueHint::DirPath)]
    pub corpus_root: Option<PathBuf>,
    #[arg(help = "New JSON file for annotators. Refuses to overwrite an existing file")]
    #[arg(long, value_hint = ValueHint::FilePath)]
    pub output: PathBuf,
    #[arg(
        help = "New JSON file for the retrieval-rank reveal key. Refuses to overwrite an existing file"
    )]
    #[arg(long, value_hint = ValueHint::FilePath)]
    pub key_output: PathBuf,
    #[arg(help = "Distinct files to retrieve per search channel before pooling")]
    #[arg(long, default_value_t = DEFAULT_POOL_DEPTH)]
    pub depth: usize,
    #[arg(help = "Stable shuffle seed for blinded IDs and candidate order")]
    #[arg(long, default_value_t = DEFAULT_POOL_SEED)]
    pub seed: u64,
}

#[derive(Debug, Args)]
pub struct PoolImportArgs {
    #[arg(long, value_hint = ValueHint::FilePath)]
    pub pool: PathBuf,
    #[arg(long, value_hint = ValueHint::FilePath)]
    pub key: PathBuf,
    #[arg(long, value_hint = ValueHint::DirPath)]
    pub dataset: PathBuf,
    #[arg(long, value_hint = ValueHint::DirPath)]
    pub corpus_root: PathBuf,
    #[arg(help = "New qrels file. Refuses to overwrite an existing file")]
    #[arg(long, value_hint = ValueHint::FilePath)]
    pub output: PathBuf,
}
