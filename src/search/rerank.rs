use anyhow::Result;

use crate::knowledge::query::KnowledgeSearchResult;
use crate::knowledge::KnowledgeStore;
use crate::search::profile::{SearchProfile, SearchWhy};

mod graph;

pub(super) use graph::apply_graph_rerank;

pub(super) fn apply_knowledge_bias(results: &mut [KnowledgeSearchResult], profile: SearchProfile) {
    if matches!(profile, SearchProfile::Default) {
        return;
    }
    let multiplier = profile.knowledge_bias_multiplier();
    for result in results {
        let boosted = result.score * multiplier;
        result.why.layers.knowledge_bias += boosted - result.score;
        result.score = boosted;
    }
}

pub(crate) fn attach_relation_neighbors(
    ks: &KnowledgeStore,
    results: Vec<KnowledgeSearchResult>,
    neighbor_limit: usize,
    limit: usize,
) -> Result<Vec<KnowledgeSearchResult>> {
    let Some(top) = results.first() else {
        return Ok(results);
    };
    let linked_paths = ks.related_files(top.file_id, neighbor_limit)?;
    if linked_paths.is_empty() {
        return Ok(results);
    }
    let linked_chunks = ks.get_related_chunks(top.file_id, usize::MAX, None)?;
    let cut_score = results.last().map_or(0.0, |result| result.score);
    let mut neighbors = Vec::new();
    for path in &linked_paths {
        let Some(chunk) = linked_chunks.iter().find(|chunk| &chunk.rel_path == path) else {
            continue;
        };
        let (source_label, source_id) = knowledge_source_for_file(ks, chunk.file_id)?;
        let why = SearchWhy {
            graph_neighbor: true,
            ..Default::default()
        };
        neighbors.push(KnowledgeSearchResult {
            title: chunk.title.clone(),
            snippet: chunk.content.clone(),
            score: cut_score,
            source_label,
            file_path: chunk.rel_path.clone(),
            file_id: chunk.file_id,
            source_id,
            lines: format!("{}-{}", chunk.line_start, chunk.line_end),
            metadata: serde_json::Value::Object(Default::default()),
            source_type: "knowledge".to_string(),
            why,
        });
    }
    Ok(crate::knowledge::links::merge_neighbors(
        results,
        neighbors,
        crate::knowledge::links::RELATION_KEEP_TOP,
        limit,
        |result| result.file_path.clone(),
    ))
}

fn knowledge_source_for_file(ks: &KnowledgeStore, file_id: i64) -> Result<(String, i64)> {
    Ok(ks.conn().query_row(
        "SELECT ks.label, ks.id
         FROM knowledge_files kf
         JOIN knowledge_sources ks ON ks.id = kf.source_id
         WHERE kf.id = ?1",
        rusqlite::params![file_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?)
}
