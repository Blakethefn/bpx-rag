use super::*;

mod analysis;
mod catalog;
mod embed;
mod identifier;
mod lock;

fn simulate_pre_identifier_index(conn: &Connection) {
    conn.execute_batch("DELETE FROM knowledge_identifiers; DROP TABLE knowledge_index_meta;")
        .unwrap();
}
