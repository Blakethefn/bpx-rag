use anyhow::Result;
use rusqlite::types::ToSql;

use crate::knowledge::filter::{FilterPredicate, file_scope_sql, predicates_to_sql};

use super::RagApp;

#[cfg(test)]
mod bench;
#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::app) struct EmbeddingCoverage {
    pub(in crate::app) total: i64,
    pub(in crate::app) current_model: i64,
}

impl EmbeddingCoverage {
    pub(in crate::app) fn notice(self, scoped: bool) -> Option<String> {
        (self.total > self.current_model).then(|| {
            let scope = if scoped {
                "chunks matching this search scope"
            } else {
                "indexed chunks"
            };
            format!(
                "The active model has vectors for {}/{} {scope}; run `bpx-rag embed` to build or replace the remaining vectors.",
                self.current_model, self.total
            )
        })
    }
}

impl RagApp {
    pub(in crate::app) fn embedding_coverage(
        &self,
        predicates: &[FilterPredicate],
    ) -> Result<Option<EmbeddingCoverage>> {
        let Some(embedder) = self.knowledge.embedder() else {
            return Ok(None);
        };
        let (sql, scope_values) = match file_scope_sql(predicates, 1) {
            Some((scope_where, values)) => {
                let scope_clause = if scope_where.is_empty() {
                    String::new()
                } else {
                    format!(" WHERE {scope_where}")
                };
                (
                    format!(
                        "SELECT COALESCE(SUM(kf.chunk_count), 0),
                                COALESCE(SUM(ec.embedded_count), 0)
                         FROM knowledge_files kf
                         LEFT JOIN knowledge_embedding_coverage ec
                           ON ec.file_id = kf.id AND ec.model_id = ?1{scope_clause}"
                    ),
                    values,
                )
            }
            None => {
                let (scope_where, values) = predicates_to_sql(predicates, 1);
                let scope_clause = if scope_where.is_empty() {
                    String::new()
                } else {
                    format!(" WHERE {scope_where}")
                };
                let file_join = if predicates.iter().any(|predicate| {
                    matches!(
                        predicate,
                        FilterPredicate::Language(_)
                            | FilterPredicate::Directory(_)
                            | FilterPredicate::File(_)
                    )
                }) {
                    "JOIN knowledge_files kf ON kf.id = kcm.file_id"
                } else {
                    ""
                };
                (
                    format!(
                        "SELECT COUNT(kcm.chunk_rowid),
                                COUNT(CASE WHEN e.model_id = ?1 THEN e.chunk_rowid END)
                         FROM knowledge_chunk_meta kcm
                         {file_join}
                         LEFT JOIN knowledge_embeddings e ON e.chunk_rowid = kcm.chunk_rowid{scope_clause}"
                    ),
                    values,
                )
            }
        };
        let mut params: Vec<Box<dyn ToSql>> = vec![Box::new(embedder.model_id().to_string())];
        params.extend(
            scope_values
                .into_iter()
                .map(|value| Box::new(value) as Box<dyn ToSql>),
        );
        let param_refs: Vec<&dyn ToSql> = params.iter().map(|param| param.as_ref()).collect();
        let (total, current_model) =
            self.knowledge
                .conn()
                .query_row(&sql, param_refs.as_slice(), |row| {
                    Ok((row.get(0)?, row.get(1)?))
                })?;
        Ok(Some(EmbeddingCoverage {
            total,
            current_model,
        }))
    }
}
