use anyhow::{anyhow, Result};

use crate::app::{self, RagApp};
use crate::cli::Commands;

pub(crate) fn run_sources(command: Commands) -> Result<()> {
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
