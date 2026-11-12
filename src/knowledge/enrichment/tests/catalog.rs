use super::super::{
    build_enrichment_fn, enrichment, enrichment_names, validate_enrichments, CATALOG,
};
use std::path::Path;

#[test]
fn catalog_names_are_the_match_and_the_json_keys() {
    let names = enrichment_names();
    assert_eq!(names, ["frontmatter", "wikilinks", "folder_tags"]);
    assert!(CATALOG.iter().map(|(name, _)| *name).eq(names.iter().copied()));

    let content = "---\ntype: task\n---\nSee [[other]]";
    let path = Path::new("/vault/01-projects/note.md");
    let base = Path::new("/vault");

    for name in names.iter().copied() {
        let (key, extract) = enrichment(name).unwrap_or_else(|| panic!("{name} has no extractor"));
        assert_eq!(key, name);
        assert!(extract(content, path, base).is_some());

        validate_enrichments(&[name.to_string()]).unwrap_or_else(|error| panic!("{name}: {error}"));
        let enrich =
            build_enrichment_fn(&[name.to_string()], base).unwrap_or_else(|| panic!("{name}"));
        let value = enrich(content, path);
        assert!(value.get(name).is_some(), "{name} wrote no JSON key");
    }

    assert!(enrichment("missing").is_none());
    let error = validate_enrichments(&["missing".to_string()])
        .unwrap_err()
        .to_string();
    assert_eq!(
        error,
        format!(
            "unknown enrichment 'missing'; choose from: {}",
            names.join(", ")
        )
    );
}
