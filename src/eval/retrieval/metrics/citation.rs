use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

pub(in crate::eval) fn citation_quality(
    corpus_dir: &Path,
    file_path: &str,
    range: &str,
    snippet: &str,
) -> Result<(bool, bool)> {
    let path = Path::new(file_path);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Ok((false, false));
    }
    let file_path = corpus_dir.join(path);
    let content = fs::read_to_string(&file_path)
        .with_context(|| format!("search result cited a missing file {}", file_path.display()))?;
    let Some((start, end)) = range.split_once('-') else {
        return Ok((false, false));
    };
    let (Ok(start), Ok(end)) = (start.parse::<u32>(), end.parse::<u32>()) else {
        return Ok((false, false));
    };
    let lines = content.lines().collect::<Vec<_>>();
    if start == 0 || end < start || end > lines.len() as u32 {
        return Ok((false, false));
    }
    let cited_lines = lines[(start - 1) as usize..end as usize].join("\n");
    let snippet = snippet.strip_suffix("...").unwrap_or(snippet);
    let normalized_citation = normalize_citation_text(&cited_lines);
    let normalized_snippet = normalize_citation_text(snippet);
    let content_matches =
        !normalized_snippet.is_empty() && normalized_citation.starts_with(&normalized_snippet);
    Ok((true, content_matches))
}

pub(in crate::eval) fn normalize_citation_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
