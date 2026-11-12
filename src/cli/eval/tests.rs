use super::*;
use crate::cli::{Cli, Commands};
use clap::{CommandFactory, Parser};
use std::path::PathBuf;

fn parse(args: &[&str]) -> Cli {
    Cli::try_parse_from(std::iter::once("bpx-rag").chain(args.iter().copied()))
        .unwrap_or_else(|error| panic!("{error}"))
}

fn pool_command() -> clap::Command {
    Cli::command()
        .find_subcommand("eval")
        .expect("eval command")
        .find_subcommand("pool")
        .expect("pool command")
        .clone()
}

fn sample_value(arg: &clap::Arg) -> &'static str {
    match arg.get_value_hint() {
        ValueHint::FilePath | ValueHint::DirPath | ValueHint::AnyPath => "sample",
        _ => "1",
    }
}

#[test]
fn legacy_pool_hides_every_create_flag() {
    let pool = pool_command();
    let create = pool.find_subcommand("create").expect("create command");
    let mut flags = 0;
    for arg in create.get_arguments() {
        let Some(long) = arg.get_long() else {
            continue;
        };
        if long == "help" {
            continue;
        }
        flags += 1;
        assert!(!arg.is_hide_set(), "create should show --{long}");
        let legacy = pool
            .get_arguments()
            .find(|candidate| candidate.get_long() == Some(long))
            .unwrap_or_else(|| panic!("legacy pool is missing --{long}"));
        assert!(legacy.is_hide_set(), "legacy pool should hide --{long}");
    }
    assert!(flags > 0, "pool create should publish its flags");
}

#[test]
fn each_legacy_create_flag_selects_create() {
    let pool = pool_command();
    let create = pool.find_subcommand("create").expect("create command");
    for arg in create.get_arguments() {
        let Some(long) = arg.get_long() else {
            continue;
        };
        if long == "help" {
            continue;
        }
        let mut tokens = vec!["eval".to_string(), "pool".to_string(), format!("--{long}")];
        if arg.get_action().takes_values() {
            tokens.push(sample_value(arg).to_string());
        }
        let refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
        let parsed = parse(&refs);
        let Commands::Eval {
            action: EvalAction::Pool(args),
        } = parsed.command.unwrap()
        else {
            panic!("expected pool for --{long}");
        };
        let error = args.resolve().unwrap_err().to_string();
        assert!(
            error.contains("generating a pool requires"),
            "--{long} was parsed and ignored: {error}"
        );
        assert!(
            !error.contains("choose a pool command"),
            "--{long} was parsed and ignored: {error}"
        );
    }
}

#[test]
fn legacy_pool_create_reads_the_create_fields() {
    let parsed = parse(&[
        "eval",
        "pool",
        "--dataset",
        "data",
        "--corpus-root",
        "corpus",
        "--output",
        "pool.json",
        "--key-output",
        "key.json",
        "--depth",
        "4",
        "--seed",
        "9",
    ]);
    let Commands::Eval {
        action: EvalAction::Pool(args),
    } = parsed.command.unwrap()
    else {
        panic!("expected pool");
    };
    let PoolAction::Create(create) = args.resolve().unwrap() else {
        panic!("legacy pool should create");
    };
    assert_eq!(create.dataset, Some(PathBuf::from("data")));
    assert_eq!(create.corpus_root, Some(PathBuf::from("corpus")));
    assert_eq!(create.output, PathBuf::from("pool.json"));
    assert_eq!(create.key_output, PathBuf::from("key.json"));
    assert_eq!(create.depth, 4);
    assert_eq!(create.seed, 9);
}
