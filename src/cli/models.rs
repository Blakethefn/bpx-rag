use clap::Subcommand;

#[derive(Debug, Subcommand)]
pub enum ModelsAction {
    #[command(about = "Show model configuration and whether all local model files are present")]
    Status,
    #[command(about = "Download the configured embedding model explicitly")]
    Download,
}
