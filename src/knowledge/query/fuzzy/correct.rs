use std::sync::Arc;

use rusqlite::Connection;

use super::super::lexical::knowledge_bm25_search;
use super::super::rank::RankedResult;
use super::super::*;
use super::{
    extract_knowledge_terms, letter_counts, within_letter_budget, CachedVocabulary, Corrections,
    FuzzyVocabulary, VocabTerm, MAX_EDIT_DISTANCE,
};

pub(in super::super) fn knowledge_fuzzy_search(
    store: &KnowledgeStore,
    query: &str,
    limit: u32,
    meta_where: &str,
    meta_params: &[String],
) -> Result<Option<Vec<RankedResult>>> {
    match fuzzy_correct(store, query)?.query {
        Some(corrected) => {
            knowledge_bm25_search(store.conn(), &corrected, limit, meta_where, meta_params)
                .map(Some)
        }
        None => Ok(None),
    }
}

pub(in super::super) fn fuzzy_correct(store: &KnowledgeStore, query: &str) -> Result<Corrections> {
    Ok(vocabulary(store)?.correct(query))
}

fn vocabulary(store: &KnowledgeStore) -> Result<Arc<FuzzyVocabulary>> {
    let fingerprint = change_fingerprint(store.conn())?;
    if let Some(cached) = store.fuzzy_cache.0.borrow().as_ref() {
        if cached.fingerprint == fingerprint {
            return Ok(Arc::clone(&cached.vocabulary));
        }
    }
    let vocabulary = Arc::new(FuzzyVocabulary::load(store.conn())?);
    *store.fuzzy_cache.0.borrow_mut() = Some(CachedVocabulary {
        fingerprint,
        vocabulary: Arc::clone(&vocabulary),
    });
    Ok(vocabulary)
}

fn change_fingerprint(conn: &Connection) -> Result<(i64, u64)> {
    let data_version: i64 = conn.query_row("PRAGMA data_version", [], |row| row.get(0))?;
    Ok((data_version, conn.total_changes()))
}

impl FuzzyVocabulary {
    fn load(conn: &Connection) -> Result<Self> {
        let mut by_length: Vec<Vec<VocabTerm>> = Vec::new();
        for term in extract_knowledge_terms(conn)? {
            let length = term.chars().count();
            if by_length.len() <= length {
                by_length.resize_with(length + 1, Vec::new);
            }
            let letters = letter_counts(&term);
            by_length[length].push(VocabTerm {
                text: term,
                letters,
            });
        }
        Ok(Self { by_length })
    }

    fn correct(&self, query: &str) -> Corrections {
        let mut levenshtein_calls = 0;
        let mut changed = false;
        let mut words = Vec::new();
        for word in query.split_whitespace() {
            let lowered = word.to_lowercase();
            if self.contains(&lowered) {
                words.push(word.to_string());
                continue;
            }
            match self.nearest(&lowered, &mut levenshtein_calls) {
                Some(term) => {
                    changed = true;
                    words.push(term.to_string());
                }
                None => words.push(word.to_string()),
            }
        }
        Corrections {
            query: changed.then(|| words.join(" ")),
            levenshtein_calls,
        }
    }

    fn contains(&self, word: &str) -> bool {
        self.bucket(word.chars().count()).is_some_and(|bucket| {
            bucket
                .binary_search_by(|term| term.text.as_str().cmp(word))
                .is_ok()
        })
    }

    fn nearest(&self, word: &str, levenshtein_calls: &mut usize) -> Option<&str> {
        let length = word.chars().count();
        let letters = letter_counts(word);
        let shortest = length.saturating_sub(MAX_EDIT_DISTANCE);
        let longest = length + MAX_EDIT_DISTANCE;
        let mut best: Option<(usize, &str)> = None;
        for bucket_length in shortest..=longest {
            let Some(bucket) = self.bucket(bucket_length) else {
                continue;
            };
            for term in bucket {
                if !within_letter_budget(&letters, &term.letters) {
                    continue;
                }
                *levenshtein_calls += 1;
                let distance = strsim::levenshtein(word, &term.text);
                let candidate = (distance, term.text.as_str());
                if distance <= MAX_EDIT_DISTANCE && best.is_none_or(|current| candidate < current) {
                    best = Some(candidate);
                }
            }
        }
        best.map(|(_, term)| term)
    }

    fn bucket(&self, length: usize) -> Option<&[VocabTerm]> {
        self.by_length.get(length).map(Vec::as_slice)
    }
}
