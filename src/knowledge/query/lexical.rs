use super::rank::RankedResult;
use anyhow::Result;
use rusqlite::Connection;

mod identifier;
#[cfg(test)]
mod tests;
pub(in crate::knowledge::query) use identifier::{
    identifier_match_expression, knowledge_identifier_search,
};

const CHUNK_COLUMNS: [ChunkColumn; 10] = [
    ChunkColumn::Rowid,
    ChunkColumn::FileId,
    ChunkColumn::SourceId,
    ChunkColumn::Title,
    ChunkColumn::Content,
    ChunkColumn::LineStart,
    ChunkColumn::LineEnd,
    ChunkColumn::Metadata,
    ChunkColumn::SourceLabel,
    ChunkColumn::FilePath,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ChunkColumn {
    Rowid,
    FileId,
    SourceId,
    Title,
    Content,
    LineStart,
    LineEnd,
    Metadata,
    SourceLabel,
    FilePath,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Selected {
    Chunk(ChunkColumn),
    Rank,
    Embedding,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct BoundColumn {
    pub(super) selected: Selected,
    pub(super) sql: String,
}

pub(super) fn chunk_columns(chunk: &str) -> Vec<BoundColumn> {
    CHUNK_COLUMNS
        .into_iter()
        .map(|column| BoundColumn {
            selected: Selected::Chunk(column),
            sql: chunk_column_sql(column, chunk),
        })
        .collect()
}

fn lexical_columns(chunk: &str, rank_expr: &str) -> Vec<BoundColumn> {
    let mut columns = chunk_columns(chunk);
    columns.push(BoundColumn {
        selected: Selected::Rank,
        sql: format!("{rank_expr} as rank"),
    });
    columns
}

pub(super) fn select_list(columns: &[BoundColumn]) -> String {
    columns
        .iter()
        .map(|column| column.sql.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

pub(super) fn column_at(columns: &[BoundColumn], selected: Selected) -> usize {
    columns
        .iter()
        .position(|column| column.selected == selected)
        .expect("select list includes the column")
}

fn chunk_column_sql(column: ChunkColumn, chunk: &str) -> String {
    match column {
        ChunkColumn::Rowid => format!("{chunk}.rowid"),
        ChunkColumn::FileId => "kcm.file_id".to_string(),
        ChunkColumn::SourceId => "kcm.source_id".to_string(),
        ChunkColumn::Title => format!("{chunk}.title"),
        ChunkColumn::Content => format!("{chunk}.content"),
        ChunkColumn::LineStart => "COALESCE(kcm.line_start, 0)".to_string(),
        ChunkColumn::LineEnd => "COALESCE(kcm.line_end, 0)".to_string(),
        ChunkColumn::Metadata => "COALESCE(kcm.metadata, '{}')".to_string(),
        ChunkColumn::SourceLabel => "COALESCE(ks.label, '') as source_label".to_string(),
        ChunkColumn::FilePath => "COALESCE(kf.rel_path, '') as file_path".to_string(),
    }
}

pub(super) fn knowledge_bm25_search(
    conn: &Connection,
    query: &str,
    limit: u32,
    meta_where: &str,
    meta_params: &[String],
) -> Result<Vec<RankedResult>> {
    let fts_query = sanitize_fts_query(query);
    if fts_query.is_empty() {
        return Ok(Vec::new());
    }

    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    for p in meta_params {
        params.push(Box::new(p.clone()));
    }

    let fts_idx = params.len() + 1;
    params.push(Box::new(fts_query));

    let meta_clause = if meta_where.is_empty() {
        String::new()
    } else {
        format!(" AND {meta_where}")
    };

    let columns = lexical_columns("kc", "bm25(knowledge_chunks)");
    let select = select_list(&columns);
    let sql = format!(
        "SELECT {select}
         FROM knowledge_chunks kc
         JOIN knowledge_chunk_meta kcm ON kcm.chunk_rowid = kc.rowid
         JOIN knowledge_files kf ON kf.id = kcm.file_id
         JOIN knowledge_sources ks ON ks.id = kcm.source_id
         WHERE knowledge_chunks MATCH ?{fts_idx}{meta_clause}
         ORDER BY rank
         LIMIT ?{}",
        params.len() + 1
    );
    params.push(Box::new(limit));

    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let results = stmt
        .query_map(param_refs.as_slice(), |row| {
            ranked_result_from_row(row, &columns)
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(results)
}

pub(super) fn ranked_result_from_row(
    row: &rusqlite::Row<'_>,
    columns: &[BoundColumn],
) -> rusqlite::Result<RankedResult> {
    let at = |column| column_at(columns, Selected::Chunk(column));
    let rank = if let Some(index) = columns
        .iter()
        .position(|column| column.selected == Selected::Rank)
    {
        row.get::<_, f64>(index)?.abs()
    } else {
        0.0
    };
    Ok(RankedResult {
        rowid: row.get(at(ChunkColumn::Rowid))?,
        file_id: row.get(at(ChunkColumn::FileId))?,
        source_id: row.get(at(ChunkColumn::SourceId))?,
        title: row.get(at(ChunkColumn::Title))?,
        content: row.get(at(ChunkColumn::Content))?,
        line_start: row.get::<_, i64>(at(ChunkColumn::LineStart))? as u32,
        line_end: row.get::<_, i64>(at(ChunkColumn::LineEnd))? as u32,
        metadata: row.get(at(ChunkColumn::Metadata))?,
        source_label: row.get(at(ChunkColumn::SourceLabel))?,
        file_path: row.get(at(ChunkColumn::FilePath))?,
        rank,
    })
}

pub(super) fn knowledge_trigram_search(
    conn: &Connection,
    query: &str,
    limit: u32,
    meta_where: &str,
    meta_params: &[String],
) -> Result<Vec<RankedResult>> {
    if query.len() < 3 {
        return Ok(Vec::new());
    }

    let fts_query = sanitize_trigram_query(query);
    if fts_query.is_empty() {
        return Ok(Vec::new());
    }

    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    for p in meta_params {
        params.push(Box::new(p.clone()));
    }

    let fts_idx = params.len() + 1;
    params.push(Box::new(fts_query));

    let meta_clause = if meta_where.is_empty() {
        String::new()
    } else {
        format!(" AND {meta_where}")
    };

    let columns = lexical_columns("kct", "bm25(knowledge_chunks_trigram)");
    let select = select_list(&columns);
    let sql = format!(
        "SELECT {select}
         FROM knowledge_chunks_trigram kct
         JOIN knowledge_chunk_meta kcm ON kcm.chunk_rowid = kct.rowid
         JOIN knowledge_files kf ON kf.id = kcm.file_id
         JOIN knowledge_sources ks ON ks.id = kcm.source_id
         WHERE knowledge_chunks_trigram MATCH ?{fts_idx}{meta_clause}
         ORDER BY rank
         LIMIT ?{}",
        params.len() + 1
    );
    params.push(Box::new(limit));

    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let results = stmt
        .query_map(param_refs.as_slice(), |row| {
            ranked_result_from_row(row, &columns)
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(results)
}

pub(super) fn sanitize_fts_query(query: &str) -> String {
    let cleaned = sanitize_fts_terms(query);
    let phrase = format!("\"{cleaned}\"");
    let terms = cleaned
        .split_whitespace()
        .take(32)
        .map(|term| format!("\"{term}\""))
        .collect::<Vec<_>>();
    if terms.is_empty() {
        return String::new();
    }
    std::iter::once(phrase)
        .chain(terms)
        .collect::<Vec<_>>()
        .join(" OR ")
}

pub(super) fn sanitize_fts_terms(query: &str) -> String {
    let mut cleaned = String::with_capacity(query.len());
    let mut last_was_space = true;
    for character in query.chars() {
        if character.is_alphanumeric() || character == '_' || character == '-' {
            cleaned.push(character);
            last_was_space = false;
        } else if !last_was_space {
            cleaned.push(' ');
            last_was_space = true;
        }
    }
    if cleaned.ends_with(' ') {
        cleaned.pop();
    }
    cleaned
}

pub(super) fn sanitize_trigram_query(query: &str) -> String {
    let cleaned = sanitize_fts_terms(query);
    if cleaned.is_empty() {
        return String::new();
    }
    format!("\"{cleaned}\"")
}
