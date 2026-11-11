//! Identifier splitting for the identifier layer.
//!
//! `getUserById`, `get_user_by_id`, and `get-user-by-id` all index as one
//! `porter unicode61` token, or as several tokens with no shared phrase. The
//! identifier layer stores the words of each compound identifier, so a
//! natural-language query such as "get user by id" can match the code.

use anyhow::Result;
use rusqlite::{params, Connection, TransactionBehavior};

/// Version of the identifier index.
///
/// Version 2 keeps identifier rows in step with `knowledge_chunk_meta`. Triggers on that
/// ordinary table delete a chunk's identifier row when the chunk's metadata row is deleted,
/// and queue a new metadata row in `knowledge_identifier_pending`. Triggers live in the
/// database file, so writers built before this version are covered too: their inserts and
/// deletes go through the same table. Queued chunks are indexed the next time the database
/// is opened or synced by a current binary.
///
/// Version 1 rows could be orphaned or attached to reused rowids, so moving to version 2
/// rebuilds every row from the stored chunk text. Chunk rows and embeddings are not touched.
pub(crate) const IDENTIFIER_INDEX_VERSION: i64 = 2;

const VERSION_KEY: &str = "identifier_index_version";

/// Bring the identifier index up to `IDENTIFIER_INDEX_VERSION` and index queued chunks.
///
/// The check and all writes run in one `BEGIN IMMEDIATE` transaction. Two processes that
/// open the same database cannot interleave, and an interrupted rebuild rolls back, so the
/// version marker is only set once every row is written.
pub(crate) fn ensure_identifier_index(conn: &Connection) -> Result<()> {
    if !needs_maintenance(conn)? {
        return Ok(());
    }
    let transaction = rusqlite::Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
    if stored_version(&transaction)? < IDENTIFIER_INDEX_VERSION {
        rebuild_all(&transaction)?;
    } else {
        index_pending(&transaction)?;
    }
    transaction.commit()?;
    Ok(())
}

fn needs_maintenance(conn: &Connection) -> Result<bool> {
    if stored_version(conn)? < IDENTIFIER_INDEX_VERSION {
        return Ok(true);
    }
    let queued: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM knowledge_identifier_pending)",
        [],
        |row| row.get(0),
    )?;
    Ok(queued)
}

/// Index one chunk's identifier words, replacing any row already held under its rowid, and
/// clear it from the pending queue. Called by sync, inside its transaction.
pub(crate) fn write_chunk_identifiers(conn: &Connection, rowid: i64, text: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM knowledge_identifiers WHERE rowid = ?1",
        params![rowid],
    )?;
    insert_words(conn, rowid, text)?;
    conn.execute(
        "DELETE FROM knowledge_identifier_pending WHERE chunk_rowid = ?1",
        params![rowid],
    )?;
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

/// Discard every identifier row and rebuild one per chunk row from the stored text. The text
/// matches what sync writes: `title`, a space, then `content`. The identifier layer joins on
/// metadata, so a chunk without a metadata row is never returned whatever its words are.
fn rebuild_all(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "DELETE FROM knowledge_identifiers;
         DELETE FROM knowledge_identifier_pending;",
    )?;
    let mut statement = conn.prepare(
        "SELECT rowid, COALESCE(title, '') || ' ' || COALESCE(content, '')
         FROM knowledge_chunks",
    )?;
    let mut rows = statement.query([])?;
    let mut written = 0usize;
    while let Some(row) = rows.next()? {
        let rowid: i64 = row.get(0)?;
        let text: String = row.get(1)?;
        insert_words(conn, rowid, &text)?;
        written += 1;
        failpoint(written)?;
    }
    drop(rows);
    drop(statement);
    conn.execute(
        "INSERT INTO knowledge_index_meta (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![VERSION_KEY, IDENTIFIER_INDEX_VERSION.to_string()],
    )?;
    Ok(())
}

/// Index the chunks queued by triggers since the last open or sync. Queue entries whose
/// metadata row no longer exists are dropped without writing anything.
pub(crate) fn index_pending(conn: &Connection) -> Result<()> {
    let queued: Vec<(i64, String)> = {
        let mut statement = conn.prepare(
            "SELECT p.chunk_rowid, COALESCE(c.title, '') || ' ' || COALESCE(c.content, '')
             FROM knowledge_identifier_pending p
             JOIN knowledge_chunk_meta m ON m.chunk_rowid = p.chunk_rowid
             LEFT JOIN knowledge_chunks c ON c.rowid = p.chunk_rowid",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    };
    for (rowid, text) in queued {
        write_chunk_identifiers(conn, rowid, &text)?;
    }
    conn.execute("DELETE FROM knowledge_identifier_pending", [])?;
    Ok(())
}

fn insert_words(conn: &Connection, rowid: i64, text: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO knowledge_identifiers(rowid, words) VALUES (?1, ?2)",
        params![rowid, identifier_words(text)],
    )?;
    Ok(())
}

// Test hook: fail the rebuild after the given number of rows, to prove that an interrupted
// migration rolls back completely.
#[cfg(test)]
thread_local! {
    pub(crate) static REBUILD_FAILPOINT: std::cell::Cell<Option<usize>> =
        const { std::cell::Cell::new(None) };
}

#[cfg(test)]
fn failpoint(written: usize) -> Result<()> {
    if REBUILD_FAILPOINT.with(|point| point.get()) == Some(written) {
        anyhow::bail!("injected failure after {written} identifier rows");
    }
    Ok(())
}

#[cfg(not(test))]
fn failpoint(_written: usize) -> Result<()> {
    Ok(())
}

mod split;
pub(crate) use split::{identifier_words, split_identifier};

#[cfg(test)]
pub(crate) mod tests;
