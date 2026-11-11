use clap::{Parser, Subcommand, ValueHint};
use std::path::PathBuf;

const DEFAULT_POOL_DEPTH: usize = 20;
const DEFAULT_POOL_SEED: u64 = 20261021;

mod eval;

pub use eval::{EvalAction, PoolAction, PoolCreateArgs, PoolImportArgs};

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

#[cfg(test)]
mod tests;
