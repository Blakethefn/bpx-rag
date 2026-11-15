use clap::{Parser, Subcommand, ValueHint};
use std::path::PathBuf;

const DEFAULT_POOL_DEPTH: usize = 20;
const DEFAULT_POOL_SEED: u64 = 20261021;

mod add;
mod eval;
mod help;
mod models;
mod read;
mod search;
mod watch;

pub use add::AddArgs;
pub use eval::{EvalAction, PoolAction, PoolCreateArgs, PoolImportArgs};
pub use models::ModelsAction;
pub use read::ReadArgs;
pub use search::SearchArgs;
pub use watch::WatchAction;

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
    #[command(about = help::INIT_ABOUT, long_about = help::INIT_LONG)]
    Init,
    #[command(about = "Start the MCP server on stdio")]
    Serve,
    #[command(about = "Search registered sources and return cited evidence chunks")]
    Search(SearchArgs),
    #[command(about = "Register a directory as a source and index it")]
    Add(AddArgs),
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
    #[command(about = help::REMOVE_ABOUT, long_about = help::REMOVE_LONG)]
    Remove {
        #[arg(help = "Registered source name")]
        #[arg(value_name = "SOURCE")]
        label: String,
    },
    #[command(about = "Print stored chunks for a source in original order")]
    Read(ReadArgs),
    #[command(about = "Compute missing vectors for the configured embedding model")]
    Embed {
        #[arg(
            help = "Restrict to one registered source. Omit to process every source. `--label` is still accepted"
        )]
        #[arg(long = "source", alias = "label", value_name = "SOURCE")]
        label: Option<String>,
    },
    #[command(about = help::SCRUB_ABOUT, long_about = help::SCRUB_LONG)]
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
    #[command(about = help::FINDINGS_ABOUT, long_about = help::FINDINGS_LONG)]
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

#[cfg(test)]
mod tests;
