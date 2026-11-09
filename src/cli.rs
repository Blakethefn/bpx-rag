use anyhow::{bail, Result};
use clap::{Args, Parser, Subcommand, ValueHint};
use std::path::PathBuf;

const DEFAULT_POOL_DEPTH: usize = 20;
const DEFAULT_POOL_SEED: u64 = 20261021;

#[derive(Debug, Parser)]
#[command(name = "bpx-rag")]
#[command(about = "Local-first hybrid retrieval for code and project knowledge")]
#[command(version)]
pub struct Cli {
    /// Write a default config file if one does not already exist.
    #[arg(long, hide = true)]
    pub init: bool,
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Write a default config file when none exists, and leave an existing file unchanged.
    ///
    /// Creates the config with the built-in defaults. A file that is already there is left as it
    /// is, and this command prints its path. `--init` is still accepted and does the same thing,
    /// including when another command is written after it: the config is written and that command
    /// is not run.
    Init,
    /// Start the MCP server on stdio.
    Serve,
    /// Search registered sources and return cited evidence chunks.
    Search {
        query: String,
        /// Restrict to one registered source.
        #[arg(short, long, value_name = "SOURCE")]
        source: Option<String>,
        /// Metadata filter, e.g. `language:rust type:code`.
        #[arg(long)]
        filter: Option<String>,
        #[arg(short, long, default_value = "10")]
        limit: u32,
        /// Preferred cap on passages from one file (1 to 50, default 1). Passages from one file
        /// are taken first, up to this number. If the other files cannot fill the limit, the
        /// remaining slots take the next-best passages from the same file. The cap is not strict.
        #[arg(long, default_value = "1", value_parser = clap::value_parser!(u32).range(1..=50))]
        max_per_file: u32,
        #[arg(long)]
        profile: Option<String>,
        /// Show what each retrieval layer did. Warns first when vector retrieval is not running,
        /// for example when the config asks for CUDA on a build without `gpu-cuda`.
        #[arg(long)]
        explain: bool,
        /// Return result count and token estimate without snippets.
        #[arg(long)]
        count_only: bool,
    },
    /// Register a directory as a source and index it.
    Add {
        #[arg(value_hint = ValueHint::DirPath)]
        path: PathBuf,
        /// Registered source name. `--label` is still accepted.
        #[arg(long = "source", alias = "label", value_name = "SOURCE")]
        label: String,
        /// Index only paths matching this glob. Repeat for several patterns.
        #[arg(long = "include", alias = "glob", value_name = "GLOB")]
        include: Vec<String>,
        /// Skip paths matching this glob. Repeat for several patterns.
        #[arg(long, value_name = "GLOB")]
        exclude: Vec<String>,
        #[arg(long, value_delimiter = ',')]
        enrich: Vec<String>,
    },
    /// Re-index changed files and drop deleted ones, for one source or all.
    Sync {
        /// Restrict to one registered source. Omit to sync every source. `--label` is still accepted.
        #[arg(long = "source", alias = "label", value_name = "SOURCE")]
        label: Option<String>,
    },
    /// List sources, file and chunk counts, and embedding coverage.
    Status,
    /// Delete one source and its indexed rows. Does not delete files on disk.
    ///
    /// Removes the registration and every indexed file, chunk, symbol, and embedding for that
    /// source. The directory that was added is left untouched. Indexing it again requires `add`.
    Remove {
        /// Registered source name.
        #[arg(value_name = "SOURCE")]
        label: String,
    },
    /// Print stored chunks for a source in original order.
    Read {
        /// Registered source name.
        #[arg(value_name = "SOURCE")]
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
    /// Compute missing vectors for the configured embedding model.
    Embed {
        /// Restrict to one registered source. Omit to process every source. `--label` is still accepted.
        #[arg(long = "source", alias = "label", value_name = "SOURCE")]
        label: Option<String>,
    },
    /// Delete indexed rows that match the sensitive-data policy. Does not delete files on disk.
    ///
    /// Reads stored index rows and deletes the ones the policy matches. It does not walk the
    /// source directory and prints counts rather than paths or file contents. Omit `--source` to
    /// scan every registered source.
    Scrub {
        /// Restrict to one registered source. Omit to scan every stored row. `--label` is still accepted.
        #[arg(long = "source", alias = "label", value_name = "SOURCE")]
        label: Option<String>,
    },
    /// Keep sources in sync with the filesystem, and show watcher status.
    Watch {
        #[command(subcommand)]
        action: WatchAction,
    },
    /// Find declarations by name, with their paths and line ranges.
    Symbol {
        name: String,
        /// Match names that start with NAME instead of matching it exactly. Matching is case-sensitive.
        #[arg(long)]
        prefix: bool,
        /// Restrict to one registered source.
        #[arg(short, long, value_name = "SOURCE")]
        source: Option<String>,
        #[arg(short, long, default_value = "20")]
        limit: usize,
    },
    /// Read stored unused-symbol and unreachable-file conclusions for one source.
    ///
    /// Prints the analysis row first. Finding rows are included only when that analysis is
    /// complete. Omit `--source` only when exactly one source is registered.
    Findings {
        /// Registered source name. Omit only when one source is registered.
        #[arg(long = "source", alias = "label", value_name = "SOURCE")]
        source: Option<String>,
    },
    /// Show direct imports, includes, and links for one file. Does not use the index.
    Relate {
        #[arg(value_hint = ValueHint::FilePath)]
        path: PathBuf,
        #[arg(long, value_hint = ValueHint::DirPath)]
        root: Option<PathBuf>,
    },
    /// Show or download the configured embedding model.
    Models {
        #[command(subcommand)]
        action: ModelsAction,
    },
    /// Benchmarks and relevance-judgment tools.
    Eval {
        #[command(subcommand)]
        action: EvalAction,
    },
}

#[derive(Debug, Subcommand)]
pub enum WatchAction {
    /// Record that a source should be kept in sync. Does not start the watcher.
    ///
    /// A watcher that is already running picks the change up within a second or two. Start one
    /// with `watch run` or `bpx-rag serve`. `watch on` is still accepted.
    #[command(name = "enable", alias = "on")]
    Enable {
        /// Registered source name.
        #[arg(value_name = "SOURCE")]
        label: String,
    },
    /// Record that a source should not be kept in sync. Does not stop the watcher.
    ///
    /// A watcher that is already running picks the change up within a second or two and then
    /// leaves this source alone. `watch off` is still accepted.
    #[command(name = "disable", alias = "off")]
    Disable {
        /// Registered source name.
        #[arg(value_name = "SOURCE")]
        label: String,
    },
    /// Show the watcher and each source's watch state.
    Status,
    /// Run a watcher in this terminal until it is stopped.
    Run,
}

#[derive(Debug, Subcommand)]
pub enum ModelsAction {
    /// Show model configuration and whether all local model files are present.
    Status,
    /// Download the configured embedding model explicitly.
    Download,
}

#[derive(Debug, Subcommand)]
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
        /// Gating policy for the trigram and fuzzy layers. Used to compare policies.
        #[arg(long, value_enum, default_value_t = crate::knowledge::query::LayerPolicy::default())]
        layer_policy: crate::knowledge::query::LayerPolicy,
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
        /// Optional glob scope, matching a registered source's configured glob.
        #[arg(long)]
        glob: Option<String>,
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
    /// Generate or import a relevance-judgment pool.
    ///
    /// `create` writes a new blinded pool and a separate rank-reveal key. `import` writes
    /// completed grades to a new qrels file. Both refuse to overwrite an existing output file.
    /// `eval pool --output <FILE> --key-output <FILE>` is still accepted as `create`.
    /// `eval import-pool` is still accepted as `import`.
    Pool(PoolArgs),
    /// Import completed pool grades. Prefer `eval pool import`.
    #[command(hide = true, name = "import-pool")]
    ImportPool(PoolImportArgs),
}

