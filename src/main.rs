mod cli;
mod config;
mod db;
mod embedder;
mod eval;
mod knowledge;
mod mcp;
mod rag_app;
mod relationships;
mod retrieval;
mod search_memory;
mod search_model;
mod session;
mod store;
mod truncate;

use anyhow::{anyhow, Result};
use clap::Parser;
use std::path::PathBuf;

use cli::{Cli, Commands, EvalAction, KnowledgeAction, ModelsAction};
use rag_app::{RagApp, SearchOptions};

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
        Commands::Serve => mcp::server::run(&project_dir),
        Commands::AnalyzeFile { path, root } => {
            let analysis = relationships::analyze_file(&path, root.as_deref())?;
            println!("{}", relationships::format_analysis(&analysis));
            Ok(())
        }
        Commands::Search {
            query,
            source,
            filter,
            limit,
            profile,
            explain,
            count_only,
            no_memory,
        } => {
            let mut app = RagApp::open(&project_dir)?;
            let output = app.search(
                &query,
                SearchOptions {
                    source: source.as_deref(),
                    filter: filter.as_deref(),
                    content_type: None,
                    limit,
                    profile: profile.as_deref(),
                    explain,
                    count_only,
                    use_memory: !no_memory,
                },
            )?;
            println!("{output}");
            Ok(())
        }
        Commands::Knowledge { action } => run_knowledge(action, &project_dir),
        Commands::Models { action } => run_models(action),
        Commands::Evaluate { action } => run_evaluation(action),
    }
}

fn run_evaluation(action: EvalAction) -> Result<()> {
    match action {
        EvalAction::Retrieval { repetitions } => {
            let report = eval::run_retrieval_evaluation(repetitions)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            if report.filter_leaks > 0 || report.citation_validity < 1.0 {
                return Err(anyhow!(
                    "retrieval evaluation found {} filter leaks and citation validity {:.3}",
                    report.filter_leaks,
                    report.citation_validity
                ));
            }
        }
        EvalAction::Scaling { repetitions } => {
            let report = eval::run_scaling_evaluation(repetitions)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
    }
    Ok(())
}

fn run_knowledge(action: KnowledgeAction, project_dir: &std::path::Path) -> Result<()> {
    let mut app = RagApp::open(project_dir)?;
    match action {
        KnowledgeAction::Add {
            path,
            label,
            glob,
            enrichments,
        } => println!(
            "{}",
            app.add_source(&path, &label, glob.as_deref(), &enrichments)?
        ),
        KnowledgeAction::Sync { label } => {
            let results = app.sync_sources(label.as_deref())?;
            if results.is_empty() {
                println!("No knowledge sources are registered.");
            } else {
                for result in &results {
                    println!("{}", rag_app::format_sync_result(result));
                }
            }
        }
        KnowledgeAction::Status => println!("{}", app.source_status()?),
        KnowledgeAction::Remove { label } => {
            let result = app.remove_source(&label)?;
            println!(
                "Removed source '{label}' ({} files, {} chunks).",
                result.files_removed, result.chunks_removed
            );
        }
        KnowledgeAction::EmbedBackfill => {
            let count = app.backfill_embeddings()?;
            println!("Created embeddings for {count} chunks.");
        }
        KnowledgeAction::Read {
            label,
            max_chunks,
            path,
            line_start,
            line_end,
        } => {
            println!(
                "{}",
                app.read_chunks(&label, max_chunks, path.as_deref(), line_start, line_end,)?
            );
        }
    }
    Ok(())
}

fn run_models(action: ModelsAction) -> Result<()> {
    let config = config::Config::load()?;
    match action {
        ModelsAction::Status => println!("{}", rag_app::embedding_model_status(&config)),
        ModelsAction::Download => {
            let model_dir = rag_app::model_directory(&config);
            embedder::Embedder::new(&model_dir, &config.embeddings.model)?;
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
