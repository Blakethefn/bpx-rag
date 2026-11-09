use super::merge_neighbors;

#[test]
fn neighbors_follow_the_top_hits_without_duplicates() {
    let ranked = vec!["a", "b", "c", "d", "e"];
    let neighbors = vec!["x", "a", "y"];
    let merged = merge_neighbors(ranked, neighbors, 2, 6, |item| item.to_string());
    assert_eq!(merged, vec!["a", "b", "x", "y", "c", "d"]);
}

#[test]
fn neighbors_never_push_out_the_top_hits() {
    let ranked = vec!["a", "b"];
    let neighbors = vec!["x", "y", "z"];
    let merged = merge_neighbors(ranked, neighbors, 2, 2, |item| item.to_string());
    assert_eq!(merged, vec!["a", "b"]);
}
