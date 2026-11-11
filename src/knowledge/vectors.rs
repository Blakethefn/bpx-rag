use anyhow::Result;

use super::{ann, filter};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct VectorIndexState {
    model_id: String,
    dimensions: usize,
    generation: i64,
    database_id: String,
}

impl super::KnowledgeStore {
    pub fn ensure_vector_index(&mut self, model_id: &str, dimensions: usize) -> Result<()> {
        self.conn.execute_batch("BEGIN")?;
        let result = (|| -> Result<(VectorIndexState, Option<ann::VectorIndex>)> {
            let state = self.conn.query_row(
                "SELECT generation, database_id FROM knowledge_embedding_state WHERE state_id = 1",
                [],
                |row| {
                    Ok(VectorIndexState {
                        model_id: model_id.to_string(),
                        dimensions,
                        generation: row.get(0)?,
                        database_id: row.get(1)?,
                    })
                },
            )?;
            if self.vector_index_state.as_ref() == Some(&state) {
                return Ok((state, None));
            }
            let index = ann::VectorIndex::load_or_build(
                &self.conn,
                self.vector_index_dir.as_deref(),
                &state.model_id,
                state.dimensions,
                state.generation,
                &state.database_id,
            )?;
            Ok((state, index))
        })();
        match result {
            Ok((state, index)) => {
                self.conn.execute_batch("COMMIT")?;
                if self.vector_index_state.as_ref() != Some(&state) {
                    self.vector_index = index;
                    self.vector_index_state = Some(state);
                }
                Ok(())
            }
            Err(error) => {
                let _ = self.conn.execute_batch("ROLLBACK");
                Err(error)
            }
        }
    }

    pub(crate) fn clear_vector_index(&mut self) {
        self.vector_index = None;
        self.vector_index_state = None;
    }

    pub(crate) fn vector_index_memory_usage(&self) -> usize {
        self.vector_index
            .as_ref()
            .map_or(0, ann::VectorIndex::memory_usage_bytes)
    }

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

        let allowed = if predicates.is_empty() {
            None
        } else {
            let (meta_where, meta_params) = filter::predicates_to_sql(predicates, 1);
            let metadata_clause = if meta_where.is_empty() {
                String::new()
            } else {
                format!(" AND {meta_where}")
            };
            let sql = format!(
                "SELECT ke.chunk_rowid
                 FROM knowledge_embeddings ke
                 JOIN knowledge_chunk_meta kcm ON kcm.chunk_rowid = ke.chunk_rowid
                 JOIN knowledge_files kf ON kf.id = kcm.file_id
                 WHERE ke.model_id = ?1{metadata_clause}"
            );
            let mut params: Vec<Box<dyn rusqlite::types::ToSql>> =
                vec![Box::new(model_id.to_string())];
            for value in meta_params {
                params.push(Box::new(value));
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
