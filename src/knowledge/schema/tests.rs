use super::*;

mod analysis;
mod embed;
mod identifier;
mod lock;

/// Chunks as an index from before the identifier index holds them: content rows with no
/// identifier rows and no index metadata.
fn simulate_pre_identifier_index(conn: &Connection) {
    conn.execute_batch("DELETE FROM knowledge_identifiers; DROP TABLE knowledge_index_meta;")
        .unwrap();
}
