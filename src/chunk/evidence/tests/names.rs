use super::*;

#[test]
fn qualified_names_record_the_field_each_kind_owns() {
    let cases = [
        (
            "src/lib.rs",
            "fn demo() {\n    a::b::c();\n}\n",
            KIND_NAME,
            "c",
            Some("a::b"),
        ),
        (
            "src/lib.rs",
            "fn demo(value: a::b::C) {}\n",
            KIND_TYPE,
            "C",
            Some("a::b"),
        ),
        (
            "a.py",
            "def demo():\n    a.b.c\n",
            KIND_NAME,
            "c",
            Some("a.b"),
        ),
        (
            "a.go",
            "package p\nfunc demo() {\n    a.b.c\n}\n",
            KIND_NAME,
            "c",
            Some("a.b"),
        ),
        (
            "a.js",
            "function demo() {\n    a.b.c;\n}\n",
            KIND_NAME,
            "c",
            Some("a.b"),
        ),
    ];
    for (path, content, kind, specifier, qualifier) in cases {
        assert_eq!(
            qualified(path, content, kind),
            vec![(specifier.to_string(), qualifier.map(str::to_string))],
            "{path} {kind}"
        );
    }
}

#[test]
fn a_scoped_type_does_not_record_its_path_as_a_name() {
    let content = "fn demo(value: a::b::C) {}\n";
    assert_eq!(
        qualified("src/lib.rs", content, KIND_NAME),
        Vec::<(String, Option<String>)>::new()
    );
}

#[test]
fn a_noise_qualified_name_claims_the_node_and_records_nothing() {
    let content = "def demo():\n    obj.self\n";
    assert_eq!(
        qualified("a.py", content, KIND_NAME),
        Vec::<(String, Option<String>)>::new()
    );
}
