use clap::{Parser, Subcommand, ValueHint};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "bpx-rag")]
#[command(about = "Filesystem-first file relationship analysis")]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Analyze direct file references without indexing the project
    AnalyzeFile {
        /// File path to analyze
        #[arg(value_hint = ValueHint::FilePath)]
        path: PathBuf,
        /// Project root to scan (defaults to the nearest project marker)
        #[arg(long, value_hint = ValueHint::DirPath)]
        root: Option<PathBuf>,
    },

    /// Start the MCP server
    Serve,
}
