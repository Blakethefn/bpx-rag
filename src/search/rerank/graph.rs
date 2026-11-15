use anyhow::Result;

use crate::knowledge::filter::parse_filter;
use crate::knowledge::query::KnowledgeSearchResult;
use crate::knowledge::KnowledgeStore;
use crate::search::profile::{SearchProfile, SearchWhy};

use super::knowledge_source_for_file;

pub(in super::super) fn apply_graph_rerank(
    ks: &KnowledgeStore,
    results: &mut Vec<KnowledgeSearchResult>,
    profile: SearchProfile,
    scoped_filtering: bool,
    filter_hint: Option<&str>,
    source_scope: Option<&str>,
) -> Result<()> {
    let boost = profile.graph_boost(scoped_filtering);
    if boost <= 0.0 || results.is_empty() {
        return Ok(());
    }

    let predicates = parse_filter(filter_hint)?;
    let preds = if predicates.is_empty() {
        None
    } else {
        Some(predicates.as_slice())
    };

    let seed_ids: Vec<i64> = results.iter().take(3).map(|r| r.file_id).collect();
    let mut linked_chunks = Vec::new();
    for file_id in seed_ids {
        linked_chunks.extend(ks.get_related_chunks(file_id, 6, preds)?);
    }

    let mut neighbor_by_file: std::collections::HashMap<i64, crate::knowledge::RelatedChunk> =
        std::collections::HashMap::new();
    let mut source_by_file: std::collections::HashMap<i64, (String, i64)> =
        std::collections::HashMap::new();
    for chunk in linked_chunks {
        if let Some(scope) = source_scope {
            let source = knowledge_source_for_file(ks, chunk.file_id)?;
            if scope != source.0 {
                continue;
            }
            source_by_file.insert(chunk.file_id, source);
        }
        let chunk_has_lines = chunk.line_start > 0 && chunk.line_end >= chunk.line_start;
        match neighbor_by_file.entry(chunk.file_id) {
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(chunk);
            }
            std::collections::hash_map::Entry::Occupied(mut entry)
                if (entry.get().line_start == 0
                    || entry.get().line_end < entry.get().line_start)
                    && chunk_has_lines =>
            {
                entry.insert(chunk);
            }
            std::collections::hash_map::Entry::Occupied(_) => {}
        }
    }

    let existing_files: std::collections::HashSet<i64> =
        results.iter().map(|r| r.file_id).collect();
    for result in results.iter_mut() {
        if neighbor_by_file.contains_key(&result.file_id) {
            result.score += boost;
            result.why.layers.graph_boost += boost;
            result.why.graph_neighbor = true;
        }
    }

    if profile.inject_neighbors() && !neighbor_by_file.is_empty() {
        let baseline = results.last().map(|r| r.score * 0.92).unwrap_or(boost);
        let mut injected = 0usize;
        for (file_id, chunk) in neighbor_by_file {
            if injected >= 2 || existing_files.contains(&file_id) {
                continue;
            }
            if chunk.line_start == 0 || chunk.line_end < chunk.line_start {
                continue;
            }
            let (source_label, source_id) = match source_by_file.get(&file_id) {
                Some((label, id)) => (label.clone(), *id),
                None => knowledge_source_for_file(ks, file_id)?,
            };
            let mut why = SearchWhy::default();
            why.layers.graph_boost = boost;
            why.graph_neighbor = true;
            results.push(KnowledgeSearchResult {
                title: chunk.title,
                snippet: chunk.content,
                score: baseline + boost,
                source_label,
                file_path: chunk.rel_path,
                file_id,
                source_id,
                lines: format!("{}-{}", chunk.line_start, chunk.line_end),
                metadata: serde_json::Value::Object(Default::default()),
                source_type: "knowledge".to_string(),
                why,
            });
            injected += 1;
        }
    }

    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(())
}
