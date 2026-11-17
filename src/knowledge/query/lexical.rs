use super::rank::RankedResult;

mod bm25;
mod identifier;
mod sanitize;
mod scope;
#[cfg(test)]
mod tests;
mod trigram;

pub(super) use bm25::knowledge_bm25_search;
pub(in crate::knowledge::query) use identifier::{
    identifier_match_expression, knowledge_identifier_search,
};
pub(super) use sanitize::{sanitize_fts_query, sanitize_trigram_query};
pub(super) use trigram::knowledge_trigram_search;

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
