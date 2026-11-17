use anyhow::Result;
use rusqlite::{Connection, OptionalExtension};
use std::collections::HashSet;

use crate::knowledge::{filter, KnowledgeStore};

mod language;
pub(super) use language::global_language_rowids;

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
            let path_rowids = if state.source_id.is_none() {
                global_path_rowids(&self.conn, model_id, &scoped_predicates)?
            } else {
                None
            };
            let language_rowids = if path_rowids.is_none() && state.source_id.is_none() {
                global_language_rowids(&self.conn, model_id, &scoped_predicates)?
            } else {
                None
            };
            let rowids = if let Some(rowids) = path_rowids.or(language_rowids) {
                if rowids.is_empty() {
                    return Ok(Some(Vec::new()));
                }
                rowids
            } else {
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
                    .collect::<rusqlite::Result<HashSet<_>>>()?;
                rowids
            };
            if rowids.is_empty() {
                return Ok(Some(Vec::new()));
            }
            Some(rowids)
        };

        index.candidates(query_embedding, limit as usize, allowed.as_ref())
    }
}

pub(super) fn global_path_rowids(
    conn: &Connection,
    model_id: &str,
    predicates: &[filter::FilterPredicate],
) -> Result<Option<HashSet<i64>>> {
    let path_predicates: Vec<_> = predicates
        .iter()
        .filter(|predicate| {
            matches!(
                predicate,
                filter::FilterPredicate::Directory(_) | filter::FilterPredicate::File(_)
            )
        })
        .cloned()
        .collect();
    if path_predicates.is_empty() {
        return Ok(None);
    }

    let residual: Vec<_> = predicates
        .iter()
        .filter(|predicate| {
            !matches!(
                predicate,
                filter::FilterPredicate::Directory(_) | filter::FilterPredicate::File(_)
            )
        })
        .cloned()
        .collect();
    let (file_scope, file_params) = filter::file_scope_sql(&path_predicates, 1)
        .ok_or_else(|| anyhow::anyhow!("path filters did not produce a file scope"))?;
    let (metadata_where, metadata_params) =
        filter::predicates_to_sql(&residual, file_params.len() + 1);
    let metadata_clause = if metadata_where.is_empty() {
        String::new()
    } else {
        format!(" AND {metadata_where}")
    };
    let sql = format!(
        "SELECT ke.chunk_rowid
         FROM knowledge_files kf INDEXED BY idx_files_rel_path
         CROSS JOIN knowledge_chunk_meta kcm INDEXED BY idx_chunk_meta_file
         CROSS JOIN knowledge_embeddings ke INDEXED BY idx_embeddings_model
         WHERE {file_scope}
           AND kcm.file_id = kf.id
           AND ke.chunk_rowid = kcm.chunk_rowid
           AND ke.model_id = ?1{metadata_clause}"
    );
    let params: Vec<Box<dyn rusqlite::types::ToSql>> =
        std::iter::once(Box::new(model_id.to_string()) as _)
            .chain(file_params.into_iter().map(|value| Box::new(value) as _))
            .chain(
                metadata_params
                    .into_iter()
                    .map(|value| Box::new(value) as _),
            )
            .collect();
    let param_refs = params
        .iter()
        .map(|param| param.as_ref())
        .collect::<Vec<_>>();
    let mut statement = conn.prepare(&sql)?;
    let rowids = statement
        .query_map(param_refs.as_slice(), |row| row.get::<_, i64>(0))?
        .collect::<rusqlite::Result<HashSet<_>>>()?;
    Ok(Some(rowids))
}
