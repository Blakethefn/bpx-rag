use std::collections::HashSet;

use rusqlite::params;

use crate::knowledge;
use crate::knowledge::KnowledgeStore;

const WORDS_PER_CHUNK: usize = 50;
const QUERY_COUNT: usize = 200;
const MIN_WORD_LEN: usize = 4;
const MAX_WORD_LEN: usize = 12;
const SEED: u64 = 0x9E37_79B9_7F4A_7C15;

struct Xorshift(u64);

impl Xorshift {
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next_u64() % bound as u64) as usize
    }
}

pub(super) fn synthetic_vocabulary(size: usize) -> Vec<String> {
    let mut rng = Xorshift(SEED);
    let mut seen = HashSet::with_capacity(size);
    let mut words = Vec::with_capacity(size);
    while words.len() < size {
        let len = MIN_WORD_LEN + rng.below(MAX_WORD_LEN - MIN_WORD_LEN + 1);
        let word: String = (0..len)
            .map(|_| char::from(b'a' + rng.below(26) as u8))
            .collect();
        if seen.insert(word.clone()) {
            words.push(word);
        }
    }
    words
}

fn typo(word: &str, kind: usize) -> String {
    let mut chars: Vec<char> = word.chars().collect();
    match kind % 3 {
        0 => {
            chars.remove(chars.len() / 2);
        }
        1 => {
            let middle = chars.len() / 2;
            chars[middle] = if chars[middle] == 'x' { 'y' } else { 'x' };
        }
        _ => chars.push('k'),
    }
    chars.into_iter().collect()
}

pub(super) fn misspelling_queries(vocabulary: &[String]) -> (Vec<String>, Vec<String>) {
    let single = (0..QUERY_COUNT)
        .map(|k| typo(&vocabulary[k * vocabulary.len() / QUERY_COUNT], k))
        .collect();
    let multi = (0..QUERY_COUNT)
        .map(|k| {
            let typo_word = typo(&vocabulary[k * vocabulary.len() / QUERY_COUNT], k);
            let first = &vocabulary[(k * 7919 + 1) % vocabulary.len()];
            let second = &vocabulary[(k * 104_729 + 2) % vocabulary.len()];
            format!("{typo_word} {first} {second}")
        })
        .collect();
    (single, multi)
}

pub(super) fn build_store(vocabulary: &[String]) -> KnowledgeStore {
    let store = knowledge::open_in_memory().expect("in-memory knowledge store");
    let conn = store.conn();
    conn.execute_batch("BEGIN").unwrap();
    for chunk in vocabulary.chunks(WORDS_PER_CHUNK) {
        conn.execute(
            "INSERT INTO knowledge_chunks (title, content) VALUES ('', ?1)",
            params![chunk.join(" ")],
        )
        .unwrap();
    }
    conn.execute_batch("COMMIT").unwrap();
    store
}
