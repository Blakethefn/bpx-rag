use anyhow::Result;
use rusqlite::Connection;

pub(super) fn scoped_rowid_range(
    conn: &Connection,
    meta_where: &str,
    meta_params: &[String],
) -> Result<Option<(i64, i64)>> {
    if !meta_where.contains("kcm.source_id") && !meta_where.contains("kcm.file_id") {
        return Ok(None);
    }

    let file_join = if meta_where.contains("lower(kf.rel_path)") {
        "JOIN knowledge_files kf ON kf.id = kcm.file_id"
    } else {
        ""
    };
    let sql = format!(
        "SELECT MIN(kcm.chunk_rowid), MAX(kcm.chunk_rowid)
         FROM knowledge_chunk_meta kcm
         {file_join}
         WHERE {meta_where}"
    );
    let params: Vec<&dyn rusqlite::types::ToSql> =
        meta_params.iter().map(|param| param as _).collect();
    let range = conn.query_row(&sql, params.as_slice(), |row| {
        Ok((row.get::<_, Option<i64>>(0)?, row.get::<_, Option<i64>>(1)?))
    })?;

    Ok(range.0.zip(range.1))
}
