use super::*;
use clap::error::ErrorKind;

fn parse(args: &[&str]) -> Cli {
    Cli::try_parse_from(std::iter::once("bpx-rag").chain(args.iter().copied()))
        .unwrap_or_else(|error| panic!("{error}"))
}

fn parse_error(args: &[&str]) -> String {
    Cli::try_parse_from(std::iter::once("bpx-rag").chain(args.iter().copied()))
        .expect_err("command should be rejected")
        .to_string()
}

fn help(args: &[&str]) -> String {
    let error = Cli::try_parse_from(std::iter::once("bpx-rag").chain(args.iter().copied()))
        .expect_err("help should stop parsing");
    assert_eq!(error.kind(), ErrorKind::DisplayHelp);
    error.to_string()
}

fn source_name(command: Option<Commands>) -> String {
    match command {
        Some(Commands::Add { label, .. } | Commands::Sync { label: Some(label) }) => label,
        other => panic!("expected a source flag, got {other:?}"),
    }
}

#[test]
fn init_is_a_command_and_the_flag_still_parses() {
    assert!(matches!(parse(&["init"]).command, Some(Commands::Init)));

    let flagged = parse(&["--init"]);
    assert!(flagged.init);
    assert!(flagged.command.is_none());

    let flagged_with_command = parse(&["--init", "serve"]);
    assert!(flagged_with_command.init);
    assert!(matches!(
        flagged_with_command.command,
        Some(Commands::Serve)
    ));
}

#[test]
fn help_lists_init_and_keeps_the_flag_off_the_command_list() {
    let root = help(&["--help"]);
    assert!(root.contains("init"));
    assert!(!root.contains("--init"), "{root}");
    assert!(root.contains("Does not delete files on disk"), "{root}");

    let init_help = help(&["init", "--help"]);
    assert!(
        init_help.contains("unchanged") || init_help.contains("already there"),
        "{init_help}"
    );
    assert!(init_help.contains("--init"), "{init_help}");
}

#[test]
fn source_flag_is_canonical_and_label_remains_an_alias() {
    assert_eq!(
        source_name(parse(&["add", "/tmp/proj", "--source", "api"]).command),
        "api"
    );
    assert_eq!(
        source_name(parse(&["add", "/tmp/proj", "--label", "api"]).command),
        "api"
    );

    let synced = parse(&["sync", "--source", "api"]);
    assert!(
        matches!(synced.command, Some(Commands::Sync { label: Some(label) }) if label == "api")
    );
    let embedded = parse(&["embed", "--label", "api"]);
    assert!(
        matches!(embedded.command, Some(Commands::Embed { label: Some(label) }) if label == "api")
    );
    let scrubbed = parse(&["scrub"]);
    assert!(matches!(
        scrubbed.command,
        Some(Commands::Scrub { label: None })
    ));

    let missing = parse_error(&["add", "/tmp/proj"]);
    assert!(missing.contains("--source"), "{missing}");

    let both = parse_error(&["add", "/tmp/proj", "--source", "api", "--label", "other"]);
    assert!(both.contains("--source"), "{both}");

    let search_label = parse_error(&["search", "query", "--label", "api"]);
    assert!(
        search_label.contains("unexpected") || search_label.contains("--label"),
        "{search_label}"
    );
}

#[test]
fn watch_enable_and_disable_keep_on_and_off() {
    assert!(matches!(
        parse(&["watch", "enable", "atlas"]).command,
        Some(Commands::Watch { action: WatchAction::Enable { label } }) if label == "atlas"
    ));
    assert!(matches!(
        parse(&["watch", "on", "atlas"]).command,
        Some(Commands::Watch { action: WatchAction::Enable { label } }) if label == "atlas"
    ));
    assert!(matches!(
        parse(&["watch", "disable", "atlas"]).command,
        Some(Commands::Watch { action: WatchAction::Disable { label } }) if label == "atlas"
    ));
    assert!(matches!(
        parse(&["watch", "off", "atlas"]).command,
        Some(Commands::Watch { action: WatchAction::Disable { label } }) if label == "atlas"
    ));

    let listed = help(&["watch", "--help"]);
    assert!(listed.contains("enable"), "{listed}");
    assert!(listed.contains("disable"), "{listed}");
    assert!(listed.contains("Does not start the watcher"), "{listed}");
    assert!(listed.contains("Does not stop the watcher"), "{listed}");
    assert!(
        !listed.lines().any(|line| {
            let trimmed = line.trim();
            trimmed.starts_with("on ") || trimmed.starts_with("off ")
        }),
        "{listed}"
    );
}

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
