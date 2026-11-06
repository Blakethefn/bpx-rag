//! Fuzzy correction of query words against the indexed content vocabulary.
//!
//! A query word that is not already an indexed term is replaced by the
//! closest vocabulary term within `MAX_EDIT_DISTANCE`. The whole vocabulary is
//! searched, not a prefix of it.
//!
//! The vocabulary is kept in memory, grouped by character count. It is read
//! again only when the database has changed. Candidates are pruned by length,
//! then by a letter-count bound, before any Levenshtein computation.

use super::*;
use rusqlite::Connection;
use std::cell::RefCell;
use std::sync::Arc;

/// Largest edit distance at which a vocabulary term replaces a query word.
pub(super) const MAX_EDIT_DISTANCE: usize = 2;

/// One slot per letter `a` to `z`, plus one slot shared by every other character.
const LETTER_SLOTS: usize = 27;
const OTHER_SLOT: usize = 26;

type LetterCounts = [u8; LETTER_SLOTS];

struct VocabTerm {
    text: String,
    letters: LetterCounts,
}

/// The content vocabulary, grouped by character count. Each group is sorted.
pub(super) struct FuzzyVocabulary {
    by_length: Vec<Vec<VocabTerm>>,
}

/// Result of correcting one query.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Corrections {
    /// The corrected query, or `None` when no word changed.
    pub(super) query: Option<String>,
    /// Levenshtein computations made for this query.
    pub(super) levenshtein_calls: usize,
}

/// The vocabulary last loaded for a store, with the database state it was read from.
#[derive(Default)]
pub(crate) struct VocabularyCache(RefCell<Option<CachedVocabulary>>);

struct CachedVocabulary {
    fingerprint: (i64, u64),
    vocabulary: Arc<FuzzyVocabulary>,
}

/// Fuzzy layer: correct the query, then search the corrected query with BM25.
/// Returns `None` when no word was corrected, so the layer did not run.
pub(super) fn knowledge_fuzzy_search(
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

/// Correct `query` against the store's vocabulary without searching.
pub(super) fn fuzzy_correct(store: &KnowledgeStore, query: &str) -> Result<Corrections> {
    Ok(vocabulary(store)?.correct(query))
}

/// The store's vocabulary. The fingerprint is read before the vocabulary, so a
/// write that lands in between leaves a stale fingerprint. The next call then
/// reloads.
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

/// Changes by other connections show up in `data_version`. Changes made by this
/// connection show up in `total_changes`.
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

    /// The closest term within `MAX_EDIT_DISTANCE`. Ties go to the smaller term,
    /// so the result does not depend on the order of the buckets.
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

/// Distinct content terms, in sorted order. The vocabulary table is created when the schema is
/// applied, so this read path never writes. A query error is returned, not treated as empty.
pub(super) fn extract_knowledge_terms(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT term FROM knowledge_chunks_vocab WHERE col = 'content' ORDER BY term",
    )?;
    let terms = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(terms)
}

fn letter_counts(text: &str) -> LetterCounts {
    let mut counts = [0u8; LETTER_SLOTS];
    for character in text.chars() {
        let slot = match character {
            'a'..='z' => usize::from(character as u8 - b'a'),
            _ => OTHER_SLOT,
        };
        counts[slot] = counts[slot].saturating_add(1);
    }
    counts
}

/// Each edit changes at most two letter counts, and each by one. A term more
/// than `MAX_EDIT_DISTANCE` edits away therefore differs by more than twice that
/// distance in total. Saturated counts only shrink the difference, so the bound
/// never rejects a term that is within the limit.
fn within_letter_budget(query: &LetterCounts, term: &LetterCounts) -> bool {
    let difference: usize = query
        .iter()
        .zip(term)
        .map(|(&a, &b)| usize::from(a.abs_diff(b)))
        .sum();
    difference <= 2 * MAX_EDIT_DISTANCE
}
