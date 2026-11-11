use anyhow::{anyhow, Result};

use crate::knowledge::symbols::{self, MatchMode, SymbolQuery, SymbolReport};

use super::{RagApp, MAX_SYMBOL_LIMIT};

impl RagApp {
    pub fn find_symbols(
        &self,
        name: &str,
        mode: MatchMode,
        source: Option<&str>,
        limit: usize,
    ) -> Result<String> {
        if !(1..=MAX_SYMBOL_LIMIT).contains(&limit) {
            return Err(anyhow!("limit must be between 1 and {MAX_SYMBOL_LIMIT}"));
        }
        let name = name.trim();
        let report = symbols::lookup(
            &self.knowledge,
            &SymbolQuery {
                name,
                mode,
                source,
                limit,
            },
        )?;
        Ok(format_symbol_report(name, mode, &report))
    }
}

fn format_symbol_report(name: &str, mode: MatchMode, report: &SymbolReport) -> String {
    let mode_label = match mode {
        MatchMode::Exact => "exact",
        MatchMode::Prefix => "prefix",
    };
    let mut lines = Vec::new();
    if report.hits.is_empty() {
        lines.push(format!("No symbols matched `{name}` ({mode_label})."));
    } else {
        lines.push(format!(
            "Showing {} symbol match(es) for `{name}` ({mode_label}).",
            report.hits.len()
        ));
        for hit in &report.hits {
            let qualified = match &hit.container {
                Some(container) => format!("{container}::{}", hit.name),
                None => hit.name.clone(),
            };
            lines.push(format!(
                "{}:{}-{}  {}  {}  [source: {}]",
                hit.rel_path, hit.line_start, hit.line_end, hit.kind, qualified, hit.source_label
            ));
        }
        if report.truncated {
            lines.push(format!(
                "More matches exist. Raise the limit (maximum {MAX_SYMBOL_LIMIT}) to see them."
            ));
        }
    }
    for pending in &report.pending {
        lines.push(format!(
            "Symbol index pending for source '{}': {} file(s) not yet symbol-indexed. Run `bpx-rag sync --source {}` or bpx_sync with that label to backfill.",
            pending.source_label, pending.files, pending.source_label
        ));
    }
    lines.join("\n")
}
