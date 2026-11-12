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
    #[arg(help = "Write a default config file if one does not already exist")]
    #[arg(long, hide = true)]
    pub init: bool,
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    #[command(
        about = "Write a default config file when none exists, and leave an existing file unchanged",
        long_about = "Write a default config file when none exists, and leave an existing file unchanged.\n\nCreates the config with the built-in defaults. A file that is already there is left as it is, and this command prints its path. `--init` is still accepted and does the same thing, including when another command is written after it: the config is written and that command is not run."
    )]
    Init,
    #[command(about = "Start the MCP server on stdio")]
    Serve,
    #[command(about = "Search registered sources and return cited evidence chunks")]
    Search {
        query: String,
        #[arg(help = "Restrict to one registered source")]
        #[arg(short, long, value_name = "SOURCE")]
        source: Option<String>,
        #[arg(help = "Metadata filter, e.g. `language:rust type:code`")]
        #[arg(long)]
        filter: Option<String>,
        #[arg(short, long, default_value = "10")]
        limit: u32,
        #[arg(
            help = "Preferred cap on passages from one file (1 to 50, default 1). Passages from one file are taken first, up to this number. If the other files cannot fill the limit, the remaining slots take the next-best passages from the same file. The cap is not strict"
        )]
        #[arg(long, default_value = "1", value_parser = clap::value_parser!(u32).range(1..=50))]
        max_per_file: u32,
        #[arg(long)]
        profile: Option<String>,
        #[arg(
            help = "Show what each retrieval layer did. Warns first when vector retrieval is not running, for example when the config asks for CUDA on a build without `gpu-cuda`"
        )]
        #[arg(long)]
        explain: bool,
        #[arg(help = "Return result count and token estimate without snippets")]
        #[arg(long)]
        count_only: bool,
    },
    #[command(about = "Register a directory as a source and index it")]
    Add {
        #[arg(value_hint = ValueHint::DirPath)]
        path: PathBuf,
        #[arg(help = "Registered source name. `--label` is still accepted")]
        #[arg(long = "source", alias = "label", value_name = "SOURCE")]
        label: String,
        #[arg(help = "Index only paths matching this glob. Repeat for several patterns")]
        #[arg(long = "include", alias = "glob", value_name = "GLOB")]
        include: Vec<String>,
        #[arg(help = "Skip paths matching this glob. Repeat for several patterns")]
        #[arg(long, value_name = "GLOB")]
        exclude: Vec<String>,
        #[arg(long, value_delimiter = ',')]
        enrich: Vec<String>,
    },
    #[command(about = "Re-index changed files and drop deleted ones, for one source or all")]
    Sync {
        #[arg(
            help = "Restrict to one registered source. Omit to sync every source. `--label` is still accepted"
        )]
        #[arg(long = "source", alias = "label", value_name = "SOURCE")]
        label: Option<String>,
    },
    #[command(about = "List sources, file and chunk counts, and embedding coverage")]
    Status,
    #[command(
        about = "Delete one source and its indexed rows. Does not delete files on disk",
        long_about = "Delete one source and its indexed rows. Does not delete files on disk.\n\nRemoves the registration and every indexed file, chunk, symbol, and embedding for that source. The directory that was added is left untouched. Indexing it again requires `add`."
    )]
    Remove {
        #[arg(help = "Registered source name")]
        #[arg(value_name = "SOURCE")]
        label: String,
    },
    #[command(about = "Print stored chunks for a source in original order")]
    Read {
        #[arg(help = "Registered source name")]
        #[arg(value_name = "SOURCE")]
        label: String,
        #[arg(long, default_value = "20")]
        max_chunks: usize,
        #[arg(help = "Restrict readback to a source-relative file path")]
        #[arg(long)]
        path: Option<String>,
        #[arg(help = "Include chunks overlapping this 1-based starting line")]
        #[arg(long)]
        line_start: Option<u32>,
        #[arg(help = "Include chunks overlapping this 1-based ending line")]
        #[arg(long)]
        line_end: Option<u32>,
    },
    #[command(about = "Compute missing vectors for the configured embedding model")]
    Embed {
        #[arg(
            help = "Restrict to one registered source. Omit to process every source. `--label` is still accepted"
        )]
        #[arg(long = "source", alias = "label", value_name = "SOURCE")]
        label: Option<String>,
    },
    #[command(
        about = "Delete indexed rows that match the sensitive-data policy. Does not delete files on disk",
        long_about = "Delete indexed rows that match the sensitive-data policy. Does not delete files on disk.\n\nReads stored index rows and deletes the ones the policy matches. It does not walk the source directory and prints counts rather than paths or file contents. Omit `--source` to scan every registered source."
    )]
    Scrub {
        #[arg(
            help = "Restrict to one registered source. Omit to scan every stored row. `--label` is still accepted"
        )]
        #[arg(long = "source", alias = "label", value_name = "SOURCE")]
        label: Option<String>,
    },
    #[command(about = "Keep sources in sync with the filesystem, and show watcher status")]
    Watch {
        #[command(subcommand)]
        action: WatchAction,
    },
    #[command(about = "Find declarations by name, with their paths and line ranges")]
    Symbol {
        name: String,
        #[arg(
            help = "Match names that start with NAME instead of matching it exactly. Matching is case-sensitive"
        )]
        #[arg(long)]
        prefix: bool,
        #[arg(help = "Restrict to one registered source")]
        #[arg(short, long, value_name = "SOURCE")]
        source: Option<String>,
        #[arg(short, long, default_value = "20")]
        limit: usize,
    },
    #[command(
        about = "Read stored unused-symbol and unreachable-file conclusions for one source",
        long_about = "Read stored unused-symbol and unreachable-file conclusions for one source.\n\nPrints the analysis row first. Finding rows are included only when that analysis is complete. Omit `--source` only when exactly one source is registered."
    )]
    Findings {
        #[arg(help = "Registered source name. Omit only when one source is registered")]
        #[arg(long = "source", alias = "label", value_name = "SOURCE")]
        source: Option<String>,
    },
    #[command(
        about = "Show direct imports, includes, and links for one file. Does not use the index"
    )]
    Relate {
        #[arg(value_hint = ValueHint::FilePath)]
        path: PathBuf,
        #[arg(long, value_hint = ValueHint::DirPath)]
        root: Option<PathBuf>,
    },
    #[command(about = "Show or download the configured embedding model")]
    Models {
        #[command(subcommand)]
        action: ModelsAction,
    },
    #[command(about = "Benchmarks and relevance-judgment tools")]
    Eval {
        #[command(subcommand)]
        action: EvalAction,
    },
}

