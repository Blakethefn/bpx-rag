use super::{FilterPredicate, path};

pub(crate) fn file_scope_sql(
    predicates: &[FilterPredicate],
    param_offset: usize,
) -> Option<(String, Vec<String>)> {
    if predicates.iter().any(|predicate| {
        !matches!(
            predicate,
            FilterPredicate::Source(_) | FilterPredicate::Directory(_) | FilterPredicate::File(_)
        )
    }) {
        return None;
    }

    let mut fragments = Vec::new();
    let mut params = Vec::new();
    for predicate in predicates {
        let index = param_offset + params.len() + 1;
        let (fragment, values) = match predicate {
            FilterPredicate::Source(label) => (
                format!("kf.source_id = (SELECT id FROM knowledge_sources WHERE label = ?{index})"),
                vec![label.clone()],
            ),
            FilterPredicate::Directory(path_value) => {
                path::predicate_sql(path_value, true, "kf.rel_path", index)
            }
            FilterPredicate::File(path_value) => {
                path::predicate_sql(path_value, false, "kf.rel_path", index)
            }
            _ => return None,
        };
        fragments.push(fragment);
        params.extend(values);
    }
    Some((fragments.join(" AND "), params))
}
