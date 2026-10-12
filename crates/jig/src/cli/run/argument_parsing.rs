//! The generic argument pipeline: normalize argv, parse, validate, and report
//! usage errors. Command-specific rules live with their commands; this module
//! only asks each owner in turn.

use std::ffi::OsString;
use std::io::Write;
use std::process;

use clap::{
    Parser,
    error::{ContextKind, ContextValue, ErrorKind},
};
use jig_commands::root_commands;

use crate::cli::output::print_json;
use crate::cli::structured_error::json_error_payload;
use crate::cli::{Cli, CommandKind, bootstrap_hints, check};

pub(in crate::cli) fn parse_cli() -> Cli {
    let args = normalize_args(std::env::args_os().collect());
    let command_args = || args.iter().skip(1).cloned();
    let report_json_errors = args_request_json(command_args());

    match Cli::try_parse_from(&args) {
        Ok(cli) => {
            if let Some(error) = post_parse_usage_error(&cli) {
                exit_with_cli_error(error, &args, report_json_errors);
            }
            cli
        }
        Err(error) => {
            if let Some(hint) = super::workflow_recovery::hint(&args, &error) {
                let message = format!("{}\n{hint}", augmented_cli_error_message(&error, &args));
                if report_json_errors {
                    let _ = print_json(&json_error_payload("usage", &message, error.exit_code()));
                } else {
                    let _ = writeln!(std::io::stderr(), "{message}");
                }
                process::exit(error.exit_code());
            }
            exit_with_cli_error(error, &args, report_json_errors)
        }
    }
}

/// Applies each command's argv normalization before Clap sees the arguments.
pub(in crate::cli) fn normalize_args(args: Vec<OsString>) -> Vec<OsString> {
    match root_subcommand_index(&args) {
        Some(index) if args[index] == root_commands::CHECK.name => {
            check::normalize_external_global_flags(args, index)
        }
        _ => args,
    }
}

pub(in crate::cli) const ROOT_FLAG_OPTIONS: &[&str] = &["--json"];
pub(in crate::cli) const ROOT_VALUE_OPTIONS: &[&str] = &[
    "--__launcher-contract-version",
    "--__launcher-profile",
    "--__launcher-repo-root",
];
pub(in crate::cli) fn root_subcommand_index(args: &[OsString]) -> Option<usize> {
    let mut index = 1;
    while index < args.len() {
        let arg = args[index].to_string_lossy();
        if ROOT_FLAG_OPTIONS.contains(&arg.as_ref()) {
            index += 1;
        } else if ROOT_VALUE_OPTIONS.contains(&arg.as_ref()) {
            index = index.saturating_add(2);
        } else if ROOT_VALUE_OPTIONS.iter().any(|option| {
            arg.strip_prefix(option)
                .is_some_and(|suffix| suffix.starts_with('='))
        }) {
            index += 1;
        } else if arg == "--" || arg.starts_with('-') {
            return None;
        } else {
            return Some(index);
        }
    }
    None
}

/// Rules Clap cannot express because they involve the global `--json` flag.
/// Each command states its own conflicts.
pub(in crate::cli) fn post_parse_usage_error(cli: &Cli) -> Option<clap::Error> {
    let message = match &cli.command {
        CommandKind::Status(opts) => opts.usage_conflict(cli.json),
        CommandKind::Ui(opts) => opts.usage_conflict(cli.json),
        _ => None,
    }?;
    Some(clap::Error::raw(ErrorKind::ArgumentConflict, message))
}

pub(in crate::cli) fn exit_with_cli_error(
    error: clap::Error,
    args: &[OsString],
    json_output: bool,
) -> ! {
    if json_output
        && !matches!(
            error.kind(),
            ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
        )
    {
        let exit_status = error.exit_code();
        let message = augmented_cli_error_message(&error, args);
        let _ = print_json(&json_error_payload("usage", &message, exit_status));
        process::exit(exit_status);
    }

    if let Some(hint) = usage_hint(&error, args) {
        // If stderr is closed, there is nowhere useful to report the parse hint.
        let _ = writeln!(std::io::stderr(), "{error}\n{hint}");
        process::exit(error.exit_code());
    }

    error.exit();
}

pub(in crate::cli) fn args_request_json(args: impl IntoIterator<Item = OsString>) -> bool {
    args.into_iter()
        .take_while(|arg| arg != "--")
        .any(|arg| arg == "--json")
}

