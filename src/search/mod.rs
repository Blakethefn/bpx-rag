pub mod profile;
pub mod truncate;

mod format;
mod query;
mod rerank;

pub(crate) use rerank::attach_relation_neighbors;
pub use query::run_query;

use crate::search::profile::SearchProfile;

#[derive(Debug, Clone)]
pub struct QueryRequest<'a> {
    pub query: &'a str,
    pub source: Option<&'a str>,
    pub knowledge_filter: Option<&'a str>,
    pub limit: u32,
    pub max_per_file: u32,
    pub snippet_bytes: usize,
    pub profile: SearchProfile,
    pub explain: bool,
    pub vector_unavailable: Option<&'a str>,
}

#[cfg(test)]
mod tests;
