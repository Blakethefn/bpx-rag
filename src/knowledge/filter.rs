use anyhow::{anyhow, Result};

mod path;
mod scope;
mod tokens;

pub(crate) use path::file_extension;
pub(crate) use scope::file_scope_sql;

#[derive(Debug, Clone, PartialEq)]
pub enum FilterPredicate {
    Source(String),
    Directory(String),
    File(String),
    Frontmatter { key: String, value: String },
    Tag(String),
    Folder(String),
    Language(String),
}

fn validate_key(key: &str) -> bool {
    !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
}

pub fn parse_filter(filter: Option<&str>) -> Result<Vec<FilterPredicate>> {
    let filter = match filter {
        Some(f) if !f.trim().is_empty() => f.trim(),
        _ => return Ok(vec![]),
    };

    let mut predicates = Vec::new();

    for token in tokens::split(filter)? {
        let (key, value) = token
            .split_once(':')
            .ok_or_else(|| anyhow!("invalid filter token '{token}'; expected key:value"))?;
        if key.is_empty() || value.is_empty() {
            return Err(anyhow!("filter keys and values must not be empty"));
        }
        let pred = match key {
            "source" => FilterPredicate::Source(value.to_string()),
            "dir" | "directory" => {
                if !path::validate_relative_path(value) {
                    return Err(anyhow!(
                        "directory filter must be a normalized relative path"
                    ));
                }
                FilterPredicate::Directory(value.to_string())
            }
            "file" => {
                if !path::validate_relative_path(value) {
                    return Err(anyhow!("file filter must be a normalized relative path"));
                }
                FilterPredicate::File(value.to_string())
            }
            "tag" => FilterPredicate::Tag(value.to_string()),
            "folder" => FilterPredicate::Folder(value.to_string()),
            "language" => FilterPredicate::Language(value.to_string()),
            other => {
                if !validate_key(other) {
                    return Err(anyhow!("invalid filter key '{other}'"));
                }
                FilterPredicate::Frontmatter {
                    key: other.to_string(),
                    value: value.to_string(),
                }
            }
        };
        predicates.push(pred);
    }

    Ok(predicates)
}

pub fn scope_predicates(
    filter: Option<&str>,
    source_label: Option<&str>,
) -> Result<Vec<FilterPredicate>> {
    let mut predicates = parse_filter(filter)?;
    if let Some(label) = source_label {
        predicates.push(FilterPredicate::Source(label.to_string()));
    }
    Ok(predicates)
}

pub fn single_source_predicate(predicates: &[FilterPredicate]) -> Option<&str> {
    let mut source = None;
    for predicate in predicates {
        let FilterPredicate::Source(label) = predicate else {
            continue;
        };
        if source.is_some_and(|current| current != label) {
            return None;
        }
        source = Some(label.as_str());
    }
    source
}

pub fn predicates_to_sql(
    predicates: &[FilterPredicate],
    param_offset: usize,
) -> (String, Vec<String>) {
    let mut fragments = Vec::new();
    let mut params: Vec<String> = Vec::new();

    for pred in predicates {
        let idx = param_offset + params.len() + 1;
        match pred {
            FilterPredicate::Source(label) => {
                fragments.push(format!(
                    "kcm.source_id = (SELECT id FROM knowledge_sources WHERE label = ?{idx})"
                ));
                params.push(label.clone());
            }
            FilterPredicate::Directory(path) => {
                let (path_sql, path_params) = path::predicate_sql(path, true, "kf.rel_path", idx);
                fragments.push(format!(
                    "kcm.file_id IN (SELECT kf.id FROM knowledge_files kf WHERE {path_sql})"
                ));
                params.extend(path_params);
            }
            FilterPredicate::File(path) => {
                let (path_sql, path_params) = path::predicate_sql(path, false, "kf.rel_path", idx);
                fragments.push(format!(
                    "kcm.file_id IN (SELECT kf.id FROM knowledge_files kf WHERE {path_sql})"
                ));
                params.extend(path_params);
            }
            FilterPredicate::Frontmatter { key, value } => {
                let json_path = format!("$.frontmatter.{key}");
                fragments.push(format!(
                    "json_extract(kcm.metadata, '{json_path}') = ?{idx}"
                ));
                params.push(value.clone());
            }
            FilterPredicate::Tag(tag) => {
                fragments.push(format!(
                    "EXISTS (SELECT 1 FROM json_each(json_extract(kcm.metadata, '$.frontmatter.tags')) WHERE value = ?{idx})"
                ));
                params.push(tag.clone());
            }
            FilterPredicate::Folder(folder) => {
                fragments.push(format!(
                    "EXISTS (SELECT 1 FROM json_each(json_extract(kcm.metadata, '$.folder_tags')) WHERE value = ?{idx})"
                ));
                params.push(folder.clone());
            }
            FilterPredicate::Language(language) => {
                let mut alternatives = Vec::new();
                for extension in crate::language::extensions_for_name(language) {
                    let idx = param_offset + params.len() + 1;
                    alternatives.push(format!("lower(kf.rel_path) LIKE ?{idx}"));
                    params.push(format!("%.{}", extension));
                }
                for file_name in crate::language::file_names_for_name(language) {
                    let exact_idx = param_offset + params.len() + 1;
                    alternatives.push(format!("lower(kf.rel_path) = ?{exact_idx}"));
                    params.push(file_name.to_string());

                    let suffix_idx = param_offset + params.len() + 1;
                    alternatives.push(format!("lower(kf.rel_path) LIKE ?{suffix_idx}"));
                    params.push(format!("%/{file_name}"));
                }
                if let Some(aliases) = crate::language::aliases_for_name(language) {
                    let placeholders = aliases
                        .iter()
                        .enumerate()
                        .map(|(offset, _)| format!("?{}", param_offset + params.len() + offset + 1))
                        .collect::<Vec<_>>();
                    alternatives.push(format!(
                        "lower(json_extract(kcm.metadata, '$.frontmatter.language')) IN ({})",
                        placeholders.join(", ")
                    ));
                    params.extend(aliases.iter().map(|alias| alias.to_ascii_lowercase()));
                } else {
                    let idx = param_offset + params.len() + 1;
                    alternatives.push(format!(
                        "lower(json_extract(kcm.metadata, '$.frontmatter.language')) = ?{idx}"
                    ));
                    params.push(language.to_ascii_lowercase());
                }
                fragments.push(format!("({})", alternatives.join(" OR ")));
            }
        }
    }

    let where_clause = fragments.join(" AND ");
    (where_clause, params)
}

#[cfg(test)]
mod tests;
