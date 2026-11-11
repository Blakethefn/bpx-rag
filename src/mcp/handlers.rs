use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::path::Path;

use crate::app::{RagApp, SearchOptions};

pub(super) fn handle_search(args: &Value, app: &mut RagApp) -> Result<String> {
    let queries = if let Some(queries) = args["queries"].as_array() {
        queries
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect::<Vec<_>>()
    } else {
        args["query"]
            .as_str()
            .map(str::to_string)
            .into_iter()
            .collect()
    };
    if queries.is_empty() || queries.len() > 8 {
        return Err(anyhow!("provide `query` or 1–8 non-empty `queries`"));
    }
    let options = SearchOptions {
        source: args["source"].as_str(),
        filter: args["filter"].as_str(),
        limit: bounded_u32(args, "limit", 10, 1, 50)?,
        max_per_file: bounded_u32(
            args,
            "max_per_file",
            1,
            1,
            crate::knowledge::query::MAX_PER_FILE,
        )?,
        profile: args["profile"].as_str(),
        explain: args["explain"].as_bool().unwrap_or(false),
        count_only: args["count_only"].as_bool().unwrap_or(false),
    };
    let mut output = String::new();
    let mut count_results = Vec::new();
    for query in queries {
        if query.trim().is_empty() {
            return Err(anyhow!("queries must not be empty"));
        }
        let result = app.search(&query, options.clone())?;
        if options.count_only {
            count_results.push(serde_json::from_str::<Value>(&result)?);
            continue;
        }
        if queries_len_multi(&args["queries"]) {
            output.push_str(&format!("# Query: {}\n\n", query.trim()));
        }
        output.push_str(&result);
        output.push_str("\n\n");
    }
    if options.count_only && count_results.len() > 1 {
        return Ok(serde_json::to_string_pretty(
            &json!({ "queries": count_results }),
        )?);
    }
    if options.count_only {
        return Ok(serde_json::to_string_pretty(&count_results[0])?);
    }
    Ok(output.trim_end().to_string())
}

pub(super) fn handle_knowledge_add(args: &Value, app: &mut RagApp) -> Result<String> {
    let path = required_string(args, "path")?;
    let label = required_string(args, "label")?;
    let enrichments = string_list(args, "enrichments")?;
    let mut include = string_list(args, "include")?;
    if let Some(glob) = args["glob"].as_str() {
        include.insert(0, glob.to_string());
    }
    let exclude = string_list(args, "exclude")?;
    app.add_source(Path::new(path), label, &include, &exclude, &enrichments)
}

/// Read an optional array of strings. Any other shape is an error, not an empty list.
fn string_list(args: &Value, key: &str) -> Result<Vec<String>> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(values)) => values
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_string)
                    .ok_or_else(|| anyhow!("'{key}' must contain only strings"))
            })
            .collect(),
        Some(_) => Err(anyhow!("'{key}' must be an array of strings")),
    }
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

pub(super) fn handle_knowledge_remove(args: &Value, app: &RagApp) -> Result<String> {
    let label = required_string(args, "label")?;
    let result = app.remove_source(label)?;
    Ok(format!(
        "Removed source '{label}' ({} files, {} chunks).",
        result.files_removed, result.chunks_removed
    ))
}

pub(super) fn handle_knowledge_read(args: &Value, app: &RagApp) -> Result<String> {
    let label = required_string(args, "label")?;
    app.read_chunks(
        label,
        bounded_usize(args, "max_chunks", 20, 1, 100)?,
        args["path"].as_str(),
        optional_bounded_u32(args, "line_start")?,
        optional_bounded_u32(args, "line_end")?,
    )
}

pub(super) fn handle_models(args: &Value, app: &mut RagApp) -> Result<String> {
    match args["action"].as_str().unwrap_or("") {
        "status" => Ok(app.embedding_model_status()),
        "download" => app.download_embedding_model(),
        action => Err(anyhow!("unknown models action '{action}'")),
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
    match args["action"].as_str().unwrap_or("") {
        "on" => app.set_watch(required_string(args, "label")?, true),
        "off" => app.set_watch(required_string(args, "label")?, false),
        "status" => app.watch_status(),
        action => Err(anyhow!("unknown watch action '{action}'")),
    }
}

pub(super) fn handle_symbol(args: &Value, app: &mut RagApp) -> Result<String> {
    let name = required_string(args, "name")?;
    let mode = if args["prefix"].as_bool().unwrap_or(false) {
        crate::knowledge::symbols::MatchMode::Prefix
    } else {
        crate::knowledge::symbols::MatchMode::Exact
    };
    let limit = bounded_usize(args, "limit", 20, 1, crate::app::MAX_SYMBOL_LIMIT)?;
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
    let path = required_string(args, "path")?;
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

fn required_string<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args[key]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("'{key}' is required"))
}

fn bounded_u32(args: &Value, key: &str, default: u32, min: u32, max: u32) -> Result<u32> {
    let value = match args[key].as_u64() {
        Some(value) => u32::try_from(value).map_err(|_| anyhow!("'{key}' is out of range"))?,
        None => default,
    };
    if !(min..=max).contains(&value) {
        return Err(anyhow!("'{key}' must be between {min} and {max}"));
    }
    Ok(value)
}

fn optional_bounded_u32(args: &Value, key: &str) -> Result<Option<u32>> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => {
            let number = value
                .as_u64()
                .ok_or_else(|| anyhow!("'{key}' must be a positive integer"))?;
            if number == 0 || number > u32::MAX as u64 {
                return Err(anyhow!("'{key}' must be between 1 and {}", u32::MAX));
            }
            Ok(Some(number as u32))
        }
    }
}

fn bounded_usize(args: &Value, key: &str, default: usize, min: usize, max: usize) -> Result<usize> {
    let value = match args[key].as_u64() {
        Some(value) => usize::try_from(value).map_err(|_| anyhow!("'{key}' is out of range"))?,
        None => default,
    };
    if !(min..=max).contains(&value) {
        return Err(anyhow!("'{key}' must be between {min} and {max}"));
    }
    Ok(value)
}

fn queries_len_multi(value: &Value) -> bool {
    value.as_array().is_some_and(|queries| queries.len() > 1)
}
