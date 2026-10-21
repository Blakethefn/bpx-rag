mod cli;
mod mcp;
mod relationships;

use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;

use cli::{Cli, Commands};

fn main() -> Result<()> {
    match Cli::parse().command {
        Commands::AnalyzeFile { path, root } => {
            let analysis = relationships::analyze_file(&path, root.as_deref())?;
            println!("{}", relationships::format_analysis(&analysis));
            Ok(())
        }
        Commands::Serve => mcp::server::run(&project_dir()),
    }
}

fn project_dir() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}
