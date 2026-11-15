use anyhow::{bail, Result};
use clap::parser::ValueSource;
use clap::{ArgMatches, Args, Command, FromArgMatches, Id, ValueHint};

use super::PoolCreateArgs;

#[derive(Debug)]
pub(super) struct LegacyPoolCreate {
    args: Option<PoolCreateArgs>,
    missing: Vec<String>,
}

impl LegacyPoolCreate {
    pub(super) fn specified(&self) -> bool {
        self.args.is_some() || !self.missing.is_empty()
    }

    pub(super) fn into_create(self) -> Result<PoolCreateArgs> {
        if let Some(args) = self.args {
            return Ok(args);
        }
        let missing = self.missing.join(" and ");
        bail!(
            "generating a pool requires {missing}. Run `bpx-rag eval pool create --output <FILE> --key-output <FILE>`"
        )
    }
}

impl Args for LegacyPoolCreate {
    fn augment_args(cmd: Command) -> Command {
        hide_create_flags(cmd)
    }

    fn augment_args_for_update(cmd: Command) -> Command {
        hide_create_flags(cmd)
    }
}

impl FromArgMatches for LegacyPoolCreate {
    fn from_arg_matches(matches: &ArgMatches) -> Result<Self, clap::Error> {
        let template = create_template();
        if !create_flag_on_command_line(matches, &template) {
            return Ok(Self {
                args: None,
                missing: Vec::new(),
            });
        }
        let missing = missing_required_flags(matches, &template);
        if !missing.is_empty() {
            return Ok(Self {
                args: None,
                missing,
            });
        }
        Ok(Self {
            args: Some(PoolCreateArgs::from_arg_matches(matches)?),
            missing: Vec::new(),
        })
    }

    fn update_from_arg_matches(&mut self, matches: &ArgMatches) -> Result<(), clap::Error> {
        *self = Self::from_arg_matches(matches)?;
        Ok(())
    }
}

fn create_template() -> Command {
    <PoolCreateArgs as Args>::augment_args(Command::new("pool-create"))
}

fn hide_create_flags(cmd: Command) -> Command {
    let ids = create_flag_ids();
    let cmd = <PoolCreateArgs as Args>::augment_args(cmd);
    cmd.mut_args(move |arg| {
        if ids.iter().any(|id| id == arg.get_id()) {
            arg.hide(true).required(false)
        } else {
            arg
        }
    })
}

fn create_flag_ids() -> Vec<Id> {
    create_template()
        .get_arguments()
        .map(|arg| arg.get_id().clone())
        .collect()
}

fn create_flag_on_command_line(matches: &ArgMatches, template: &Command) -> bool {
    template
        .get_arguments()
        .any(|arg| matches.value_source(arg.get_id().as_str()) == Some(ValueSource::CommandLine))
}

fn missing_required_flags(matches: &ArgMatches, template: &Command) -> Vec<String> {
    template
        .get_arguments()
        .filter(|arg| arg.is_required_set())
        .filter(|arg| matches.value_source(arg.get_id().as_str()) != Some(ValueSource::CommandLine))
        .filter_map(|arg| {
            let long = arg.get_long()?;
            Some(format!("--{long} <{}>", flag_placeholder(arg)))
        })
        .collect()
}

fn flag_placeholder(arg: &clap::Arg) -> &'static str {
    match arg.get_value_hint() {
        ValueHint::FilePath | ValueHint::AnyPath => "FILE",
        ValueHint::DirPath => "DIR",
        _ => "VALUE",
    }
}
