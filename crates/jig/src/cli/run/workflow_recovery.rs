//! Usage recovery only: validate a suggested argv, never dispatch it.
use super::*;

pub(super) fn hint(args: &[OsString], error: &clap::Error) -> Option<String> {
    let executable = recovery_executable(args)?;
    let quoted_executable = crate::shell::quote(&executable);
    let Some(root) = root_subcommand_index(args) else {
        return (error.kind() == ErrorKind::UnknownArgument && invalid_option(error, "--summary"))
            .then(|| {
                format!("No global --summary option exists. See:\n  {quoted_executable} --help")
            });
    };
    // Launcher handoff arguments are private and must not appear in a retry.
    let mut retry = vec![executable];
    retry.extend(
        args[1..root]
            .iter()
            .filter(|arg| *arg == "--json")
            .map(|_| "--json".to_owned()),
    );
    let command = retry.len();
    retry.extend(
        args[root..]
            .iter()
            .map(|arg| arg.to_str().map(str::to_owned))
            .collect::<Option<Vec<_>>>()?,
    );
    if error.kind() == ErrorKind::InvalidSubcommand && retry[command] == "contract" {
        retry.splice(command..=command, ["check".into(), "contract".into()]);
        return Some(suggestion(
            retry,
            &format!("{quoted_executable} check contract --help"),
        ));
    }
    if error.kind() != ErrorKind::UnknownArgument {
        return None;
    }
    let nested = (command + 1..retry.len()).find(|index| retry[*index] != "--json");
    let work = (retry[command] == "work")
        .then_some(nested)
        .flatten()
        .filter(|index| !retry[*index].starts_with('-'));
    if let Some(nested) = work {
        if retry[nested] == "start" && invalid_option(error, "--description") {
            replace_option(&mut retry, "--description", "--body")?;
            return Some(suggestion(
                retry,
                &format!("{quoted_executable} work start --help"),
            ));
        }
        if retry[nested] == "status" && invalid_option(error, "--plan-id") {
            retry[nested] = "gates".into();
            return Some(suggestion(
                retry,
                &format!("{quoted_executable} work gates --help"),
            ));
        }
    }
    if invalid_option(error, "--summary") {
        let projected = work
            .is_some_and(|index| matches!(retry[index].as_str(), "check" | "gates" | "evidence"))
            || retry[command] == "info";
        let help = work.map_or_else(
            || format!("{quoted_executable} {} --help", retry[command]),
            |index| format!("{quoted_executable} work {} --help", retry[index]),
        );
        if projected {
            let flag = retry.iter().position(|arg| arg == "--summary");
            if let Some(flag) = flag {
                retry.splice(flag..=flag, ["--projection".into(), "agent-v1".into()]);
                return Some(suggestion(retry, &help));
            }
        }
        // No equivalent compact operation exists here. Do not guess a plan or
        // redirect an unrelated command to a receipt-producing operation.
        return Some(format!(
            "No --summary option exists for this command. See:\n  {help}"
        ));
    }
    None
}

fn recovery_executable(args: &[OsString]) -> Option<String> {
    let mut prefix = args.iter().skip(1);
    while let Some(arg) = prefix.next() {
        let arg = arg.to_str()?;
        let root = if arg == "--__launcher-repo-root" {
            Some(prefix.next()?.to_str()?)
        } else {
            arg.strip_prefix("--__launcher-repo-root=")
        };
        if let Some(root) = root {
            // The launcher may have been called from outside the repository.
            // An absolute public launcher preserves its owner without exposing
            // the private handoff flags or requiring a global installation.
            let root = std::path::Path::new(root);
            return root
                .is_absolute()
                .then(|| root.join("scripts/jig"))?
                .to_str()
                .map(str::to_owned);
        }
        if ROOT_VALUE_OPTIONS.contains(&arg) {
            prefix.next()?;
        } else if !ROOT_FLAG_OPTIONS.contains(&arg)
            && !ROOT_VALUE_OPTIONS.iter().any(|option| {
                arg.strip_prefix(option)
                    .is_some_and(|value| value.starts_with('='))
            })
        {
            break;
        }
    }
    args.first()?
        .to_str()
        .filter(|arg| !arg.is_empty())
        .map(str::to_owned)
}

