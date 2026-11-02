//! Identifier splitting for the identifier layer.
//!
//! `getUserById`, `get_user_by_id`, and `get-user-by-id` all index as one
//! `porter unicode61` token, or as several tokens with no shared phrase. The
//! identifier layer stores the words of each compound identifier, so a
//! natural-language query such as "get user by id" can match the code.

use anyhow::Result;
use rusqlite::{params, Connection, TransactionBehavior};
use std::collections::HashSet;

/// Version of the identifier index. Bump it when `identifier_words` changes what it
/// writes. A bump backfills identifier rows from the stored chunk text. It does not
/// reindex files, so chunk rows and embeddings are left alone.
pub(crate) const IDENTIFIER_INDEX_VERSION: i64 = 1;

const VERSION_KEY: &str = "identifier_index_version";

/// Bring the identifier index up to `IDENTIFIER_INDEX_VERSION`.
///
/// Chunks that have no identifier row get one, computed from the title and content
/// already stored. The check and the backfill run in one `BEGIN IMMEDIATE` transaction,
/// so two processes that open the same database cannot write duplicate rows.
pub(crate) fn ensure_identifier_index(conn: &Connection) -> Result<()> {
    if stored_version(conn)? >= IDENTIFIER_INDEX_VERSION {
        return Ok(());
    }
    let transaction = rusqlite::Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
    if stored_version(&transaction)? < IDENTIFIER_INDEX_VERSION {
        backfill_missing_rows(&transaction)?;
        transaction.execute(
            "INSERT INTO knowledge_index_meta (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![VERSION_KEY, IDENTIFIER_INDEX_VERSION.to_string()],
        )?;
    }
    transaction.commit()?;
    Ok(())
}

fn stored_version(conn: &Connection) -> Result<i64> {
    let stored: Option<String> = match conn.query_row(
        "SELECT value FROM knowledge_index_meta WHERE key = ?1",
        params![VERSION_KEY],
        |row| row.get(0),
    ) {
        Ok(value) => Some(value),
        Err(rusqlite::Error::QueryReturnedNoRows) => None,
        Err(error) => return Err(error.into()),
    };
    match stored {
        Some(value) => Ok(value.parse()?),
        None => Ok(0),
    }
}

/// Insert identifier rows for chunks that have none. The text matches what sync writes,
/// `title`, a space, then `content`.
fn backfill_missing_rows(conn: &Connection) -> Result<()> {
    let mut statement = conn.prepare(
        "SELECT kc.rowid, COALESCE(kc.title, '') || ' ' || COALESCE(kc.content, '')
         FROM knowledge_chunks kc
         WHERE NOT EXISTS (SELECT 1 FROM knowledge_identifiers ki WHERE ki.rowid = kc.rowid)",
    )?;
    let missing = statement
        .query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(statement);
    for (rowid, text) in missing {
        conn.execute(
            "INSERT INTO knowledge_identifiers(rowid, words) VALUES (?1, ?2)",
            params![rowid, identifier_words(&text)],
        )?;
    }
    Ok(())
}

/// Split one identifier into lowercase words.
///
/// Splits on `_` and `-`, and at case boundaries: `lower|Upper`, `digit|Upper`,
/// and `ABC|Xyz` (`HTTP|Server`). Digits are never split from letters.
pub(crate) fn split_identifier(identifier: &str) -> Vec<String> {
    let chars: Vec<char> = identifier.chars().collect();
    let mut words = Vec::new();
    let mut current = String::new();
    for (index, &character) in chars.iter().enumerate() {
        if character == '_' || character == '-' {
            push_word(&mut words, &mut current);
            continue;
        }
        if index > 0 && starts_word(&chars, index) {
            push_word(&mut words, &mut current);
        }
        current.extend(character.to_lowercase());
    }
    push_word(&mut words, &mut current);
    words
}

/// Whether `chars[index]` begins a new word inside an identifier.
fn starts_word(chars: &[char], index: usize) -> bool {
    let previous = chars[index - 1];
    let current = chars[index];
    if (previous.is_lowercase() || previous.is_ascii_digit()) && current.is_uppercase() {
        return true;
    }
    // In `HTTPServer` the capital that starts `Server` follows another capital
    // and precedes a lowercase letter.
    previous.is_uppercase()
        && current.is_uppercase()
        && chars.get(index + 1).is_some_and(|next| next.is_lowercase())
}

fn push_word(words: &mut Vec<String>, current: &mut String) {
    if !current.is_empty() {
        words.push(std::mem::take(current));
    }
}

/// The split words of every compound identifier in `text`, for the identifier
/// layer. Each distinct identifier contributes its words in source order.
///
/// Word order matters: the layer matches phrases, so the words of one identifier
/// must stay adjacent. A plain word, or a number such as a date, is not an
/// identifier and contributes nothing.
pub(crate) fn identifier_words(text: &str) -> String {
    let mut seen = HashSet::new();
    let mut phrases = Vec::new();
    for token in text.split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '-')) {
        let identifier = token.trim_matches(|c| c == '_' || c == '-');
        if !identifier.starts_with(char::is_alphabetic) || !seen.insert(identifier) {
            continue;
        }
        let words = split_identifier(identifier);
        if words.len() > 1 {
            phrases.push(words.join(" "));
        }
    }
    phrases.join(" ")
}

#[cfg(test)]
mod tests;