#[derive(Debug, Subcommand)]
pub enum WatchAction {
    #[command(
        about = "Record that a source should be kept in sync. Does not start the watcher",
        long_about = "Record that a source should be kept in sync. Does not start the watcher.\n\nA watcher that is already running picks the change up within a second or two. Start one with `watch run` or `bpx-rag serve`. `watch on` is still accepted."
    )]
    #[command(name = "enable", alias = "on")]
    Enable {
        #[arg(help = "Registered source name")]
        #[arg(value_name = "SOURCE")]
        label: String,
    },
    #[command(
        about = "Record that a source should not be kept in sync. Does not stop the watcher",
        long_about = "Record that a source should not be kept in sync. Does not stop the watcher.\n\nA watcher that is already running picks the change up within a second or two and then leaves this source alone. `watch off` is still accepted."
    )]
    #[command(name = "disable", alias = "off")]
    Disable {
        #[arg(help = "Registered source name")]
        #[arg(value_name = "SOURCE")]
        label: String,
    },
    #[command(about = "Show the watcher and each source's watch state")]
    Status,
    #[command(about = "Run a watcher in this terminal until it is stopped")]
    Run,
}

#[derive(Debug, Subcommand)]
pub enum ModelsAction {
    #[command(about = "Show model configuration and whether all local model files are present")]
    Status,
    #[command(about = "Download the configured embedding model explicitly")]
    Download,
}

#[cfg(test)]
mod tests;
