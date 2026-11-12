use std::path::Path;

use anyhow::{anyhow, Context, Result};

use crate::knowledge::{self, KnowledgeSourceInfo};

use super::RagApp;

impl RagApp {
    pub fn add_source(
        &mut self,
        path: &Path,
        label: &str,
        include: &[String],
        exclude: &[String],
        enrichments: &[String],
    ) -> Result<String> {
        validate_label(label)?;
        knowledge::enrichment::validate_enrichments(enrichments)?;
        let _writer = self.writer_lock()?;
        let canonical_path = path
            .canonicalize()
            .with_context(|| format!("could not resolve source path {}", path.display()))?;
        if !canonical_path.is_dir() {
            return Err(anyhow!("source path must be a directory"));
        }
        let patterns = knowledge::scope::patterns_from(include, exclude);
        knowledge::scope::validate(&canonical_path, &patterns)?;
        if self.knowledge.get_source(label)?.is_some() {
            return Err(anyhow!("a knowledge source named '{label}' already exists"));
        }
        self.ensure_embedder();
        self.knowledge.add_source(
            label,
            &canonical_path.to_string_lossy(),
            &patterns,
            enrichments,
        )?;
        match self.register_and_sync(label) {
            Ok(result) => Ok(format_sync_result(&result)),
            Err(error) => {
                if let Err(cleanup) = self.knowledge.remove_source(label) {
                    eprintln!(
                        "[bpx-rag] could not remove source '{label}' after its first sync failed: {cleanup:#}"
                    );
                }
                Err(error)
            }
        }
    }

    fn register_and_sync(&self, label: &str) -> Result<knowledge::sync::SyncResult> {
        let source = self
            .knowledge
            .get_source(label)?
            .ok_or_else(|| anyhow!("new source could not be loaded"))?;
        self.sync_one(&source)
    }

    pub fn sync_sources(
        &mut self,
        label: Option<&str>,
    ) -> Result<Vec<knowledge::sync::SyncResult>> {
        let sources = match label {
            Some(label) => vec![self
                .knowledge
                .get_source(label)?
                .ok_or_else(|| anyhow!("knowledge source '{label}' was not found"))?],
            None => self.knowledge.list_sources()?,
        };
        if sources.is_empty() {
            return Ok(Vec::new());
        }
        let _writer = self.writer_lock()?;
        self.ensure_embedder();
        sources.iter().map(|source| self.sync_one(source)).collect()
    }

    pub fn remove_source(&self, label: &str) -> Result<knowledge::RemoveResult> {
        validate_label(label)?;
        let _writer = self.writer_lock()?;
        self.knowledge.remove_source(label)
    }

    pub fn scrub_sensitive_index(
        &self,
        source_label: Option<&str>,
    ) -> Result<knowledge::sync::SensitiveIndexScrubReport> {
        let _writer = self.writer_lock()?;
        let source_id = match source_label {
            Some(label) if label.trim().is_empty() => {
                return Err(anyhow!("source label must not be empty"));
            }
            Some(label) => Some(
                self.knowledge
                    .get_source(label)?
                    .ok_or_else(|| anyhow!("knowledge source '{label}' was not found"))?
                    .id,
            ),
            None => None,
        };
        knowledge::sync::scrub_sensitive_indexed_files(&self.knowledge, source_id)
    }

    fn sync_one(&self, source: &KnowledgeSourceInfo) -> Result<knowledge::sync::SyncResult> {
        let base = Path::new(&source.path);
        let enrichment_fn = knowledge::enrichment::build_enrichment_fn(&source.enrichments, base);
        let enrichment_ref = enrichment_fn
            .as_ref()
            .map(|function| function.as_ref() as &dyn Fn(&str, &Path) -> serde_json::Value);
        knowledge::sync::sync_source(&self.knowledge, source, enrichment_ref)
    }
}

pub(in crate::app) fn validate_label(label: &str) -> Result<()> {
    if label.trim().is_empty() || label.len() > 256 {
        return Err(anyhow!("source label must be between 1 and 256 bytes"));
    }
    Ok(())
}

pub fn format_sync_result(result: &knowledge::sync::SyncResult) -> String {
    let summary = format!(
        "Source '{}': scanned {}, added {}, updated {}, removed {}, unchanged {}; {} sensitive files skipped by policy; {} chunks indexed this sync ({} total; {} Tree-sitter / {} existing chunker across {} / {} changed files) in {} ms.",
        result.source_label,
        result.files_scanned,
        result.files_added,
        result.files_updated,
        result.files_removed,
        result.files_unchanged,
        result.sensitive_files_skipped,
        result.chunks_indexed,
        result.chunks_total,
        result.tree_sitter_chunks_indexed,
        result.existing_chunker_chunks_indexed,
        result.tree_sitter_files_indexed,
        result.existing_chunker_files_indexed,
        result.duration_ms
    );
    result.skipped_files.iter().fold(summary, |text, skipped| {
        format!(
            "{text}\nwarning: skipped '{}' from this source: {}",
            skipped.rel_path, skipped.reason
        )
    })
}
