use clap::Args;

#[derive(Debug, Args)]
pub struct SearchArgs {
    pub query: String,
    #[arg(help = "Restrict to one registered source")]
    #[arg(short, long, value_name = "SOURCE")]
    pub source: Option<String>,
    #[arg(help = "Metadata filter, e.g. `language:rust type:code`")]
    #[arg(long)]
    pub filter: Option<String>,
    #[arg(short, long, default_value = "10")]
    pub limit: u32,
    #[arg(
        help = "Preferred cap on passages from one file (1 to 50, default 1). Passages from one file are taken first, up to this number. If the other files cannot fill the limit, the remaining slots take the next-best passages from the same file. The cap is not strict"
    )]
    #[arg(long, default_value = "1", value_parser = clap::value_parser!(u32).range(1..=50))]
    pub max_per_file: u32,
    #[arg(long)]
    pub profile: Option<String>,
    #[arg(
        help = "Show what each retrieval layer did. Warns first when vector retrieval is not running, for example when the config asks for CUDA on a build without `gpu-cuda`"
    )]
    #[arg(long)]
    pub explain: bool,
    #[arg(help = "Return result count and token estimate without snippets")]
    #[arg(long)]
    pub count_only: bool,
}
