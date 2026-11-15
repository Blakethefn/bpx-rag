use anyhow::{bail, Result};
use clap::{Args, Subcommand, ValueHint};
use std::path::PathBuf;

mod args;
mod legacy;

pub use args::{PoolCreateArgs, PoolImportArgs};
use legacy::LegacyPoolCreate;

#[derive(Debug, Subcommand)]
pub enum EvalAction {
    #[command(about = "Run the real-model retrieval benchmark. This never downloads a model")]
    Retrieval {
        #[arg(long, default_value = "3")]
        repetitions: usize,
        #[arg(
            help = "RRF multiplier for lexical candidates (BM25, trigram, fuzzy). Must be in (0, 10]; defaults to 1.0"
        )]
        #[arg(long, default_value_t = 1.0)]
        keyword_weight: f64,
        #[arg(
            help = "RRF multiplier for semantic vector candidates. Must be in (0, 10]; defaults to 1.0"
        )]
        #[arg(long, default_value_t = 1.0)]
        vector_weight: f64,
        #[arg(
            help = "Directory containing queries.jsonl and qrels.jsonl. Defaults to the bundled fixture"
        )]
        #[arg(long, value_hint = ValueHint::DirPath)]
        dataset: Option<PathBuf>,
        #[arg(help = "Source tree to index. Defaults to <dataset>/corpus")]
        #[arg(long, value_hint = ValueHint::DirPath)]
        corpus_root: Option<PathBuf>,
        #[arg(help = "Gating policy for the trigram and fuzzy layers. Used to compare policies")]
        #[arg(long, value_enum, default_value_t = crate::knowledge::query::LayerPolicy::default())]
        layer_policy: crate::knowledge::query::LayerPolicy,
    },
    #[command(about = "Measure SQLite vector-search scaling with deterministic synthetic vectors")]
    Scaling {
        #[arg(long, default_value = "5")]
        repetitions: usize,
    },
    #[command(
        about = "Measure cached-model embedding throughput on production-sized source chunks"
    )]
    Embeddings {
        #[arg(long, value_hint = ValueHint::DirPath)]
        corpus_root: PathBuf,
        #[arg(help = "Optional glob scope, matching a registered source's configured glob")]
        #[arg(long)]
        glob: Option<String>,
        #[arg(
            help = "ONNX intra-op CPU thread counts to compare. Defaults to roughly one-third, two-thirds, and all available CPUs"
        )]
        #[arg(long, value_delimiter = ',')]
        threads: Option<Vec<usize>>,
        #[arg(help = "FastEmbed batch sizes to compare")]
        #[arg(long, value_delimiter = ',', default_value = "16,32")]
        batch_sizes: Vec<usize>,
        #[arg(help = "Repeated embedding passes per thread/batch combination")]
        #[arg(long, default_value = "2")]
        repetitions: usize,
        #[arg(help = "Maximum corpus-wide sampled chunks to embed per run")]
        #[arg(long, default_value = "32")]
        max_chunks: usize,
    },
    #[command(
        about = "Generate or import a relevance-judgment pool",
        long_about = "Generate or import a relevance-judgment pool.\n\n`create` writes a new blinded pool and a separate rank-reveal key. `import` writes completed grades to a new qrels file. Both refuse to overwrite an existing output file. `eval pool --output <FILE> --key-output <FILE>` is still accepted as `create`. `eval import-pool` is still accepted as `import`."
    )]
    Pool(PoolArgs),
    #[command(about = "Import completed pool grades. Prefer `eval pool import`")]
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
    #[command(
        about = "Generate a blinded pool of lexical, vector, and hybrid file candidates for human relevance grading",
        long_about = "Generate a blinded pool of lexical, vector, and hybrid file candidates for human relevance grading.\n\nWrites a new annotator file and a separate rank-reveal key. Refuses to overwrite either file, and the two paths must be different."
    )]
    Create(PoolCreateArgs),
    #[command(
        about = "Import completed pool grades into a new qrels file",
        long_about = "Import completed pool grades into a new qrels file.\n\nRefuses to overwrite an existing output file. Does not change the annotation pool or the reveal key."
    )]
    Import(PoolImportArgs),
}

#[cfg(test)]
mod tests;
