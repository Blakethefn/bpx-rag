use anyhow::Result;
use rusqlite::Connection;
use std::collections::HashSet;

use crate::knowledge::filter::{self, FilterPredicate};

pub(in crate::knowledge::vectors) fn global_language_rowids(
    conn: &Connection,
    model_id: &str,
    predicates: &[FilterPredicate],
) -> Result<Option<HashSet<i64>>> {
    let mut languages = predicates.iter().filter_map(|predicate| match predicate {
        FilterPredicate::Language(language) => Some(language.as_str()),
        _ => None,
    });
    let Some(language) = languages.next() else {
        return Ok(None);
    };
    if languages.next().is_some() {
        return Ok(None);
    }

    let extensions = crate::language::extensions_for_name(language);
    let file_names = crate::language::file_names_for_name(language);
    let aliases = crate::language::aliases_for_name(language)
        .map(|names| names.iter().map(|name| name.to_ascii_lowercase()).collect())
        .unwrap_or_else(|| vec![language.to_ascii_lowercase()]);
    let residual = predicates
        .iter()
        .filter(|predicate| !matches!(predicate, FilterPredicate::Language(_)))
        .cloned()
        .collect::<Vec<_>>();
    let mut values = vec![model_id.to_string()];
    let mut branches = Vec::new();

    if !extensions.is_empty() || !file_names.is_empty() {
        let mut alternatives = Vec::new();
        if !extensions.is_empty() {
            let placeholders = extensions
                .iter()
                .map(|extension| {
                    values.push(extension.clone());
                    format!("?{}", values.len())
                })
                .collect::<Vec<_>>();
            alternatives.push(format!("kf.extension IN ({})", placeholders.join(", ")));
            let dotfile_suffixes = extensions
                .iter()
                .map(|extension| {
                    values.push(format!("%.{extension}"));
                    format!(
                        "(kf.extension = '' AND lower(kf.rel_path) LIKE ?{})",
                        values.len()
                    )
                })
                .collect::<Vec<_>>();
            alternatives.extend(dotfile_suffixes);
        }
        for file_name in file_names {
            let exact = values.len() + 1;
            values.push(file_name.to_string());
            let suffix = values.len() + 1;
            values.push(format!("%/{file_name}"));
            alternatives.push(format!(
                "lower(kf.rel_path) = ?{exact} OR lower(kf.rel_path) LIKE ?{suffix}"
            ));
        }
        let (where_sql, params) = filter::predicates_to_sql(&residual, values.len());
        let clause = where_sql
            .is_empty()
            .then(String::new)
            .unwrap_or_else(|| format!(" AND {where_sql}"));
        values.extend(params);
        branches.push(format!(
            "SELECT ke.chunk_rowid
             FROM knowledge_files kf INDEXED BY idx_files_extension
             CROSS JOIN knowledge_chunk_meta kcm INDEXED BY idx_chunk_meta_file
             CROSS JOIN knowledge_embeddings ke INDEXED BY idx_embeddings_model
             WHERE ke.model_id = ?1
               AND ({})
               AND kcm.file_id = kf.id
               AND ke.chunk_rowid = kcm.chunk_rowid{clause}",
            alternatives.join(" OR ")
        ));
    }

    let metadata_start = values.len() + 1;
    let metadata_placeholders = aliases
        .iter()
        .enumerate()
        .map(|(index, _)| format!("?{}", metadata_start + index))
        .collect::<Vec<_>>();
    let (where_sql, params) = filter::predicates_to_sql(&residual, values.len() + aliases.len());
    let clause = where_sql
        .is_empty()
        .then(String::new)
        .unwrap_or_else(|| format!(" AND {where_sql}"));
    values.extend(aliases);
    values.extend(params);
    branches.push(format!(
        "SELECT ke.chunk_rowid
         FROM knowledge_chunk_meta kcm INDEXED BY idx_chunk_meta_language
         CROSS JOIN knowledge_files kf
         CROSS JOIN knowledge_embeddings ke INDEXED BY idx_embeddings_model
         WHERE ke.model_id = ?1
           AND CASE WHEN json_valid(kcm.metadata)
                    THEN lower(json_extract(kcm.metadata, '$.frontmatter.language')) END
               IN ({})
           AND kf.id = kcm.file_id
           AND ke.chunk_rowid = kcm.chunk_rowid{clause}",
        metadata_placeholders.join(", ")
    ));

    let sql = branches.join(" UNION ");
    let params = values
        .iter()
        .map(|value| value as &dyn rusqlite::types::ToSql)
        .collect::<Vec<_>>();
    let mut statement = conn.prepare(&sql)?;
    let rowids = statement
        .query_map(params.as_slice(), |row| row.get::<_, i64>(0))?
        .collect::<rusqlite::Result<HashSet<_>>>()?;
    Ok(Some(rowids))
}
