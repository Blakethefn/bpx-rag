use super::*;

#[test]
fn pool_create_and_import_group_the_old_pool_commands() {
    let created = parse(&[
        "eval",
        "pool",
        "create",
        "--output",
        "pool.json",
        "--key-output",
        "key.json",
    ]);
    let Commands::Eval {
        action: EvalAction::Pool(args),
    } = created.command.unwrap()
    else {
        panic!("expected pool");
    };
    let PoolAction::Create(create) = args.resolve().unwrap() else {
        panic!("expected create");
    };
    assert_eq!(create.output, PathBuf::from("pool.json"));
    assert_eq!(create.key_output, PathBuf::from("key.json"));
    assert_eq!(create.depth, DEFAULT_POOL_DEPTH);
    assert_eq!(create.seed, DEFAULT_POOL_SEED);

    let legacy = parse(&[
        "eval",
        "pool",
        "--output",
        "pool.json",
        "--key-output",
        "key.json",
        "--depth",
        "4",
    ]);
    let Commands::Eval {
        action: EvalAction::Pool(args),
    } = legacy.command.unwrap()
    else {
        panic!("expected legacy pool");
    };
    let PoolAction::Create(create) = args.resolve().unwrap() else {
        panic!("legacy pool should create");
    };
    assert_eq!(create.depth, 4);
    assert_eq!(create.seed, DEFAULT_POOL_SEED);

    let imported = parse(&[
        "eval",
        "pool",
        "import",
        "--pool",
        "pool.json",
        "--key",
        "key.json",
        "--dataset",
        "data",
        "--corpus-root",
        "corpus",
        "--output",
        "qrels.jsonl",
    ]);
    let Commands::Eval {
        action: EvalAction::Pool(args),
    } = imported.command.unwrap()
    else {
        panic!("expected pool import");
    };
    assert!(matches!(args.resolve().unwrap(), PoolAction::Import(_)));

    let alias = parse(&[
        "eval",
        "import-pool",
        "--pool",
        "pool.json",
        "--key",
        "key.json",
        "--dataset",
        "data",
        "--corpus-root",
        "corpus",
        "--output",
        "qrels.jsonl",
    ]);
    assert!(matches!(
        alias.command,
        Some(Commands::Eval {
            action: EvalAction::ImportPool(_)
        })
    ));

    let bare = parse(&["eval", "pool"]);
    let Commands::Eval {
        action: EvalAction::Pool(args),
    } = bare.command.unwrap()
    else {
        panic!("expected pool");
    };
    let missing = args.resolve().unwrap_err().to_string();
    assert!(missing.contains("eval pool create"), "{missing}");
    assert!(missing.contains("eval pool import"), "{missing}");

    let partial = parse(&["eval", "pool", "--output", "pool.json"]);
    let Commands::Eval {
        action: EvalAction::Pool(args),
    } = partial.command.unwrap()
    else {
        panic!("expected pool");
    };
    let partial_error = args.resolve().unwrap_err().to_string();
    assert!(partial_error.contains("--key-output"), "{partial_error}");
    assert!(
        partial_error.contains("eval pool create"),
        "{partial_error}"
    );

    let mixed = parse_error(&["eval", "pool", "--output", "pool.json", "create"]);
    assert!(
        mixed.contains("create") || mixed.contains("conflict") || mixed.contains("cannot"),
        "{mixed}"
    );

    let pool_help = help(&["eval", "pool", "--help"]);
    assert!(pool_help.contains("create"), "{pool_help}");
    assert!(pool_help.contains("import"), "{pool_help}");
    assert!(pool_help.contains("overwrite"), "{pool_help}");

    let eval_help = help(&["eval", "--help"]);
    assert!(!eval_help.contains("import-pool"), "{eval_help}");

    let create_help = help(&["eval", "pool", "create", "--help"]);
    assert!(create_help.contains("overwrite"), "{create_help}");
    let import_help = help(&["eval", "pool", "import", "--help"]);
    assert!(import_help.contains("overwrite"), "{import_help}");
}
