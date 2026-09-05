use super::*;

pub(in crate::cli) fn parse_cli() -> Cli {
    let args = normalize_external_check_global_flags(std::env::args_os().collect());
    let command_args = || args.iter().skip(1).cloned();
    let report_json_errors = args_request_json(command_args()) && !args_target_mcp(command_args());

    match Cli::try_parse_from(args) {
        Ok(cli) => {
            if let Some(error) = post_parse_usage_error(&cli) {
                exit_with_cli_error(error, report_json_errors);
            }
            cli
        }
        Err(error) => exit_with_cli_error(error, report_json_errors),
    }
}

pub(in crate::cli) const ROOT_FLAG_OPTIONS: &[&str] = &["--json"];
pub(in crate::cli) const ROOT_VALUE_OPTIONS: &[&str] = &[
    "--__launcher-contract-version",
    "--__launcher-profile",
    "--__launcher-repo-root",
];
pub(in crate::cli) const CHECK_VALUE_OPTIONS: &[&str] = &[
    "--plan-id",
    "--profile",
    "--affected",
    "--comparison-base",
    "--comparison-exact-tree",
    "--comparison-provenance",
];

pub(in crate::cli) fn normalize_external_check_global_flags(
    mut args: Vec<OsString>,
) -> Vec<OsString> {
    let Some(check_index) =
        root_subcommand_index(&args).filter(|index| args[*index] == tool_defs::cli_command::CHECK)
    else {
        return args;
    };
    let separator_index = args
        .iter()
        .position(|arg| arg == "--")
        .unwrap_or(args.len());
    if check_index >= separator_index {
        return args;
    }

    let external_selector = check_external_selector(&args[check_index + 1..separator_index]);
    let mut moved_json = Vec::new();
    let mut moved_help = Vec::new();
    let mut index = check_index + 1;
    let mut option_value = false;
    while index < args.len() && args[index] != "--" {
        if option_value {
            option_value = false;
            index += 1;
        } else if args[index]
            .to_str()
            .is_some_and(|arg| CHECK_VALUE_OPTIONS.contains(&arg))
        {
            option_value = true;
            index += 1;
        } else if args[index] == "--json" {
            moved_json.push(args.remove(index));
        } else if external_selector && matches!(args[index].to_str(), Some("--help" | "-h")) {
            moved_help.push(args.remove(index));
        } else {
            index += 1;
        }
    }
    for flag in moved_json.into_iter().rev() {
        args.insert(1, flag);
    }
    if !moved_help.is_empty() {
        let check_index = root_subcommand_index(&args)
            .expect("check command remains present after normalization");
        for flag in moved_help.into_iter().rev() {
            args.insert(check_index + 1, flag);
        }
    }
    args
}

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

pub(in crate::cli) fn check_external_selector(args: &[OsString]) -> bool {
    let mut skip_value = false;
    for arg in args {
        let arg = arg.to_string_lossy();
        if skip_value {
            skip_value = false;
            continue;
        }
        if CHECK_VALUE_OPTIONS.contains(&arg.as_ref()) {
            skip_value = true;
            continue;
        }
        if arg.starts_with('-') {
            continue;
        }
        return !CHECK_SUBCOMMAND_NAMES.contains(&arg.as_ref());
    }
    false
}

pub(in crate::cli) fn post_parse_usage_error(cli: &Cli) -> Option<clap::Error> {
    let message = match &cli.command {
        CommandKind::Status(opts) if cli.json && opts.tui => {
            "`--tui` cannot be combined with `--json`"
        }
        CommandKind::Status(opts) if opts.command.is_some() && opts.tui => {
            "a status subject cannot be combined with `--tui`"
        }
        CommandKind::Ui(opts) if opts.retired_port.is_some() => {
            "the `jig ui` browser server and `--port` option were removed in 0.3.0; use `jig ui` for the terminal dashboard or `jig ui --json` for one-shot data (`--port` will stop parsing in 0.4.0)"
        }
        CommandKind::Ui(opts)
            if cli.json
                && (opts.refresh_seconds.is_some() || opts.status_refresh_seconds.is_some()) =>
        {
            "`--refresh-seconds` and `--status-refresh-seconds` cannot be combined with `--json`"
        }
        CommandKind::Ui(opts)
            if cli.json && opts.plan.is_some() && opts.timeline_limit.is_some() =>
        {
            "`--timeline-limit` cannot be combined with `--plan` in JSON mode"
        }
        CommandKind::Work(WorkCommand::Start(opts)) if cli.json && opts.print_plan_id => {
            "`--print-plan-id` cannot be combined with `--json`"
        }
        _ => return None,
    };
    Some(clap::Error::raw(ErrorKind::ArgumentConflict, message))
}