pub(in crate::cli) fn augmented_cli_error_message(
    error: &clap::Error,
    args: &[OsString],
) -> String {
    let mut message = error.to_string();
    if let Some(hint) = usage_hint(error, args) {
        message.push('\n');
        message.push_str(&hint);
    }
    message
}

/// The first hint a command owner offers for this usage error. Owners read
/// Clap's structured error context and the root command, never rendered text.
pub(in crate::cli) fn usage_hint(error: &clap::Error, args: &[OsString]) -> Option<String> {
    let root_command = root_subcommand_index(args).and_then(|index| args[index].to_str());
    bootstrap_hints::template_hint(error)
        .or_else(|| check::moved_command_hint(root_command?, invalid_subcommand(error)?))
        .or_else(|| bootstrap_hints::missing_init_path_hint(error, root_command?))
}

/// The subcommand name Clap rejected, from its structured error context.
fn invalid_subcommand(error: &clap::Error) -> Option<&str> {
    if error.kind() != ErrorKind::InvalidSubcommand {
        return None;
    }
    match error.get(ContextKind::InvalidSubcommand)? {
        ContextValue::String(name) => Some(name),
        _ => None,
    }
}

#[cfg(test)]
mod argument_normalization_tests {
    use std::ffi::OsString;

    use clap::CommandFactory;

    use super::{Cli, ROOT_FLAG_OPTIONS, ROOT_VALUE_OPTIONS, normalize_args};
    use crate::cli::check::CHECK_VALUE_OPTIONS;

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn normalization_option_tables_match_the_clap_contract() {
        let command = Cli::command();
        let mut root_flags = command
            .get_arguments()
            .filter(|arg| !arg.get_action().takes_values())
            .filter_map(|arg| arg.get_long())
            .filter(|long| !matches!(*long, "help" | "version"))
            .map(|long| format!("--{long}"))
            .collect::<Vec<_>>();
        let mut root_values = command
            .get_arguments()
            .filter(|arg| arg.get_action().takes_values())
            .filter_map(|arg| arg.get_long())
            .map(|long| format!("--{long}"))
            .collect::<Vec<_>>();
        let check = command.find_subcommand("check").unwrap();
        let mut check_values = check
            .get_arguments()
            .filter(|arg| arg.get_action().takes_values())
            .filter_map(|arg| arg.get_long())
            .map(|long| format!("--{long}"))
            .collect::<Vec<_>>();
        root_flags.sort();
        root_values.sort();
        check_values.sort();

        let mut expected_root_flags = ROOT_FLAG_OPTIONS.to_vec();
        let mut expected_root_values = ROOT_VALUE_OPTIONS.to_vec();
        let mut expected_check_values = CHECK_VALUE_OPTIONS.to_vec();
        expected_root_flags.sort_unstable();
        expected_root_values.sort_unstable();
        expected_check_values.sort_unstable();
        assert_eq!(root_flags, expected_root_flags);
        assert_eq!(root_values, expected_root_values);
        assert_eq!(check_values, expected_check_values);
    }

    #[test]
    fn check_text_used_as_an_option_value_is_not_a_root_command() {
        let original = args(&["jig", "status", "run", "check", "--json"]);

        assert_eq!(normalize_args(original.clone()), original);
    }

    #[test]
    fn external_check_moves_only_actual_global_flags() {
        assert_eq!(
            normalize_args(args(&["jig", "check", "api:test", "--json"])),
            args(&["jig", "--json", "check", "api:test"])
        );
        assert_eq!(
            normalize_args(args(&["jig", "check", "--profile", "--json",])),
            args(&["jig", "check", "--profile", "--json"])
        );
    }

    #[test]
    fn external_check_places_help_after_check_when_json_moves_to_the_front() {
        for prefix in [
            &[][..],
            &["--__launcher-repo-root", "/tmp/ExampleProject"][..],
            &["--__launcher-repo-root=/tmp/ExampleProject"][..],
        ] {
            for flags in [["--json", "--help"], ["--help", "--json"]] {
                let mut original = vec!["jig"];
                original.extend_from_slice(prefix);
                original.extend(["check", "api:test"]);
                original.extend(flags);
                let mut expected = vec!["jig", "--json"];
                expected.extend_from_slice(prefix);
                expected.extend(["check", "--help", "api:test"]);
                assert_eq!(normalize_args(args(&original)), args(&expected));
            }
        }
    }
}
