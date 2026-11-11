use super::*;
use clap::error::ErrorKind;

mod pool;

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
