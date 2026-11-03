mod app;
mod chunk;
mod cli;
mod config;
mod db;
mod embed;
mod eval;
mod knowledge;
mod language;
mod mcp;
mod relations;
mod rerank;
mod search;

use anyhow::{anyhow, Result};
use clap::Parser;
use std::path::PathBuf;

use app::{RagApp, SearchOptions};
use cli::{Cli, Commands, EvalAction, ModelsAction};
use knowledge::query::SearchWeights;

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
        | Commands::Scrub { .. } => run_sources(command),
        Commands::Models { action } => run_models(action),
        Commands::Eval { action } => run_evaluation(action),
    }
}

fn run_evaluation(action: EvalAction) -> Result<()> {
    match action {
        EvalAction::Retrieval {
            repetitions,
            keyword_weight,
            vector_weight,
            dataset,
            corpus_root,
            layer_policy,
        } => {
            let report = eval::run_retrieval_evaluation(
                repetitions,
                dataset.as_deref(),
                corpus_root.as_deref(),
                SearchWeights {
                    keyword_weight,
                    vector_weight,
                },
                layer_policy,
            )?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            if report.filter_leaks > 0
                || report.citation_validity < 1.0
                || !report.citation_content_mismatches.is_empty()
            {
                return Err(anyhow!(
                    "retrieval evaluation found {} filter leaks, citation validity {:.3}, and {} snippet/source mismatches",
                    report.filter_leaks,
                    report.citation_validity,
                    report.citation_content_mismatches.len()
                ));
            }
        }
        EvalAction::Scaling { repetitions } => {
            let report = eval::run_scaling_evaluation(repetitions)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        EvalAction::Embeddings {
            corpus_root,
            glob,
            threads,
            batch_sizes,
            repetitions,
            max_chunks,
        } => {
            let report = eval::run_embedding_benchmark(
                &corpus_root,
                glob.as_deref(),
                threads.as_deref().unwrap_or(&[]),
                &batch_sizes,
                repetitions,
                max_chunks,
            )?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        EvalAction::Pool {
            dataset,
            corpus_root,
            output,
            key_output,
            depth,
            seed,
        } => {
            let report = eval::generate_judgment_pool(
                dataset.as_deref(),
                corpus_root.as_deref(),
                &output,
                &key_output,
                depth,
                seed,
            )?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        EvalAction::ImportPool {
            pool,
            key,
            dataset,
            corpus_root,
            output,
        } => {
            let imported =
                eval::import_judgment_pool(&pool, &key, &dataset, &corpus_root, &output)?;
            println!(
                "Imported {} explicit relevance grades into {}.",
                imported,
                output.display()
            );
        }
    }
    Ok(())
}

fn run_sources(command: Commands) -> Result<()> {
    let mut app = RagApp::open()?;
    match command {
        Commands::Add {
            path,
            label,
            include,
            exclude,
            enrich,
        } => println!(
            "{}",
            app.add_source(&path, &label, &include, &exclude, &enrich)?
        ),
        Commands::Sync { label } => {
            let results = app.sync_sources(label.as_deref())?;
            if results.is_empty() {
                println!("No knowledge sources are registered.");
            } else {
                for result in &results {
                    println!("{}", app::format_sync_result(result));
                }
            }
        }
        Commands::Status => println!("{}", app.source_status()?),
        Commands::Remove { label } => {
            let result = app.remove_source(&label)?;
            println!(
                "Removed source '{label}' ({} files, {} chunks).",
                result.files_removed, result.chunks_removed
            );
        }
        Commands::Embed { label } => {
            let report = app.backfill_embeddings(label.as_deref())?;
            println!("{}", report.summary());
        }
        Commands::Scrub { label } => {
            let report = app.scrub_sensitive_index(label.as_deref())?;
            println!("{}", report.summary());
        }
        Commands::Read {
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
        _ => {
            return Err(anyhow!(
                "internal error: non-source command routed to run_sources"
            ))
        }
    }
    Ok(())
}

fn run_models(action: ModelsAction) -> Result<()> {
    let config = config::Config::load()?;
    match action {
        ModelsAction::Status => {
            println!("{}", app::embedding_model_status(&config));
            println!("{}", rerank::status_line(&config));
        }
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
