use anyhow::Result;
use rusqlite::{types::ToSql, Connection};

use crate::knowledge::filter::{file_scope_sql, FilterPredicate};

use super::EmbeddingCoverage;

pub(super) fn embedding_coverage(
    conn: &Connection,
    model_id: &str,
    predicates: &[FilterPredicate],
) -> Result<Option<EmbeddingCoverage>> {
    let languages: Vec<_> = predicates
        .iter()
        .filter_map(|predicate| match predicate {
            FilterPredicate::Language(language) => Some(language.as_str()),
            _ => None,
        })
        .collect();
    if languages.len() != 1
        || predicates.iter().any(|predicate| {
            !matches!(
                predicate,
                FilterPredicate::Language(_)
                    | FilterPredicate::Source(_)
                    | FilterPredicate::Directory(_)
                    | FilterPredicate::File(_)
            )
        })
    {
        return Ok(None);
    }

    let language = languages[0];
    let structural: Vec<_> = predicates
        .iter()
        .filter(|predicate| !matches!(predicate, FilterPredicate::Language(_)))
        .cloned()
        .collect();
    let mut values: Vec<Box<dyn ToSql>> = vec![Box::new(model_id.to_string())];
    let path_match = path_match(language, &mut values);
    let (file_scope, file_scope_values) =
        file_scope_sql(&structural, values.len()).unwrap_or_default();
    values.extend(
        file_scope_values
            .into_iter()
            .map(|value| Box::new(value) as _),
    );
    let file_scope = if file_scope.is_empty() {
        String::new()
    } else {
        format!(" AND {file_scope}")
    };
    let file_sql = format!(
        "SELECT COALESCE(SUM(kf.chunk_count), 0),
                COALESCE(SUM(ec.embedded_count), 0)
         FROM knowledge_files kf
         LEFT JOIN knowledge_embedding_coverage ec
           ON ec.file_id = kf.id AND ec.model_id = ?1
         WHERE {path_match}{file_scope}"
    );
    let refs = sql_params(&values);
    let (mut total, mut current_model) = conn.query_row(&file_sql, refs.as_slice(), |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
    })?;

    let metadata_values = frontmatter_languages(language);
    let frontmatter_match = placeholders(
        "CASE WHEN json_valid(kcm.metadata) THEN lower(json_extract(kcm.metadata, '$.frontmatter.language')) END",
        &metadata_values,
        values.len(),
    );
    values.extend(
        metadata_values
            .into_iter()
            .map(|value| Box::new(value) as _),
    );
    let (scope_sql, scope_values) = file_scope_sql(&structural, values.len()).unwrap_or_default();
    values.extend(scope_values.into_iter().map(|value| Box::new(value) as _));
    let metadata_scope = if scope_sql.is_empty() {
        String::new()
    } else {
        format!(" AND {scope_sql}")
    };
    let metadata_sql = format!(
        "SELECT COUNT(kcm.chunk_rowid),
                COUNT(CASE WHEN e.model_id = ?1 THEN e.chunk_rowid END)
         FROM knowledge_chunk_meta kcm INDEXED BY idx_chunk_meta_language
         JOIN knowledge_files kf ON kf.id = kcm.file_id
         LEFT JOIN knowledge_embeddings e ON e.chunk_rowid = kcm.chunk_rowid
         WHERE {frontmatter_match} AND NOT ({path_match}){metadata_scope}"
    );
    let refs = sql_params(&values);
    let (metadata_total, metadata_vectors) =
        conn.query_row(&metadata_sql, refs.as_slice(), |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
        })?;
    total += metadata_total;
    current_model += metadata_vectors;

    Ok(Some(EmbeddingCoverage {
        total,
        current_model,
    }))
}

fn path_match(language: &str, values: &mut Vec<Box<dyn ToSql>>) -> String {
    let extensions = crate::language::extensions_for_name(language);
    let mut path_conditions = Vec::new();
    if !extensions.is_empty() {
        let placeholders = values.len()..values.len() + extensions.len();
        path_conditions.push(format!(
            "kf.extension IN ({})",
            placeholders
                .map(|index| format!("?{}", index + 1))
                .collect::<Vec<_>>()
                .join(", ")
        ));
        values.extend(extensions.iter().cloned().map(|value| Box::new(value) as _));

        let old_file_placeholders = values.len()..values.len() + extensions.len();
        path_conditions.push(format!(
            "(kf.extension = '' AND ({}))",
            old_file_placeholders
                .map(|index| format!("lower(kf.rel_path) LIKE ?{}", index + 1))
                .collect::<Vec<_>>()
                .join(" OR ")
        ));
        values.extend(
            extensions
                .iter()
                .map(|extension| Box::new(format!("%.{extension}")) as _),
        );
    }

    for file_name in crate::language::file_names_for_name(language) {
        let exact_index = values.len() + 1;
        path_conditions.push(format!("lower(kf.rel_path) = ?{exact_index}"));
        values.push(Box::new(file_name.to_string()));
        let suffix_index = values.len() + 1;
        path_conditions.push(format!("lower(kf.rel_path) LIKE ?{suffix_index}"));
        values.push(Box::new(format!("%/{file_name}")));
    }
    if path_conditions.is_empty() {
        "0".to_string()
    } else {
        format!("({})", path_conditions.join(" OR "))
    }
}

fn frontmatter_languages(language: &str) -> Vec<String> {
    crate::language::aliases_for_name(language).map_or_else(
        || vec![language.to_ascii_lowercase()],
        |aliases| aliases.iter().map(|alias| alias.to_string()).collect(),
    )
}

fn placeholders(expression: &str, values: &[String], offset: usize) -> String {
    let placeholders = (offset..offset + values.len())
        .map(|index| format!("?{}", index + 1))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{expression} IN ({placeholders})")
}

fn sql_params(values: &[Box<dyn ToSql>]) -> Vec<&dyn ToSql> {
    values.iter().map(|value| value.as_ref()).collect()
}
