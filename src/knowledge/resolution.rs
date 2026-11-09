//! One resolution per stored reference.
//!
//! Resolutions are derived from `knowledge_symbols` and `knowledge_references`. This pass does
//! not read source files. Symbol ids are deleted and reinserted by evidence replacement, so the
//! caller rewrites resolutions after that replacement and before commit.

use anyhow::Result;
use rusqlite::{params, Connection};
use std::collections::HashMap;

use crate::chunk::evidence::{KIND_MODULE, KIND_PATH};

struct StoredFile {
    id: i64,
    rel_path: String,
}

struct StoredSymbol {
    id: i64,
    name: String,
    container: Option<String>,
    rel_path: String,
}

struct StoredReference {
    id: i64,
    file_id: i64,
    kind: String,
    specifier: String,
    qualifier: Option<String>,
    from_rel: String,
}

pub(crate) fn rebuild(conn: &Connection, source_id: i64) -> Result<()> {
    conn.execute(
        "DELETE FROM knowledge_resolutions WHERE source_id = ?1",
        params![source_id],
    )?;

    let files = load_files(conn, source_id)?;
    let symbols = load_symbols(conn, source_id)?;
    let references = load_references(conn, source_id)?;
    let files_by_path: HashMap<&str, i64> = files
        .iter()
        .map(|file| (file.rel_path.as_str(), file.id))
        .collect();

    let mut insert = conn.prepare_cached(
        "INSERT INTO knowledge_resolutions
            (source_id, reference_id, status, target_symbol_id, target_file_id)
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;

    for reference in &references {
        if reference.kind == KIND_MODULE || reference.kind == KIND_PATH {
            match module_file(
                &reference.from_rel,
                &reference.specifier,
                &files_by_path,
                reference.file_id,
            ) {
                Some(file_id) => {
                    insert.execute(params![
                        source_id,
                        reference.id,
                        "resolved",
                        None::<i64>,
                        file_id
                    ])?;
                }
                None => {
                    insert.execute(params![
                        source_id,
                        reference.id,
                        "unresolved",
                        None::<i64>,
                        None::<i64>
                    ])?;
                }
            }
            continue;
        }

        let candidates = symbol_candidates(reference, &symbols);
        match candidates.as_slice() {
            [] => {
                insert.execute(params![
                    source_id,
                    reference.id,
                    "unresolved",
                    None::<i64>,
                    None::<i64>
                ])?;
            }
            [symbol_id] => {
                insert.execute(params![
                    source_id,
                    reference.id,
                    "resolved",
                    symbol_id,
                    None::<i64>
                ])?;
            }
            many => {
                for symbol_id in many {
                    insert.execute(params![
                        source_id,
                        reference.id,
                        "ambiguous",
                        symbol_id,
                        None::<i64>
                    ])?;
                }
            }
        }
    }
    Ok(())
}

fn load_files(conn: &Connection, source_id: i64) -> Result<Vec<StoredFile>> {
    let mut statement =
        conn.prepare("SELECT id, rel_path FROM knowledge_files WHERE source_id = ?1")?;
    let rows = statement.query_map(params![source_id], |row| {
        Ok(StoredFile {
            id: row.get(0)?,
            rel_path: row.get(1)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

fn load_symbols(conn: &Connection, source_id: i64) -> Result<Vec<StoredSymbol>> {
    let mut statement = conn.prepare(
        "SELECT ks.id, ks.name, ks.container, kf.rel_path
         FROM knowledge_symbols ks
         JOIN knowledge_files kf ON kf.id = ks.file_id
         WHERE ks.source_id = ?1",
    )?;
    let rows = statement.query_map(params![source_id], |row| {
        Ok(StoredSymbol {
            id: row.get(0)?,
            name: row.get(1)?,
            container: row.get(2)?,
            rel_path: row.get(3)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

fn load_references(conn: &Connection, source_id: i64) -> Result<Vec<StoredReference>> {
    let mut statement = conn.prepare(
        "SELECT kr.id, kr.file_id, kr.kind, kr.specifier, kr.qualifier, kf.rel_path
         FROM knowledge_references kr
         JOIN knowledge_files kf ON kf.id = kr.file_id
         WHERE kr.source_id = ?1
         ORDER BY kr.id",
    )?;
    let rows = statement.query_map(params![source_id], |row| {
        Ok(StoredReference {
            id: row.get(0)?,
            file_id: row.get(1)?,
            kind: row.get(2)?,
            specifier: row.get(3)?,
            qualifier: row.get(4)?,
            from_rel: row.get(5)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

fn symbol_candidates(reference: &StoredReference, symbols: &[StoredSymbol]) -> Vec<i64> {
    let named: Vec<&StoredSymbol> = symbols
        .iter()
        .filter(|symbol| symbol.name == reference.specifier)
        .collect();
    let Some(qualifier) = reference.qualifier.as_deref() else {
        return named.iter().map(|symbol| symbol.id).collect();
    };
    if is_receiver(qualifier) {
        return named.iter().map(|symbol| symbol.id).collect();
    }
    let last = qualifier
        .rsplit([':', '.', '/'])
        .find(|part| !part.is_empty())
        .unwrap_or(qualifier);
    named
        .into_iter()
        .filter(|symbol| {
            symbol.container.as_deref() == Some(qualifier)
                || symbol.container.as_deref() == Some(last)
                || file_stem(&symbol.rel_path) == last
                || parent_dir(&symbol.rel_path).ends_with(last)
        })
        .map(|symbol| symbol.id)
        .collect()
}

fn is_receiver(qualifier: &str) -> bool {
    matches!(qualifier, "self" | "Self" | "this" | "cls" | "super")
}

/// The first indexed file named by a module or path specifier, skipping the file that
/// contains the reference.
fn module_file(
    from_rel: &str,
    specifier: &str,
    files: &HashMap<&str, i64>,
    from_file_id: i64,
) -> Option<i64> {
    for candidate in module_candidates(from_rel, specifier) {
        if let Some(&file_id) = files.get(candidate.as_str()) {
            if file_id != from_file_id {
                return Some(file_id);
            }
        }
    }
    None
}

fn module_candidates(from_rel: &str, specifier: &str) -> Vec<String> {
    let from_rel = from_rel.replace('\\', "/");
    let specifier = specifier.trim().replace('\\', "/");
    if specifier.is_empty() {
        return Vec::new();
    }
    let ext = extension(&from_rel);
    if matches!(ext, "py" | "pyi") {
        return python_candidates(&from_rel, &specifier);
    }
    if ext == "rs" {
        return rust_candidates(&from_rel, &specifier);
    }
    if ext == "go" {
        return go_candidates(&specifier);
    }
    if specifier.starts_with('.') {
        return relative_candidates(&from_rel, &specifier);
    }
    let mut candidates = Vec::new();
    let parent = parent_dir(&from_rel);
    for candidate_ext in ["rs", "py", "ts", "tsx", "js", "jsx", "go"] {
        candidates.push(join_rel(&parent, &format!("{specifier}.{candidate_ext}")));
    }
    candidates
}

fn rust_candidates(from_rel: &str, specifier: &str) -> Vec<String> {
    let name = specifier
        .rsplit("::")
        .find(|part| !part.is_empty())
        .unwrap_or(specifier);
    if matches!(name, "crate" | "self" | "super") {
        return Vec::new();
    }
    let parent = parent_dir(from_rel);
    let stem = file_stem(from_rel);
    let mut bases = Vec::new();
    if stem != "lib" && stem != "main" && stem != "mod" {
        bases.push(join_rel(&parent, stem));
    }
    bases.push(parent);
    let mut candidates = Vec::new();
    for base in bases {
        candidates.push(join_rel(&base, &format!("{name}.rs")));
        candidates.push(join_rel(&base, &format!("{name}/mod.rs")));
    }
    candidates
}

fn python_candidates(from_rel: &str, specifier: &str) -> Vec<String> {
    let mut level = 0usize;
    let rest = specifier.trim_start_matches(|character| {
        if character == '.' {
            level += 1;
            true
        } else {
            false
        }
    });
    let mut base = parent_dir(from_rel);
    if level > 1 {
        for _ in 1..level {
            base = parent_dir(&base);
        }
    }
    let dotted = rest.trim_matches('.').replace('.', "/");
    if dotted.is_empty() {
        return Vec::new();
    }
    if level == 0 {
        let parent = parent_dir(from_rel);
        vec![
            join_rel(&parent, &format!("{dotted}.py")),
            join_rel(&parent, &format!("{dotted}/__init__.py")),
            format!("{dotted}.py"),
            format!("{dotted}/__init__.py"),
        ]
    } else {
        vec![
            join_rel(&base, &format!("{dotted}.py")),
            join_rel(&base, &format!("{dotted}/__init__.py")),
        ]
    }
}

fn go_candidates(specifier: &str) -> Vec<String> {
    let name = specifier
        .rsplit('/')
        .find(|part| !part.is_empty())
        .unwrap_or(specifier);
    vec![format!("{name}/{name}.go"), format!("{name}.go")]
}

fn relative_candidates(from_rel: &str, specifier: &str) -> Vec<String> {
    let joined = join_rel(&parent_dir(from_rel), specifier);
    let mut candidates = vec![joined.clone()];
    for ext in ["ts", "tsx", "js", "jsx", "mjs", "cjs", "py", "rs"] {
        if !joined.ends_with(&format!(".{ext}")) {
            candidates.push(format!("{joined}.{ext}"));
        }
    }
    for ext in ["ts", "tsx", "js", "jsx"] {
        candidates.push(format!("{joined}/index.{ext}"));
    }
    candidates
}

fn join_rel(parent: &str, child: &str) -> String {
    let combined = if parent.is_empty() {
        child.to_string()
    } else {
        format!("{parent}/{child}")
    };
    normalize_rel(&combined)
}

fn normalize_rel(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    let mut parts = Vec::new();
    for part in normalized.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

fn parent_dir(path: &str) -> String {
    match path.rfind('/') {
        Some(index) => path[..index].to_string(),
        None => String::new(),
    }
}

fn file_stem(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.').map(|(stem, _)| stem).unwrap_or(name)
}

fn extension(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.').map(|(_, ext)| ext).unwrap_or("")
}
