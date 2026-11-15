use super::*;
use rusqlite::Connection;
use std::cell::RefCell;
use std::sync::Arc;

mod correct;
pub(super) use correct::{fuzzy_correct, knowledge_fuzzy_search};

pub(super) const MAX_EDIT_DISTANCE: usize = 2;

const LETTER_SLOTS: usize = 27;
const OTHER_SLOT: usize = 26;

type LetterCounts = [u8; LETTER_SLOTS];

struct VocabTerm {
    text: String,
    letters: LetterCounts,
}

pub(super) struct FuzzyVocabulary {
    by_length: Vec<Vec<VocabTerm>>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Corrections {
    pub(super) query: Option<String>,
    pub(super) levenshtein_calls: usize,
}

#[derive(Default)]
pub(crate) struct VocabularyCache(RefCell<Option<CachedVocabulary>>);

struct CachedVocabulary {
    fingerprint: (i64, u64),
    vocabulary: Arc<FuzzyVocabulary>,
}

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

fn within_letter_budget(query: &LetterCounts, term: &LetterCounts) -> bool {
    let difference: usize = query
        .iter()
        .zip(term)
        .map(|(&a, &b)| usize::from(a.abs_diff(b)))
        .sum();
    difference <= 2 * MAX_EDIT_DISTANCE
}
