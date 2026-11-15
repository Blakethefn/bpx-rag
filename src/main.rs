mod app;
mod chunk;
mod cli;
mod commands;
mod config;
mod db;
mod embed;
mod eval;
mod knowledge;
mod language;
mod mcp;
mod relations;
mod search;
#[cfg(test)]
mod support;
mod watch;

use anyhow::{anyhow, Result};
use clap::Parser;
use std::path::PathBuf;

use app::{RagApp, SearchOptions};
use cli::{Cli, Commands};

fn main() -> Result<()> {
    let cli = Cli::parse();
    if cli.init {
        return config::Config::init_default();
    }
    let Some(command) = cli.command else {
        return Err(anyhow!("choose a command; use `bpx-rag --help` for usage"));
    };
    let project_dir = project_dir();

    match command {
        Commands::Init => config::Config::init_default(),
        Commands::Serve => mcp::server::run(&project_dir),
        Commands::Symbol {
            name,
            prefix,
            source,
            limit,
        } => {
            let app = RagApp::open()?;
            let mode = if prefix {
                knowledge::symbols::MatchMode::Prefix
            } else {
                knowledge::symbols::MatchMode::Exact
            };
            println!(
                "{}",
                app.find_symbols(&name, mode, source.as_deref(), limit)?
            );
            Ok(())
        }
        Commands::Findings { source } => {
            println!("{}", RagApp::open()?.findings(source.as_deref())?);
            Ok(())
        }
        Commands::Watch { action } => commands::run_watch(action),
        Commands::Relate { path, root } => {
            let analysis = relations::analyze_file(&path, root.as_deref())?;
            println!("{}", relations::format_analysis(&analysis));
            Ok(())
        }
        Commands::Search(args) => {
            let mut app = RagApp::open()?;
            let output = app.search(
                &args.query,
                SearchOptions {
                    source: args.source.as_deref(),
                    filter: args.filter.as_deref(),
                    limit: args.limit,
                    max_per_file: args.max_per_file,
                    profile: args.profile.as_deref(),
                    explain: args.explain,
                    count_only: args.count_only,
                },
            )?;
            println!("{output}");
            Ok(())
        }
        Commands::Add(_)
        | Commands::Sync { .. }
        | Commands::Status
        | Commands::Remove { .. }
        | Commands::Read(_)
        | Commands::Embed { .. }
        | Commands::Scrub { .. } => commands::run_sources(command),
        Commands::Models { action } => commands::run_models(action),
        Commands::Eval { action } => commands::run_evaluation(action),
    }
}

fn project_dir() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}
