use clap::Args;

#[derive(Debug, Args)]
pub struct ReadArgs {
    #[arg(help = "Registered source name")]
    #[arg(value_name = "SOURCE")]
    pub label: String,
    #[arg(long, default_value = "20")]
    pub max_chunks: usize,
    #[arg(help = "Restrict readback to a source-relative file path")]
    #[arg(long)]
    pub path: Option<String>,
    #[arg(help = "Include chunks overlapping this 1-based starting line")]
    #[arg(long)]
    pub line_start: Option<u32>,
    #[arg(help = "Include chunks overlapping this 1-based ending line")]
    #[arg(long)]
    pub line_end: Option<u32>,
}
