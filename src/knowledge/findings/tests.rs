use crate::knowledge::sync::sync_source;
use crate::knowledge::KnowledgeStore;
use rusqlite::params;

mod alias;
mod cargo;
mod import;
mod method;
mod program;
mod reach;
mod report;
mod roots;
mod seal;
mod status;
mod targets;
mod visibility;

fn sync(store: &KnowledgeStore, label: &str) {
    let source = store.get_source(label).unwrap().unwrap();
    sync_source(store, &source, None).unwrap();
}

fn analysis(store: &KnowledgeStore, label: &str) -> (String, i64, String, String) {
    store
        .conn()
        .query_row(
            "SELECT a.status, a.evidence_generation, a.fingerprint, a.gaps
             FROM knowledge_analysis a
             JOIN knowledge_sources s ON s.id = a.source_id
             WHERE s.label = ?1",
            params![label],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap()
}

fn finding_rows(store: &KnowledgeStore) -> Vec<(i64, String, String, Option<String>, String)> {
    let mut statement = store
        .conn()
        .prepare(
            "SELECT f.id, f.kind, kf.rel_path, ks.name, f.reason
             FROM knowledge_findings f
             JOIN knowledge_files kf ON kf.id = f.file_id
             LEFT JOIN knowledge_symbols ks ON ks.id = f.symbol_id
             ORDER BY f.id",
        )
        .unwrap();
    statement
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

fn resolutions(
    store: &KnowledgeStore,
    specifier: &str,
) -> Vec<(String, Option<String>, Option<String>)> {
    let mut statement = store
        .conn()
        .prepare(
            "SELECT res.status, ks.name, target.rel_path
             FROM knowledge_resolutions res
             JOIN knowledge_references kr ON kr.id = res.reference_id
             LEFT JOIN knowledge_symbols ks ON ks.id = res.target_symbol_id
             LEFT JOIN knowledge_files target ON target.id = ks.file_id
             WHERE kr.specifier = ?1
             ORDER BY res.status, target.rel_path, ks.name",
        )
        .unwrap();
    statement
        .query_map(params![specifier], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

fn unused(
    findings: &[(i64, String, String, Option<String>, String)],
    path: &str,
    name: &str,
) -> bool {
    findings.iter().any(|(_, kind, found, symbol, reason)| {
        kind == "possibly_unused_symbol"
            && found == path
            && symbol.as_deref() == Some(name)
            && reason == "no reference in sealed scope"
    })
}

fn file_reason<'a>(
    findings: &'a [(i64, String, String, Option<String>, String)],
    path: &str,
) -> Option<&'a str> {
    findings.iter().find_map(|(_, kind, found, _, reason)| {
        (kind == "unreachable_file" && found == path).then_some(reason.as_str())
    })
}

fn entries(store: &KnowledgeStore) -> Vec<(String, String, String)> {
    let mut statement = store
        .conn()
        .prepare(
            "SELECT kind, reason, COALESCE(target_rel_path, '')
             FROM knowledge_entries
             ORDER BY target_rel_path, kind, reason",
        )
        .unwrap();
    statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

fn has_entry(entries: &[(String, String, String)], kind: &str, reason: &str, path: &str) -> bool {
    entries
        .iter()
        .any(|(found_kind, found_reason, found_path)| {
            found_kind == kind && found_reason == reason && found_path == path
        })
}
