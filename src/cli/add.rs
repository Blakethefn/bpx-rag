use clap::{Args, ValueHint};
use std::path::PathBuf;

#[derive(Debug, Args)]
pub struct AddArgs {
    #[arg(value_hint = ValueHint::DirPath)]
    pub path: PathBuf,
    #[arg(help = "Registered source name. `--label` is still accepted")]
    #[arg(long = "source", alias = "label", value_name = "SOURCE")]
    pub label: String,
    #[arg(help = "Index only paths matching this glob. Repeat for several patterns")]
    #[arg(long = "include", alias = "glob", value_name = "GLOB")]
    pub include: Vec<String>,
    #[arg(help = "Skip paths matching this glob. Repeat for several patterns")]
    #[arg(long, value_name = "GLOB")]
    pub exclude: Vec<String>,
    #[arg(long, value_delimiter = ',')]
    pub enrich: Vec<String>,
}
