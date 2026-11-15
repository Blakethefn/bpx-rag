use super::{assert_chunk, by_path, insert_chunk, insert_file, insert_link};
use crate::knowledge::open_in_memory;

#[test]
fn related_chunks_keep_column_positions_on_both_directions() {
    let store = open_in_memory().unwrap();
    store.add_source("notes", "/notes", &[], &[]).unwrap();
    let source_id = store.get_source("notes").unwrap().unwrap().id;
    let conn = store.conn();

    let origin = insert_file(conn, source_id, "origin.md");
    let outgoing = insert_chunk(
        conn,
        source_id,
        "out.md",
        "Out title",
        "out body",
        Some(11),
        Some(29),
    );
    let outgoing_blank = insert_chunk(
        conn,
        source_id,
        "out-blank.md",
        "Out blank title",
        "out blank body",
        None,
        None,
    );
    let incoming = insert_chunk(
        conn,
        source_id,
        "in.md",
        "In title",
        "in body",
        Some(3),
        Some(8),
    );
    let incoming_blank = insert_chunk(
        conn,
        source_id,
        "in-blank.md",
        "In blank title",
        "in blank body",
        None,
        None,
    );
    insert_link(conn, origin, outgoing);
    insert_link(conn, origin, outgoing_blank);
    insert_link(conn, incoming, origin);
    insert_link(conn, incoming_blank, origin);

    let related = store.get_related_chunks(origin, 10, None).unwrap();
    let by_path = by_path(&related);

    assert_eq!(by_path.len(), 4);
    assert_chunk(
        by_path["out.md"],
        outgoing,
        "out.md",
        "Out title",
        "out body",
        11,
        29,
    );
    assert_chunk(
        by_path["out-blank.md"],
        outgoing_blank,
        "out-blank.md",
        "Out blank title",
        "out blank body",
        0,
        0,
    );
    assert_chunk(
        by_path["in.md"],
        incoming,
        "in.md",
        "In title",
        "in body",
        3,
        8,
    );
    assert_chunk(
        by_path["in-blank.md"],
        incoming_blank,
        "in-blank.md",
        "In blank title",
        "in blank body",
        0,
        0,
    );
}