#[derive(Debug, Args)]
#[command(args_conflicts_with_subcommands = true)]
pub struct PoolArgs {
    #[command(subcommand)]
    action: Option<PoolAction>,
    #[command(flatten)]
    legacy: LegacyPoolCreate,
}

impl PoolArgs {
    pub(crate) fn resolve(self) -> Result<PoolAction> {
        if self.action.is_some() && self.legacy.specified() {
            bail!(
                "pool options belong on the subcommand. Run `bpx-rag eval pool create --output <FILE> --key-output <FILE>` or `bpx-rag eval pool import`"
            );
        }
        if let Some(action) = self.action {
            return Ok(action);
        }
        if self.legacy.specified() {
            return Ok(PoolAction::Create(self.legacy.into_create()?));
        }
        bail!(
            "choose a pool command: `bpx-rag eval pool create --output <FILE> --key-output <FILE>` generates a relevance-judgment pool, and `bpx-rag eval pool import` imports completed grades. `bpx-rag eval pool --output <FILE> --key-output <FILE>` and `bpx-rag eval import-pool` are still accepted"
        )
    }
}

#[derive(Debug, Subcommand)]
pub enum PoolAction {
    /// Generate a blinded pool of lexical, vector, and hybrid file candidates for human relevance grading.
    ///
    /// Writes a new annotator file and a separate rank-reveal key. Refuses to overwrite either
    /// file, and the two paths must be different.
    Create(PoolCreateArgs),
    /// Import completed pool grades into a new qrels file.
    ///
    /// Refuses to overwrite an existing output file. Does not change the annotation pool or the
    /// reveal key.
    Import(PoolImportArgs),
}

