use std::collections::BTreeSet;
use std::path::Path;
use std::time::Instant;

use anyhow::Result;

use crate::knowledge::{KnowledgeSourceInfo, KnowledgeStore};

use super::classify::{classify_file, mark_excluded, ChangePlan, Classification, ScanCounts};
use super::files::load_db_files;
use super::plan::apply_plan;
use super::walk::walk_source_dir_with_report;
use super::{EnrichmentFn, SkippedFile, SyncResult};

pub fn sync_paths(
    store: &KnowledgeStore,
    source: &KnowledgeSourceInfo,
    enrichment_fn: EnrichmentFn<'_>,
    rel_paths: &[String],
) -> Result<SyncResult> {
    let start = Instant::now();
    let conn = store.conn();
    let base_path = Path::new(&source.path);

    let walk = walk_source_dir_with_report(base_path, &source.patterns)?;
    let files_scanned = walk.files.len();
    let mut sensitive_files_skipped = walk.sensitive_files_skipped;
    let db_files = load_db_files(conn, source.id)?;

    let mut plan = ChangePlan::default();
    let unique_paths: BTreeSet<&String> = rel_paths.iter().collect();
    let mut skipped: Vec<SkippedFile> = walk
        .unreadable
        .iter()
        .filter(|file| unique_paths.contains(&file.rel_path))
        .cloned()
        .collect();
    for rel_path in unique_paths {
        match walk.files.get(rel_path) {
            Some(abs_path) => match classify_file(rel_path, abs_path, &db_files, &mut plan)? {
                Classification::Indexed => {}
                Classification::Sensitive => {
                    sensitive_files_skipped += 1;
                    mark_excluded(&db_files, rel_path, &mut plan);
                }
                Classification::Skipped(reason) => {
                    skipped.push(SkippedFile {
                        rel_path: rel_path.clone(),
                        reason,
                    });
                    mark_excluded(&db_files, rel_path, &mut plan);
                }
            },
            None => mark_excluded(&db_files, rel_path, &mut plan),
        }
    }

    let counts = ScanCounts {
        files_scanned,
        sensitive_files_skipped,
        skipped,
    };
    apply_plan(store, source, &db_files, plan, enrichment_fn, start, counts)
}