pub(in crate::cli) fn exit_with_cli_error(error: clap::Error, json_output: bool) -> ! {
    if json_output
        && !matches!(
            error.kind(),
            ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
        )
    {
        let exit_status = error.exit_code();
        let message = augmented_cli_error_message(&error);
        let _ = print_json(&json_error_payload("usage", &message, exit_status));
        process::exit(exit_status);
    }

    if should_add_template_hint(&error) {
        let message = error.to_string();
        // If stderr is closed, there is nowhere useful to report the parse hint.
        let _ = writeln!(std::io::stderr(), "{message}\n{TEMPLATE_ERROR_HINT}");
        process::exit(error.exit_code());
    }

    if let Some(hint) = moved_check_command_hint(&error) {
        let message = error.to_string();
        // If stderr is closed, there is nowhere useful to report the parse hint.
        let _ = writeln!(std::io::stderr(), "{message}\n{hint}");
        process::exit(error.exit_code());
    }

    if let Some(hint) = missing_init_path_hint(&error) {
        let message = error.to_string();
        // If stderr is closed, there is nowhere useful to report the parse hint.
        let _ = writeln!(std::io::stderr(), "{message}\n{hint}");
        process::exit(error.exit_code());
    }

    error.exit();
}

pub(in crate::cli) fn args_request_json(args: impl IntoIterator<Item = OsString>) -> bool {
    args.into_iter()
        .take_while(|arg| arg != "--")
        .any(|arg| arg == "--json")
}

pub(in crate::cli) fn args_target_mcp(args: impl IntoIterator<Item = OsString>) -> bool {
    // Callers pass command arguments without argv[0]. Prefix a synthetic
    // executable so the same root-option parser used by normalization can
    // skip generated-launcher option values without mistaking one for `mcp`.
    let argv = std::iter::once(OsString::from("jig"))
        .chain(args)
        .take_while(|arg| arg != "--")
        .collect::<Vec<_>>();
    root_subcommand_index(&argv).is_some_and(|index| argv[index] == tool_defs::cli_command::MCP)
}

pub(in crate::cli) fn augmented_cli_error_message(error: &clap::Error) -> String {
    let mut message = error.to_string();
    let hint = if should_add_template_hint(error) {
        Some(TEMPLATE_ERROR_HINT.to_string())
    } else if let Some(hint) = moved_check_command_hint(error) {
        Some(hint)
    } else {
        missing_init_path_hint(error).map(str::to_string)
    };
    if let Some(hint) = hint {
        message.push('\n');
        message.push_str(&hint);
    }
    message
}

pub(in crate::cli) fn missing_init_path_hint(error: &clap::Error) -> Option<&'static str> {
    if error.kind() != ErrorKind::MissingRequiredArgument {
        return None;
    }

    if !error.context().any(|(kind, value)| {
        kind == ContextKind::Usage && context_contains(value, "jig init <PATH>")
    }) {
        return None;
    }

    Some(
        "\
`jig init` creates a new Jig-managed repository.
Use `jig adopt .` for an existing repository.

Use one of:
  jig init /path/to/new-repo --preset harness-only --repo-name new-repo --sqlx-enabled false --no-input --no-vault
  jig init /path/to/new-repo --preset rust-react
  jig init /path/to/new-repo --preset rust-react --db postgres --frontends web,landing,admin
  jig adopt .              # preview Jig adoption for this existing repo
  jig adopt . --write      # apply Jig adoption to this existing repo
  jig presets              # list available project scaffolds",
    )
}

