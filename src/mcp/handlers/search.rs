use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use super::parse;
use crate::app::{RagApp, SearchOptions};

pub(crate) fn handle_search(args: &Value, app: &mut RagApp) -> Result<String> {
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
        limit: parse::bounded_u32(args, "limit", 10, 1, 50)?,
        max_per_file: parse::bounded_u32(
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

fn queries_len_multi(value: &Value) -> bool {
    value.as_array().is_some_and(|queries| queries.len() > 1)
}
