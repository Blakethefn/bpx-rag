use anyhow::Result;

use super::ann;

mod candidates;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct VectorIndexState {
    model_id: String,
    dimensions: usize,
    generation: i64,
    database_id: String,
    source_id: Option<i64>,
}

impl super::KnowledgeStore {
    pub(crate) fn ensure_vector_index_for_source(
        &mut self,
        model_id: &str,
        dimensions: usize,
        source_label: Option<&str>,
    ) -> Result<()> {
        let Some(source_label) = source_label else {
            return self.ensure_vector_index(model_id, dimensions);
        };
        let Some(source) = self.get_source(source_label)? else {
            self.clear_vector_index();
            return Ok(());
        };
        if source.chunk_count < ann::ANN_MIN_VECTORS as i64 {
            self.clear_vector_index();
            return Ok(());
        }
        self.ensure_vector_index_scope(model_id, dimensions, Some(source.id))
    }

    pub fn ensure_vector_index(&mut self, model_id: &str, dimensions: usize) -> Result<()> {
        self.ensure_vector_index_scope(model_id, dimensions, None)
    }

    fn ensure_vector_index_scope(
        &mut self,
        model_id: &str,
        dimensions: usize,
        source_id: Option<i64>,
    ) -> Result<()> {
        self.conn.execute_batch("BEGIN")?;
        let result = (|| -> Result<(VectorIndexState, Option<ann::VectorIndex>)> {
            let (global_generation, database_id) = self.conn.query_row(
                "SELECT generation, database_id FROM knowledge_embedding_state WHERE state_id = 1",
                [],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
            )?;
            let generation = match source_id {
                Some(source_id) => ann::source_generation(&self.conn, source_id)?,
                None => global_generation,
            };
            let state = VectorIndexState {
                model_id: model_id.to_string(),
                dimensions,
                generation,
                database_id,
                source_id,
            };
            if self.vector_index_state.as_ref() == Some(&state) {
                return Ok((state, None));
            }
            let index = ann::VectorIndex::load_or_build_scoped(
                &self.conn,
                self.vector_index_dir.as_deref(),
                &state.model_id,
                state.dimensions,
                state.generation,
                &state.database_id,
                state.source_id,
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
}