pub(in crate::cli) fn moved_check_command_hint(error: &clap::Error) -> Option<String> {
    if error.kind() != ErrorKind::InvalidSubcommand {
        return None;
    }

    let message = error.to_string();
    let moved = [
        ("fmt-check", "jig check fmt"),
        ("clippy", "jig check clippy"),
        ("test", "jig check test"),
        ("test-locked", "jig check test-locked"),
        ("sqlx-check", "jig check sqlx"),
        ("schema-check", "jig check schema"),
        ("contract-check", "jig check contract"),
        ("check-agent-guides", "jig check agent-guides"),
        (
            "check-migration-immutability",
            "jig check migration-immutability",
        ),
        (
            "check-sqlx-unchecked-non-test",
            "jig check sqlx-unchecked-non-test",
        ),
    ];

    // Like the nested agent-map case below, this depends on Clap 4.6.1 formatted
    // usage text and is only a best-effort migration hint. Global options such as
    // --json make the top-level usage line include [OPTIONS]; recheck this matcher
    // on Clap upgrades or when adding more global flags.
    if message.contains("Usage: jig [OPTIONS] <COMMAND>")
        && let Some((_, replacement)) = moved
            .iter()
            .find(|(legacy, _)| message.contains(&format!("'{legacy}'")))
    {
        return Some(moved_check_hint_for(replacement));
    }

    // Clap 4.6.1 reports nested invalid subcommands through formatted usage text;
    // this hint is best-effort and may disappear if that formatting changes.
    if message.contains("unrecognized subcommand 'check'")
        && message.contains("Usage: jig agent-map [OPTIONS] <COMMAND>")
    {
        return Some(moved_check_hint_for("jig check agent-map"));
    }

    None
}

pub(in crate::cli) fn moved_check_hint_for(replacement: &str) -> String {
    format!("This check command moved. Use:\n  {replacement}")
}

pub(in crate::cli) fn should_add_template_hint(error: &clap::Error) -> bool {
    if !matches!(
        error.kind(),
        ErrorKind::InvalidValue | ErrorKind::TooFewValues
    ) {
        return false;
    }
    error
        .context()
        .any(|(kind, value)| kind == ContextKind::InvalidArg && context_mentions_template(value))
}

pub(in crate::cli) fn context_contains(value: &ContextValue, needle: &str) -> bool {
    match value {
        ContextValue::String(value) => value.contains(needle),
        ContextValue::Strings(values) => values.iter().any(|value| value.contains(needle)),
        ContextValue::StyledStr(value) => value.to_string().contains(needle),
        ContextValue::StyledStrs(values) => values
            .iter()
            .any(|value| value.to_string().contains(needle)),
        _ => false,
    }
}

pub(in crate::cli) fn context_mentions_template(value: &ContextValue) -> bool {
    match value {
        ContextValue::String(value) => is_template_arg(value),
        ContextValue::Strings(values) => values.iter().any(|value| is_template_arg(value)),
        ContextValue::StyledStr(value) => is_template_arg(&value.to_string()),
        ContextValue::StyledStrs(values) => values
            .iter()
            .any(|value| is_template_arg(&value.to_string())),
        _ => false,
    }
}

pub(in crate::cli) fn is_template_arg(value: &str) -> bool {
    value
        .split_whitespace()
        .next()
        .is_some_and(|arg| arg == "--template")
}

#[cfg(test)]
mod argument_normalization_tests {
    use std::ffi::OsString;

    use clap::CommandFactory;

    use super::{
        CHECK_VALUE_OPTIONS, Cli, ROOT_FLAG_OPTIONS, ROOT_VALUE_OPTIONS,
        normalize_external_check_global_flags,
    };

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
        let original = args(&["jig", "work", "start", "--goal", "check", "--json"]);

        assert_eq!(
            normalize_external_check_global_flags(original.clone()),
            original
        );
    }

    #[test]
    fn external_check_moves_only_actual_global_flags() {
        assert_eq!(
            normalize_external_check_global_flags(args(&["jig", "check", "api:test", "--json"])),
            args(&["jig", "--json", "check", "api:test"])
        );
        assert_eq!(
            normalize_external_check_global_flags(args(&["jig", "check", "--profile", "--json",])),
            args(&["jig", "check", "--profile", "--json"])
        );
    }
}
