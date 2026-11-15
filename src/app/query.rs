use anyhow::{Result, anyhow};

use crate::knowledge::filter::{scope_predicates, single_source_predicate};
use crate::search::profile::SearchProfile;
use crate::search::{self, QueryRequest};

use super::embed::add_embedding_notice;
use super::{RagApp, SearchOptions};

impl RagApp {
    pub fn search(&mut self, query: &str, options: SearchOptions<'_>) -> Result<String> {
        let query = validate_query(query)?;
        let profile = SearchProfile::parse(options.profile)?;
        let predicates = scope_predicates(options.filter, options.source)?;
        let source_scope = single_source_predicate(&predicates);
        self.ensure_embedder();
        let coverage = self.embedding_coverage(&predicates)?;
        if let Some((model_id, dimensions)) = self
            .knowledge
            .embedder()
            .map(|embedder| (embedder.model_id().to_string(), embedder.dim()))
        {
            let embedded_count = coverage.map_or(0, |coverage| coverage.current_model);
            if let Err(error) = self.knowledge.ensure_vector_index_for_scope(
                &model_id,
                dimensions,
                source_scope,
                embedded_count,
            ) {
                self.knowledge.clear_vector_index();
                self.embedding_notice = Some(format!(
                    "Approximate index unavailable; using exact vector search: {error}"
                ));
                eprintln!("[bpx-rag] failed to prepare approximate index: {error}");
            }
        }
        if let Some(notice) = coverage.and_then(|coverage| coverage.notice(!predicates.is_empty()))
        {
            self.embedding_notice = Some(notice);
        }
        let outcome = search::run_query(
            QueryRequest {
                query,
                source: options.source,
                knowledge_filter: options.filter,
                limit: options.limit.clamp(1, 50),
                max_per_file: options.max_per_file,
                snippet_bytes: self.config.search.snippet_bytes,
                profile,
                explain: options.explain,
                vector_unavailable: if self.knowledge.embedder().is_none() {
                    self.embedding_notice.as_deref()
                } else {
                    None
                },
            },
            &self.config,
            &self.knowledge,
            options.count_only,
        )?;
        Ok(add_embedding_notice(
            outcome,
            self.embedding_notice.as_deref(),
            options.count_only,
        ))
    }
}

fn validate_query(query: &str) -> Result<&str> {
    let query = query.trim();
    if query.is_empty() {
        return Err(anyhow!("search query cannot be empty"));
    }
    if query.len() > 16_384 {
        return Err(anyhow!("search query exceeds the 16 KiB limit"));
    }
    Ok(query)
}
