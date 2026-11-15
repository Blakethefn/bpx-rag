use clap::Subcommand;

use super::help;

#[derive(Debug, Subcommand)]
pub enum WatchAction {
    #[command(
        about = help::WATCH_ENABLE_ABOUT,
        long_about = help::WATCH_ENABLE_LONG
    )]
    #[command(name = "enable", alias = "on")]
    Enable {
        #[arg(help = "Registered source name")]
        #[arg(value_name = "SOURCE")]
        label: String,
    },
    #[command(
        about = help::WATCH_DISABLE_ABOUT,
        long_about = help::WATCH_DISABLE_LONG
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
