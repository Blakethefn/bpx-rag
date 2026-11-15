use anyhow::Result;
use std::collections::HashSet;

use super::{
    ANN_CANDIDATE_MULTIPLIER, ANN_MAX_CANDIDATES, SMALL_FILTER_EXACT_LIMIT, VectorIndex,
};

impl VectorIndex {
    pub fn candidates(
        &self,
        query: &[f32],
        limit: usize,
        allowed: Option<&HashSet<i64>>,
    ) -> Result<Option<Vec<i64>>> {
        if query.len() != self.dimensions || limit == 0 {
            return Ok(Some(Vec::new()));
        }
        let target = limit
            .saturating_mul(ANN_CANDIDATE_MULTIPLIER)
            .clamp(256, ANN_MAX_CANDIDATES)
            .min(self.index.size());
        if target == 0 {
            return Ok(Some(Vec::new()));
        }

        let matches = match allowed {
            Some(allowed) if allowed.len() <= SMALL_FILTER_EXACT_LIMIT => {
                return Ok(Some(allowed.iter().copied().collect()));
            }
            Some(allowed) => self
                .index
                .filtered_search(query, target, |key| allowed.contains(&(key as i64)))?,
            None => self.index.search(query, target)?,
        };
        let candidates = matches
            .keys
            .into_iter()
            .map(|key| key as i64)
            .collect::<Vec<_>>();
        if candidates.len() >= limit {
            return Ok(Some(candidates));
        }
        match allowed {
            Some(_) => Ok(None),
            None => Ok(Some(candidates)),
        }
    }
}