#[derive(Debug, Args)]
pub struct PoolCreateArgs {
    /// Dataset directory containing queries.jsonl. Defaults to the bundled fixture.
    #[arg(long, value_hint = ValueHint::DirPath)]
    pub dataset: Option<PathBuf>,
    /// Corpus tree to index. Defaults to <dataset>/corpus.
    #[arg(long, value_hint = ValueHint::DirPath)]
    pub corpus_root: Option<PathBuf>,
    /// New JSON file for annotators. Refuses to overwrite an existing file.
    #[arg(long, value_hint = ValueHint::FilePath)]
    pub output: PathBuf,
    /// New JSON file for the retrieval-rank reveal key. Refuses to overwrite an existing file.
    #[arg(long, value_hint = ValueHint::FilePath)]
    pub key_output: PathBuf,
    /// Distinct files to retrieve per search channel before pooling.
    #[arg(long, default_value_t = DEFAULT_POOL_DEPTH)]
    pub depth: usize,
    /// Stable shuffle seed for blinded IDs and candidate order.
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
    /// New qrels file. Refuses to overwrite an existing file.
    #[arg(long, value_hint = ValueHint::FilePath)]
    pub output: PathBuf,
}

#[derive(Debug, Args)]
struct LegacyPoolCreate {
    #[arg(long, hide = true, value_hint = ValueHint::DirPath)]
    dataset: Option<PathBuf>,
    #[arg(long, hide = true, value_hint = ValueHint::DirPath)]
    corpus_root: Option<PathBuf>,
    #[arg(long, hide = true, value_hint = ValueHint::FilePath)]
    output: Option<PathBuf>,
    #[arg(long, hide = true, value_hint = ValueHint::FilePath)]
    key_output: Option<PathBuf>,
    #[arg(long, hide = true)]
    depth: Option<usize>,
    #[arg(long, hide = true)]
    seed: Option<u64>,
}

impl LegacyPoolCreate {
    fn specified(&self) -> bool {
        self.dataset.is_some()
            || self.corpus_root.is_some()
            || self.output.is_some()
            || self.key_output.is_some()
            || self.depth.is_some()
            || self.seed.is_some()
    }

    fn into_create(self) -> Result<PoolCreateArgs> {
        let mut missing = Vec::new();
        if self.output.is_none() {
            missing.push("--output <FILE>");
        }
        if self.key_output.is_none() {
            missing.push("--key-output <FILE>");
        }
        if !missing.is_empty() {
            bail!(
                "generating a pool requires {}. Run `bpx-rag eval pool create --output <FILE> --key-output <FILE>`",
                missing.join(" and ")
            );
        }
        Ok(PoolCreateArgs {
            dataset: self.dataset,
            corpus_root: self.corpus_root,
            output: self.output.expect("checked above"),
            key_output: self.key_output.expect("checked above"),
            depth: self.depth.unwrap_or(DEFAULT_POOL_DEPTH),
            seed: self.seed.unwrap_or(DEFAULT_POOL_SEED),
        })
    }
}

#[cfg(test)]
mod tests;
