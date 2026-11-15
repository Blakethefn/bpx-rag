use anyhow::Result;
use rusqlite::OptionalExtension;

use crate::knowledge::{filter, KnowledgeStore};

impl KnowledgeStore {
    pub(crate) fn vector_candidates(
        &self,
        model_id: &str,
        query_embedding: &[f32],
        limit: u32,
        predicates: &[filter::FilterPredicate],
    ) -> Result<Option<Vec<i64>>> {
        let Some(index) = &self.vector_index else {
            return Ok(None);
        };
        let Some(state) = &self.vector_index_state else {
            return Ok(None);
        };
        if state.model_id != model_id || state.dimensions != query_embedding.len() {
            return Ok(None);
        }

        if let Some(index_source_id) = state.source_id {
            let source_id = match filter::single_source_predicate(predicates) {
                Some(label) => self
                    .conn
                    .query_row(
                        "SELECT id FROM knowledge_sources WHERE label = ?1",
                        rusqlite::params![label],
                        |row| row.get::<_, i64>(0),
                    )
                    .optional()?,
                None => None,
            };
            if source_id != Some(index_source_id) {
                return Ok(None);
            }
        }

        let scoped_predicates = if state.source_id.is_some() {
            predicates
                .iter()
                .filter(|predicate| !matches!(predicate, filter::FilterPredicate::Source(_)))
                .cloned()
                .collect::<Vec<_>>()
        } else {
            predicates.to_vec()
        };

        let allowed = if scoped_predicates.is_empty() {
            None
        } else {
            let (meta_where, meta_params) = filter::predicates_to_sql(&scoped_predicates, 1);
            let metadata_clause = if meta_where.is_empty() {
                String::new()
            } else {
                format!(" AND {meta_where}")
            };
            let sql = match state.source_id {
                Some(_) => format!(
                    "SELECT ke.chunk_rowid
                     FROM knowledge_chunk_meta kcm INDEXED BY idx_chunk_meta_source
                     CROSS JOIN knowledge_embeddings ke
                     JOIN knowledge_files kf ON kf.id = kcm.file_id
                     WHERE kcm.source_id = ?{} AND ke.chunk_rowid = kcm.chunk_rowid
                       AND ke.model_id = ?1{metadata_clause}",
                    meta_params.len() + 2
                ),
                None => format!(
                    "SELECT ke.chunk_rowid
                     FROM knowledge_embeddings ke
                     JOIN knowledge_chunk_meta kcm ON kcm.chunk_rowid = ke.chunk_rowid
                     JOIN knowledge_files kf ON kf.id = kcm.file_id
                     WHERE ke.model_id = ?1{metadata_clause}"
                ),
            };
            let mut params: Vec<Box<dyn rusqlite::types::ToSql>> =
                vec![Box::new(model_id.to_string())];
            for value in meta_params {
                params.push(Box::new(value));
            }
            if let Some(source_id) = state.source_id {
                params.push(Box::new(source_id));
            }
            let param_refs = params
                .iter()
                .map(|param| param.as_ref())
                .collect::<Vec<_>>();
            let mut statement = self.conn.prepare(&sql)?;
            let rowids = statement
                .query_map(param_refs.as_slice(), |row| row.get::<_, i64>(0))?
                .collect::<rusqlite::Result<std::collections::HashSet<_>>>()?;
            if rowids.is_empty() {
                return Ok(Some(Vec::new()));
            }
            Some(rowids)
        };

        index.candidates(query_embedding, limit as usize, allowed.as_ref())
    }
}
