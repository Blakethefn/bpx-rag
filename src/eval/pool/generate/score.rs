use super::super::super::sha256;
use super::super::super::QueryAnswerability;
use super::super::output::opaque_id;
use super::super::{
    JudgmentPoolCandidate, JudgmentPoolEvidence, JudgmentPoolKeyEntry, PoolCandidateAccumulator,
    PoolChannel,
};
use super::load::LoadedPool;
use crate::knowledge;
use anyhow::Result;
use std::collections::BTreeMap;

pub(super) fn score_candidates(
    loaded: &LoadedPool,
    depth: usize,
    seed: u64,
) -> Result<(Vec<JudgmentPoolCandidate>, Vec<JudgmentPoolKeyEntry>)> {
    let mut pool_candidates = Vec::new();
    let mut key_candidates = Vec::new();
    for query in &loaded.queries {
        let blinded_query_id = opaque_id(seed, "query", &query.query_id);
        let mut candidates: BTreeMap<String, PoolCandidateAccumulator> = BTreeMap::new();
        for channel in PoolChannel::ALL {
            let weights = channel.weights();
            let results = knowledge::query::knowledge_search(
                &loaded.store,
                &query.text,
                query.filter.as_deref(),
                Some("judgment-pool"),
                depth as u32,
                &weights,
                1200,
            )?;
            for (offset, result) in results.into_iter().enumerate() {
                let candidate = candidates.entry(result.file_path.clone()).or_default();
                let evidence_key = format!(
                    "{}:{}:{}",
                    result.lines,
                    result.title,
                    sha256(&result.snippet)
                );
                candidate
                    .evidence
                    .entry(evidence_key)
                    .or_insert(JudgmentPoolEvidence {
                        title: result.title,
                        lines: result.lines,
                        snippet: result.snippet,
                    });
                channel.record_rank(&mut candidate.retrieved_by, offset + 1);
            }
        }

        if query.answerability == QueryAnswerability::UnanswerableInCorpus {
            for (path, evidence) in indexed_corpus_evidence(&loaded.store, loaded.source_id)? {
                let candidate = candidates.entry(path.clone()).or_default();
                for evidence in evidence {
                    let evidence_key = format!(
                        "{}:{}:{}",
                        evidence.lines,
                        evidence.title,
                        sha256(&evidence.snippet)
                    );
                    candidate.evidence.entry(evidence_key).or_insert(evidence);
                }
            }
        }

        for (path, candidate) in candidates {
            let candidate_id =
                opaque_id(seed, "candidate", &format!("{}:{path}", blinded_query_id));
            pool_candidates.push(JudgmentPoolCandidate {
                blinded_query_id: blinded_query_id.clone(),
                query: query.text.clone(),
                candidate_id: candidate_id.clone(),
                path: path.clone(),
                evidence: candidate.evidence.into_values().collect(),
                grade: None,
                reviewer_notes: None,
            });
            key_candidates.push(JudgmentPoolKeyEntry {
                candidate_id,
                blinded_query_id: blinded_query_id.clone(),
                query_id: query.query_id.clone(),
                path,
                retrieved_by: candidate.retrieved_by,
            });
        }
    }
    Ok((pool_candidates, key_candidates))
}

pub(in crate::eval) fn indexed_corpus_evidence(
    store: &knowledge::KnowledgeStore,
    source_id: i64,
) -> Result<BTreeMap<String, Vec<JudgmentPoolEvidence>>> {
    let mut statement = store.conn().prepare(
        "SELECT kf.rel_path, kc.title, kc.content, kcm.line_start, kcm.line_end
         FROM knowledge_files kf
         LEFT JOIN knowledge_chunk_meta kcm ON kcm.file_id = kf.id
         LEFT JOIN knowledge_chunks kc ON kc.rowid = kcm.chunk_rowid
         WHERE kf.source_id = ?1
         ORDER BY kf.rel_path, kcm.line_start, kcm.line_end, kcm.chunk_rowid",
    )?;
    let mut files: BTreeMap<String, Vec<JudgmentPoolEvidence>> = BTreeMap::new();
    let rows = statement.query_map([source_id], |row| {
        let path: String = row.get(0)?;
        let title: Option<String> = row.get(1)?;
        let snippet: Option<String> = row.get(2)?;
        let line_start: Option<i64> = row.get(3)?;
        let line_end: Option<i64> = row.get(4)?;
        Ok((path, title, snippet, line_start, line_end))
    })?;
    for row in rows {
        let (path, title, snippet, line_start, line_end) = row?;
        let evidence = files.entry(path).or_default();
        if let (Some(title), Some(snippet), Some(line_start), Some(line_end)) =
            (title, snippet, line_start, line_end)
        {
            evidence.push(JudgmentPoolEvidence {
                title,
                lines: format!("{line_start}-{line_end}"),
                snippet,
            });
        }
    }
    Ok(files)
}
