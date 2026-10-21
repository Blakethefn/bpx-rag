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
    /// Evaluate retrieval against a judged dataset using a cached model.
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
    EmbedBackfill {
        /// Restrict backfill to one registered source label. Omit to process all sources.
        #[arg(long)]
        label: Option<String>,
    },
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
        /// RRF multiplier for lexical candidates (BM25, trigram, fuzzy).
        /// Must be in (0, 10]; defaults to 1.0.
        #[arg(long, default_value_t = 1.0)]
        keyword_weight: f64,
        /// RRF multiplier for semantic vector candidates.
        /// Must be in (0, 10]; defaults to 1.0.
        #[arg(long, default_value_t = 1.0)]
        vector_weight: f64,
        /// Directory containing queries.jsonl and qrels.jsonl. Defaults to the bundled fixture.
        #[arg(long, value_hint = ValueHint::DirPath)]
        dataset: Option<PathBuf>,
        /// Source tree to index. Defaults to <dataset>/corpus.
        #[arg(long, value_hint = ValueHint::DirPath)]
        corpus_root: Option<PathBuf>,
    },
    /// Measure SQLite vector-search scaling with deterministic synthetic vectors.
    Scaling {
        #[arg(long, default_value = "5")]
        repetitions: usize,
    },
    /// Measure cached-model embedding throughput on production-sized source chunks.
    Embeddings {
        #[arg(long, value_hint = ValueHint::DirPath)]
        corpus_root: PathBuf,
        /// ONNX intra-op CPU thread counts to compare.
        /// Defaults to roughly one-third, two-thirds, and all available CPUs.
        #[arg(long, value_delimiter = ',')]
        threads: Option<Vec<usize>>,
        /// FastEmbed batch sizes to compare.
        #[arg(long, value_delimiter = ',', default_value = "16,32")]
        batch_sizes: Vec<usize>,
        /// Repeated embedding passes per thread/batch combination.
        #[arg(long, default_value = "2")]
        repetitions: usize,
        /// Maximum corpus-wide sampled chunks to embed per run.
        #[arg(long, default_value = "32")]
        max_chunks: usize,
    },
    /// Build a blinded pool of lexical, vector, and hybrid file candidates for human relevance grading.
    Pool {
        /// Dataset directory containing queries.jsonl. Defaults to the bundled fixture.
        #[arg(long, value_hint = ValueHint::DirPath)]
        dataset: Option<PathBuf>,
        /// Corpus tree to index. Defaults to <dataset>/corpus.
        #[arg(long, value_hint = ValueHint::DirPath)]
        corpus_root: Option<PathBuf>,
        /// New JSON file for annotators; snippets and relative paths are included.
        #[arg(long, value_hint = ValueHint::FilePath)]
        output: PathBuf,
        /// New JSON file containing the separate retrieval-rank reveal key.
        #[arg(long, value_hint = ValueHint::FilePath)]
        key_output: PathBuf,
        /// Distinct files to retrieve per search channel before pooling.
        #[arg(long, default_value = "20")]
        depth: usize,
        /// Stable shuffle seed for blinded IDs and candidate order.
        #[arg(long, default_value = "20261021")]
        seed: u64,
    },
    /// Import completed pool grades into the evaluator's JSONL qrels format.
    ImportPool {
        #[arg(long, value_hint = ValueHint::FilePath)]
        pool: PathBuf,
        #[arg(long, value_hint = ValueHint::FilePath)]
        key: PathBuf,
        #[arg(long, value_hint = ValueHint::DirPath)]
        dataset: PathBuf,
        #[arg(long, value_hint = ValueHint::DirPath)]
        corpus_root: PathBuf,
        /// New output qrels file. Existing files are never overwritten.
        #[arg(long, value_hint = ValueHint::FilePath)]
        output: PathBuf,
    },
}
