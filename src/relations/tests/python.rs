use super::*;

#[test]
fn python_imported_symbols_do_not_match_unrelated_same_named_files() {
    let dir = tempdir().unwrap();
    write_file(dir.path(), "pkg/__init__.py", "def helper(): pass\n");
    write_file(dir.path(), "helper.py", "def helper(): pass\n");
    write_file(dir.path(), "consumer.py", "from pkg import helper\n");

    let result = analyze_file(&dir.path().join("helper.py"), Some(dir.path())).unwrap();

    assert!(!incoming_from(
        &result,
        "consumer.py",
        "pkg.helper",
        "import"
    ));
}

#[test]
fn python_import_lists_and_from_imports_resolve_each_module() {
    let dir = tempdir().unwrap();
    write_file(dir.path(), "pkg/__init__.py", "");
    write_file(dir.path(), "pkg/alpha.py", "");
    write_file(dir.path(), "pkg/beta.py", "");
    write_file(dir.path(), "pkg/sub/__init__.py", "");
    write_file(dir.path(), "pkg/sub/gamma.py", "");
    write_file(
        dir.path(),
        "consumer.py",
        "import pkg.alpha, pkg.beta\nfrom pkg.sub.gamma import Thing\n",
    );

    for (target, specifier) in [
        ("pkg/alpha.py", "pkg.alpha"),
        ("pkg/beta.py", "pkg.beta"),
        ("pkg/sub/gamma.py", "pkg.sub.gamma"),
    ] {
        let result = analyze_file(&dir.path().join(target), Some(dir.path())).unwrap();
        assert!(
            incoming_from(&result, "consumer.py", specifier, "import"),
            "{target}"
        );
    }
}

#[test]
fn python_from_package_import_resolves_module_without_inventing_symbol_edges() {
    let dir = tempdir().unwrap();
    write_file(dir.path(), "pkg/__init__.py", "");
    write_file(dir.path(), "pkg/quotes.py", "");
    write_file(
        dir.path(),
        "consumer.py",
        "from pkg import quotes, MissingSymbol\n",
    );

    let quotes = analyze_file(&dir.path().join("pkg/quotes.py"), Some(dir.path())).unwrap();
    let consumer = analyze_file(&dir.path().join("consumer.py"), Some(dir.path())).unwrap();

    assert!(incoming_from(
        &quotes,
        "consumer.py",
        "pkg.quotes",
        "import"
    ));
    assert!(consumer
        .outgoing
        .iter()
        .any(|relationship| relationship.specifier == "pkg"));
    assert!(consumer
        .outgoing
        .iter()
        .any(|relationship| relationship.specifier == "pkg.quotes"));
    assert!(!consumer
        .outgoing
        .iter()
        .any(|relationship| relationship.specifier == "pkg.MissingSymbol"));
}
