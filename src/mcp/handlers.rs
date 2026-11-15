use anyhow::{anyhow, Result};
use serde_json::Value;
use std::path::Path;

use crate::app::RagApp;

mod actions;
mod parse;
mod search;

pub(crate) use actions::{ModelAction, WatchAction};
pub(crate) use search::handle_search;

pub(super) fn handle_knowledge_add(args: &Value, app: &mut RagApp) -> Result<String> {
    let path = parse::required_string(args, "path")?;
    let label = parse::required_string(args, "label")?;
    let enrichments = parse::string_list(args, "enrichments")?;
    let mut include = parse::string_list(args, "include")?;
    if let Some(glob) = args["glob"].as_str() {
        include.insert(0, glob.to_string());
    }
    let exclude = parse::string_list(args, "exclude")?;
    app.add_source(Path::new(path), label, &include, &exclude, &enrichments)
}

pub(super) fn handle_knowledge_sync(args: &Value, app: &mut RagApp) -> Result<String> {
    let results = app.sync_sources(args["label"].as_str())?;
    if results.is_empty() {
        return Ok("No knowledge sources are registered.".into());
    }
    Ok(results
        .iter()
        .map(crate::app::format_sync_result)
        .collect::<Vec<_>>()
        .join("\n"))
}

pub(super) fn handle_knowledge_remove(args: &Value, app: &mut RagApp) -> Result<String> {
    let label = parse::required_string(args, "label")?;
    let result = app.remove_source(label)?;
    Ok(format!(
        "Removed source '{label}' ({} files, {} chunks).",
        result.files_removed, result.chunks_removed
    ))
}

pub(super) fn handle_knowledge_read(args: &Value, app: &mut RagApp) -> Result<String> {
    let label = parse::required_string(args, "label")?;
    app.read_chunks(
        label,
        parse::bounded_usize(args, "max_chunks", 20, 1, 100)?,
        args["path"].as_str(),
        parse::optional_bounded_u32(args, "line_start")?,
        parse::optional_bounded_u32(args, "line_end")?,
    )
}

pub(super) fn handle_models(args: &Value, app: &mut RagApp) -> Result<String> {
    match ModelAction::parse(args["action"].as_str())? {
        ModelAction::Status => Ok(app.embedding_model_status()),
        ModelAction::Download => app.download_embedding_model(),
    }
}

pub(super) fn handle_embed(args: &Value, app: &mut RagApp) -> Result<String> {
    let label = match args.get("label") {
        None | Some(Value::Null) => None,
        Some(Value::String(label)) => Some(label.as_str()),
        Some(_) => return Err(anyhow!("'label' must be a string")),
    };
    Ok(app.backfill_embeddings(label)?.summary())
}

pub(super) fn handle_watch(args: &Value, app: &mut RagApp) -> Result<String> {
    match WatchAction::parse(args["action"].as_str())? {
        WatchAction::On => app.set_watch(parse::required_string(args, "label")?, true),
        WatchAction::Off => app.set_watch(parse::required_string(args, "label")?, false),
        WatchAction::Status => app.watch_status(),
    }
}

pub(super) fn handle_symbol(args: &Value, app: &mut RagApp) -> Result<String> {
    let name = parse::required_string(args, "name")?;
    let mode = if args["prefix"].as_bool().unwrap_or(false) {
        crate::knowledge::symbols::MatchMode::Prefix
    } else {
        crate::knowledge::symbols::MatchMode::Exact
    };
    let limit = parse::bounded_usize(args, "limit", 20, 1, crate::app::MAX_SYMBOL_LIMIT)?;
    app.find_symbols(name, mode, args["source"].as_str(), limit)
}

pub(super) fn handle_findings(args: &Value, app: &mut RagApp) -> Result<String> {
    let source = match args.get("source") {
        None | Some(Value::Null) => None,
        Some(Value::String(source)) => Some(source.as_str()),
        Some(_) => return Err(anyhow!("'source' must be a string")),
    };
    app.findings(source)
}

pub(super) fn handle_file_relationships(args: &Value, project_dir: &Path) -> Result<String> {
    let path = parse::required_string(args, "path")?;
    let root = args["root"].as_str().map(Path::new);
    let requested = Path::new(path);
    let resolved = if requested.is_absolute() || root.is_some() {
        requested.to_path_buf()
    } else {
        project_dir.join(requested)
    };
    let analysis = crate::relations::analyze_file(&resolved, root)?;
    Ok(crate::relations::format_analysis(&analysis))
}

#[cfg(test)]
mod tests;