fn invalid_option(error: &clap::Error, option: &str) -> bool {
    error.context().any(|(kind, value)| {
        kind == ContextKind::InvalidArg
            && matches!(value, ContextValue::String(value) if value == option || value.starts_with(&format!("{option}=")))
    })
}

fn replace_option(args: &mut [String], old: &str, new: &str) -> Option<()> {
    let arg = args.iter_mut().find(|arg| {
        arg.as_str() == old
            || arg
                .strip_prefix(old)
                .is_some_and(|suffix| suffix.starts_with('='))
    })?;
    *arg = format!("{new}{}", &arg[old.len()..]);
    Some(())
}

fn suggestion(args: Vec<String>, help: &str) -> String {
    let valid = match Cli::try_parse_from(&args) {
        Ok(cli) => {
            post_parse_usage_error(&cli).is_none()
                && match &cli.command {
                    // Info deliberately validates projection semantics after
                    // parsing. Consult that owner before advertising a retry.
                    CommandKind::Info(opts) => opts.validate_projection().is_ok(),
                    _ => true,
                }
        }
        Err(error) => error.kind() == ErrorKind::DisplayHelp,
    };
    if valid {
        let command = args
            .iter()
            .map(|arg| crate::shell::quote(arg))
            .collect::<Vec<_>>()
            .join(" ");
        format!("Suggested retry (not executed):\n  {command}")
    } else {
        format!("More arguments need attention; see:\n  {help}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recovery(args: &[&str]) -> Option<String> {
        let args = args.iter().map(OsString::from).collect::<Vec<_>>();
        hint(&args, &Cli::try_parse_from(&args).unwrap_err())
    }

    #[test]
    fn recovery_preserves_public_arguments_and_omits_private_launcher_handoff() {
        let hint = recovery(&[
            "jig",
            "--__launcher-contract-version",
            "8",
            "--__launcher-profile",
            "runtime",
            "--__launcher-repo-root",
            "/tmp/Example Project",
            "--json",
            "work",
            "start",
            "--title",
            "Example 'quoted' title",
            "--description=Some notes",
        ])
        .unwrap();
        assert!(hint.contains("--body=Some notes"), "{hint}");
        assert!(hint.contains("--json work start"), "{hint}");
        assert!(!hint.contains("launcher"));
        assert!(
            hint.contains("'/tmp/Example Project/scripts/jig'"),
            "{hint}"
        );
        assert!(hint.contains("'Example '\\''quoted'\\'' title'"), "{hint}");
    }

    #[test]
    fn recovery_uses_help_when_correction_is_not_parseable() {
        for args in [
            vec!["jig", "work", "start", "--description", "notes"],
            vec![
                "jig",
                "work",
                "start",
                "--title",
                "Example",
                "--body",
                "one",
                "--description",
                "two",
            ],
            vec![
                "jig",
                "work",
                "gates",
                "--summary",
                "--projection",
                "standard",
            ],
            vec!["jig", "work", "status", "--summary"],
        ] {
            let hint = recovery(&args).unwrap();
            assert!(hint.ends_with("--help"), "{hint}");
            assert!(!hint.contains("Suggested retry"), "{hint}");
        }
    }

    #[test]
    fn recovery_does_not_reinterpret_unrelated_commands_or_bad_values() {
        assert!(recovery(&["jig", "work", "append", "--description", "notes"]).is_none());
        assert!(recovery(&["jig", "work", "contract"]).is_none());
        let hint = recovery(&["jig", "work", "gates", "--projection", "--summary"]).unwrap();
        assert!(hint.ends_with("jig work gates --help"), "{hint}");
        assert!(!hint.contains("Suggested retry"));
    }
}
