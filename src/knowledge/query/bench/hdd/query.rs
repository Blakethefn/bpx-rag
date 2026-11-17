use anyhow::Result;
use rusqlite::{types::ToSql, Connection};

use crate::knowledge::filter::{predicates_to_sql, FilterPredicate};

use super::super::super::vector::dot_product;
use super::super::hdd::MODEL;

const DIMENSIONS: usize = 768;

pub(super) fn deterministic_query_vector() -> Vec<f32> {
    let mut vector = (0..DIMENSIONS)
        .map(|index| ((index * 37 + 11) % 101) as f32 - 50.0)
        .collect::<Vec<_>>();
    let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    vector.iter_mut().for_each(|value| *value /= norm);
    vector
}

pub(super) fn legacy_sql(predicates: &[FilterPredicate]) -> (String, Vec<Box<dyn ToSql>>) {
    let (where_sql, values) = predicates_to_sql(predicates, 0);
    let clause = if where_sql.is_empty() {
        String::new()
    } else {
        format!(" AND {where_sql}")
    };
    let model_param = values.len() + 1;
    let sql = format!(
        "SELECT kc.rowid, ke.embedding, kcm.file_id, kcm.source_id, kc.title, kc.content,
                COALESCE(kcm.line_start,0), COALESCE(kcm.line_end,0), COALESCE(kcm.metadata,'{{}}'),
                COALESCE(ks.label,''), COALESCE(kf.rel_path,'')
         FROM knowledge_embeddings ke JOIN knowledge_chunks kc ON kc.rowid=ke.chunk_rowid
         JOIN knowledge_chunk_meta kcm ON kcm.chunk_rowid=ke.chunk_rowid
         JOIN knowledge_files kf ON kf.id=kcm.file_id JOIN knowledge_sources ks ON ks.id=kcm.source_id
         WHERE 1=1{clause} AND ke.model_id=?{model_param}"
    );
    let mut params = values
        .into_iter()
        .map(|value| Box::new(value) as Box<dyn ToSql>)
        .collect::<Vec<_>>();
    params.push(Box::new(MODEL.to_string()));
    (sql, params)
}

pub(super) fn file_first_sql(
    file_scope: &str,
    file_params: &[String],
    predicates: &[FilterPredicate],
) -> Result<(String, Vec<Box<dyn ToSql>>)> {
    let residual = predicates
        .iter()
        .filter(|predicate| {
            !matches!(
                predicate,
                FilterPredicate::Directory(_) | FilterPredicate::File(_)
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    let (residual_where, residual_params) = predicates_to_sql(&residual, file_params.len());
    let residual_clause = if residual_where.is_empty() {
        String::new()
    } else {
        format!(" AND {residual_where}")
    };
    let model_param = file_params.len() + residual_params.len() + 1;
    let sql = format!(
        "SELECT kc.rowid, ke.embedding, kcm.file_id, kcm.source_id, kc.title, kc.content,
                COALESCE(kcm.line_start,0), COALESCE(kcm.line_end,0), COALESCE(kcm.metadata,'{{}}'),
                COALESCE(ks.label,''), COALESCE(kf.rel_path,'')
         FROM knowledge_files kf INDEXED BY idx_files_rel_path
         CROSS JOIN knowledge_chunk_meta kcm INDEXED BY idx_chunk_meta_file
         CROSS JOIN knowledge_embeddings ke CROSS JOIN knowledge_chunks kc CROSS JOIN knowledge_sources ks
         WHERE {file_scope} AND kcm.file_id=kf.id AND ke.chunk_rowid=kcm.chunk_rowid
           AND kc.rowid=ke.chunk_rowid AND ks.id=kcm.source_id
           AND ke.model_id=?{model_param}{residual_clause} ORDER BY ke.chunk_rowid"
    );
    let mut params = file_params
        .iter()
        .cloned()
        .map(|value| Box::new(value) as Box<dyn ToSql>)
        .collect::<Vec<_>>();
    params.extend(
        residual_params
            .into_iter()
            .map(|value| Box::new(value) as _),
    );
    params.push(Box::new(MODEL.to_string()));
    Ok((sql, params))
}

pub(super) fn legacy_rows(
    conn: &Connection,
    sql: &str,
    params: &[Box<dyn ToSql>],
) -> Result<Vec<(i64, Vec<u8>)>> {
    let refs = sql_params(params);
    Ok(conn
        .prepare(sql)?
        .query_map(refs.as_slice(), |row| {
            let rowid = row.get::<_, i64>(0)?;
            let blob = row.get::<_, Vec<u8>>(1)?;
            let _: i64 = row.get(2)?;
            let _: i64 = row.get(3)?;
            let _: String = row.get(4)?;
            let _: String = row.get(5)?;
            let _: i64 = row.get(6)?;
            let _: i64 = row.get(7)?;
            let _: String = row.get(8)?;
            let _: String = row.get(9)?;
            let _: String = row.get(10)?;
            Ok((rowid, blob))
        })?
        .collect::<rusqlite::Result<_>>()?)
}

pub(super) fn score_rows(query: &[f32], rows: Vec<(i64, Vec<u8>)>) -> Result<Vec<(i64, f32)>> {
    let mut scored = rows
        .into_iter()
        .map(|(rowid, bytes)| {
            let vector = crate::embed::bytes_to_embedding(&bytes);
            anyhow::ensure!(
                vector.len() == query.len(),
                "stored embedding dimension mismatch"
            );
            Ok((rowid, dot_product(query, &vector)))
        })
        .collect::<Result<Vec<_>>>()?;
    scored.sort_by(|left, right| {
        right
            .1
            .partial_cmp(&left.1)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(scored)
}

pub(super) fn score_ids(conn: &Connection, query: &[f32], rowids: &[i64]) -> Result<Vec<f32>> {
    let mut statement = conn.prepare(
        "SELECT embedding FROM knowledge_embeddings WHERE chunk_rowid = ?1 AND model_id = ?2",
    )?;
    rowids
        .iter()
        .map(|rowid| {
            let bytes: Vec<u8> =
                statement.query_row(rusqlite::params![rowid, MODEL], |row| row.get(0))?;
            let vector = crate::embed::bytes_to_embedding(&bytes);
            anyhow::ensure!(
                vector.len() == query.len(),
                "stored embedding dimension mismatch"
            );
            Ok(dot_product(query, &vector))
        })
        .collect()
}

fn sql_params(params: &[Box<dyn ToSql>]) -> Vec<&dyn ToSql> {
    params.iter().map(|param| param.as_ref()).collect()
}
