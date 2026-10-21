use clap::{Parser, Subcommand, ValueHint};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "bpx-rag")]
#[command(about = "Local-first hybrid retrieval for code and project knowledge")]
#[command(version)]
pub struct Cli {
    /// Write a default config file if one does not already exist.
    #[arg(long)]
    pub init: bool,
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Start the MCP server.
    Serve,
    /// Search indexed project knowledge and return cited evidence chunks.
    Search {
        query: String,
        #[arg(short, long)]
        source: Option<String>,
        /// Metadata filter, e.g. `language:rust type:code`.
        #[arg(long)]
        filter: Option<String>,
        #[arg(short, long, default_value = "10")]
        limit: u32,
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        explain: bool,
        /// Return result count and token estimate without snippets.
        #[arg(long)]
        count_only: bool,
        /// Disable reuse of search memory for explicit retrieval profiles.
        #[arg(long)]
        no_memory: bool,
    },
    /// Manage persistent sources in the local knowledge index.
    Knowledge {
        #[command(subcommand)]
        action: KnowledgeAction,
    },
    /// Inspect or explicitly download the configured embedding model.
    Models {
        #[command(subcommand)]
        action: ModelsAction,
    },
    /// Evaluate retrieval against the checked-in judged corpus using a cached model.
    Evaluate {
        #[command(subcommand)]
        action: EvalAction,
    },
    /// Analyze direct imports, includes, and links without using the RAG index.
    AnalyzeFile {
        #[arg(value_hint = ValueHint::FilePath)]
        path: PathBuf,
        #[arg(long, value_hint = ValueHint::DirPath)]
        root: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
pub enum KnowledgeAction {
    /// Register and initially index a project or documents directory.
    Add {
        #[arg(value_hint = ValueHint::DirPath)]
        path: PathBuf,
        #[arg(long)]
        label: String,
        #[arg(long)]
        glob: Option<String>,
        #[arg(long, value_delimiter = ',')]
        enrichments: Vec<String>,
    },
    /// Incrementally re-index changed files and remove deleted files.
    Sync {
        #[arg(long)]
        label: Option<String>,
    },
    /// List sources, file/chunk counts, and embedding coverage.
    Status,
    /// Remove one source and its indexed chunks.
    Remove { label: String },
    /// Backfill missing embeddings after installing the configured model.
    EmbedBackfill,
    /// Read indexed chunks for a source in original order.
    Read {
        label: String,
        #[arg(long, default_value = "20")]
        max_chunks: usize,
        /// Restrict readback to a source-relative file path.
        #[arg(long)]
        path: Option<String>,
        /// Include chunks overlapping this 1-based starting line.
        #[arg(long)]
        line_start: Option<u32>,
        /// Include chunks overlapping this 1-based ending line.
        #[arg(long)]
        line_end: Option<u32>,
    },
}

#[derive(Subcommand)]
pub enum ModelsAction {
    /// Show model configuration and whether all local model files are present.
    Status,
    /// Download the configured embedding model explicitly.
    Download,
}

#[derive(Subcommand)]
pub enum EvalAction {
    /// Run the real-model retrieval benchmark. This never downloads a model.
    Retrieval {
        #[arg(long, default_value = "3")]
        repetitions: usize,
    },
    /// Measure SQLite vector-search scaling with deterministic synthetic vectors.
    Scaling {
        #[arg(long, default_value = "5")]
        repetitions: usize,
    },
}
