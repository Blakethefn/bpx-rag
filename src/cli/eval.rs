use anyhow::{bail, Result};
use clap::parser::ValueSource;
use clap::{ArgMatches, Args, Command, FromArgMatches, Id, Subcommand, ValueHint};
use std::path::PathBuf;

use super::{DEFAULT_POOL_DEPTH, DEFAULT_POOL_SEED};

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

/// Hidden `eval pool` flags. They are the `PoolCreateArgs` fields, so a flag added there is
/// accepted on this path and selecting any of them chooses legacy create.
#[derive(Debug)]
struct LegacyPoolCreate {
    args: Option<PoolCreateArgs>,
    missing: Vec<String>,
}

impl LegacyPoolCreate {
    fn specified(&self) -> bool {
        self.args.is_some() || !self.missing.is_empty()
    }

    fn into_create(self) -> Result<PoolCreateArgs> {
        if let Some(args) = self.args {
            return Ok(args);
        }
        let missing = self.missing.join(" and ");
        bail!(
            "generating a pool requires {missing}. Run `bpx-rag eval pool create --output <FILE> --key-output <FILE>`"
        )
    }
}

impl Args for LegacyPoolCreate {
    fn augment_args(cmd: Command) -> Command {
        hide_create_flags(cmd)
    }

    fn augment_args_for_update(cmd: Command) -> Command {
        hide_create_flags(cmd)
    }
}

impl FromArgMatches for LegacyPoolCreate {
    fn from_arg_matches(matches: &ArgMatches) -> Result<Self, clap::Error> {
        let template = create_template();
        if !create_flag_on_command_line(matches, &template) {
            return Ok(Self {
                args: None,
                missing: Vec::new(),
            });
        }
        let missing = missing_required_flags(matches, &template);
        if !missing.is_empty() {
            return Ok(Self {
                args: None,
                missing,
            });
        }
        Ok(Self {
            args: Some(PoolCreateArgs::from_arg_matches(matches)?),
            missing: Vec::new(),
        })
    }

    fn update_from_arg_matches(&mut self, matches: &ArgMatches) -> Result<(), clap::Error> {
        *self = Self::from_arg_matches(matches)?;
        Ok(())
    }
}

fn create_template() -> Command {
    <PoolCreateArgs as Args>::augment_args(Command::new("pool-create"))
}

fn hide_create_flags(cmd: Command) -> Command {
    let ids = create_flag_ids();
    let cmd = <PoolCreateArgs as Args>::augment_args(cmd);
    cmd.mut_args(move |arg| {
        if ids.iter().any(|id| id == arg.get_id()) {
            arg.hide(true).required(false)
        } else {
            arg
        }
    })
}

fn create_flag_ids() -> Vec<Id> {
    create_template()
        .get_arguments()
        .map(|arg| arg.get_id().clone())
        .collect()
}

fn create_flag_on_command_line(matches: &ArgMatches, template: &Command) -> bool {
    template
        .get_arguments()
        .any(|arg| matches.value_source(arg.get_id().as_str()) == Some(ValueSource::CommandLine))
}

fn missing_required_flags(matches: &ArgMatches, template: &Command) -> Vec<String> {
    template
        .get_arguments()
        .filter(|arg| arg.is_required_set())
        .filter(|arg| matches.value_source(arg.get_id().as_str()) != Some(ValueSource::CommandLine))
        .filter_map(|arg| {
            let long = arg.get_long()?;
            Some(format!("--{long} <{}>", flag_placeholder(arg)))
        })
        .collect()
}

fn flag_placeholder(arg: &clap::Arg) -> &'static str {
    match arg.get_value_hint() {
        ValueHint::FilePath | ValueHint::AnyPath => "FILE",
        ValueHint::DirPath => "DIR",
        _ => "VALUE",
    }
}

#[cfg(test)]
mod tests;
