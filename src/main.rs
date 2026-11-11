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
mod watch;

use anyhow::{anyhow, Result};
use clap::Parser;
use std::path::PathBuf;

use app::{RagApp, SearchOptions};
use cli::{Cli, Commands, ModelsAction, WatchAction};

fn main() -> Result<()> {
    let cli = Cli::parse();
    // `--init` remains the old flag. It writes the config and does not run a command after it.
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
        Commands::Watch { action } => run_watch(action),
        Commands::Relate { path, root } => {
            let analysis = relations::analyze_file(&path, root.as_deref())?;
            println!("{}", relations::format_analysis(&analysis));
            Ok(())
        }
        Commands::Search {
            query,
            source,
            filter,
            limit,
            max_per_file,
            profile,
            explain,
            count_only,
        } => {
            let mut app = RagApp::open()?;
            let output = app.search(
                &query,
                SearchOptions {
                    source: source.as_deref(),
                    filter: filter.as_deref(),
                    limit,
                    max_per_file,
                    profile: profile.as_deref(),
                    explain,
                    count_only,
                },
            )?;
            println!("{output}");
            Ok(())
        }
        Commands::Add { .. }
        | Commands::Sync { .. }
        | Commands::Status
        | Commands::Remove { .. }
        | Commands::Read { .. }
        | Commands::Embed { .. }
        | Commands::Scrub { .. } => commands::run_sources(command),
        Commands::Models { action } => run_models(action),
        Commands::Eval { action } => commands::run_evaluation(action),
    }
}

fn run_watch(action: WatchAction) -> Result<()> {
    match action {
        WatchAction::Enable { label } => {
            println!("{}", RagApp::open()?.set_watch(&label, true)?);
            Ok(())
        }
        WatchAction::Disable { label } => {
            println!("{}", RagApp::open()?.set_watch(&label, false)?);
            Ok(())
        }
        WatchAction::Status => {
            println!("{}", RagApp::open()?.watch_status()?);
            Ok(())
        }
        WatchAction::Run => {
            match RagApp::open()?.run_watch()? {
                watch::RunOutcome::Stopped => println!("Watcher stopped."),
                watch::RunOutcome::AlreadyRunning { pid } => println!(
                    "A watcher is already running for this database (pid {}).",
                    pid.map_or_else(|| "unknown".to_string(), |pid| pid.to_string())
                ),
            }
            Ok(())
        }
    }
}

fn run_models(action: ModelsAction) -> Result<()> {
    let config = config::Config::load()?;
    match action {
        ModelsAction::Status => println!("{}", app::embedding_model_status(&config)),
        ModelsAction::Download => {
            let model_dir = app::model_directory(&config);
            embed::Embedder::new_with_device(
                &model_dir,
                &config.embeddings.model,
                config.embeddings.batch_size,
                config.embeddings.intra_threads,
                config.embeddings.device,
            )?;
            println!(
                "Downloaded and loaded {} at {}.",
                config.embeddings.model,
                model_dir.display()
            );
        }
    }
    Ok(())
}

fn project_dir() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}
