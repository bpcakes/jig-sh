fn shell_command_name(words: &[ShellWord]) -> ShellCommandName {
    let mut index = 0;
    let mut ambiguous_wrapper = false;
    let mut allow_shell_assignments = true;
    let mut allow_prefix_keyword = true;
    let mut force_external = false;
    let mut require_builtin = false;
    let mut changes_cwd = false;
    let mut path_lookup = ShellPathLookup::Captured;
    let mut taint_immediate_external_lookup = false;
    let mut taint_after_immediate_external_wrapper = false;
    let mut allow_keyword = true;
    let mut external_wrappers = Vec::new();
    while let Some(word) = words.get(index) {
        let word = shell_word_value(word);
        if word.is_empty() {
            // An empty quoted command name is a real shell word. Skipping it
            // could misidentify a later `cargo sqlx` argument as the program.
            return ShellCommandName::AmbiguousWrapper {
                external_wrappers,
                changes_cwd,
            };
        }
        let prefix_keyword = allow_shell_assignments
            && shell_word_is_prefix_keyword(&words[index], allow_prefix_keyword);
        let shell_assignment = allow_shell_assignments && shell_word_is_assignment(&words[index]);
        if prefix_keyword || shell_assignment {
            if shell_assignment {
                // Bash recognizes reserved prefix words before command
                // assignments, not after them. Once an assignment word starts
                // the simple command, a later reserved word is the command
                // name or a syntax error; it cannot expose a later executable.
                allow_prefix_keyword = false;
                if bash_assignment_name(&word).is_some_and(path_variable_name) {
                    path_lookup = if shell_path_assignment_is_literal(&words[index]) {
                        ShellPathLookup::CommandLocal(index)
                    } else {
                        ShellPathLookup::Unverifiable
                    };
                }
            }
            index += 1;
            continue;
        }
        if words[index].active_dollar || words[index].dynamic {
            return ShellCommandName::AmbiguousWrapper {
                external_wrappers,
                changes_cwd,
            };
        }
        if require_builtin && !bash_builtin(&word) {
            // `builtin` never falls back to PATH lookup. An unknown literal
            // target fails inside Bash without executing an external command.
            return ShellCommandName::NoExternalExecutable {
                external_wrappers,
                changes_cwd,
            };
        }
        if !allow_shell_assignments && looks_like_shell_assignment(&word) {
            // Assignment words are only recognized before the shell command
            // name. `command`, `exec`, and `nohup` treat an assignment-looking
            // target as the executable name; do not skip across it and expose
            // a later argument as a tool.
            return ShellCommandName::AmbiguousWrapper {
                external_wrappers,
                changes_cwd,
            };
        }
        let shell_builtin_dispatch = !force_external && bash_builtin(&word);
        let shell_keyword_dispatch =
            !force_external && allow_keyword && shell_word_is_keyword(&words[index]);
        let wrapper = shell_wrapper(
            words,
            index,
            &word,
            shell_builtin_dispatch,
            shell_keyword_dispatch,
        );
        if let Some((wrapper_kind, wrapper_target)) = wrapper {
            if wrapper_kind.is_external() {
                external_wrappers.push(ExternalWrapperReference {
                    index,
                    path_lookup: if taint_immediate_external_lookup {
                        ShellPathLookup::Unverifiable
                    } else {
                        path_lookup
                    },
                    changes_cwd,
                });
                taint_immediate_external_lookup = false;
                if taint_after_immediate_external_wrapper {
                    path_lookup = ShellPathLookup::Unverifiable;
                    taint_after_immediate_external_wrapper = false;
                }
            }
            match wrapper_target {
                WrapperTarget::Executable {
                    index: target,
                    ambiguous,
                } => {
                    if wrapper_kind == ShellWrapperKind::Exec
                        && exec_wrapper_clears_environment(words, index + 1, target)
                    {
                        taint_after_immediate_external_wrapper = true;
                    }
                    taint_immediate_external_lookup |= ambiguous;
                    index = target;
                    ambiguous_wrapper |= ambiguous;
                    allow_shell_assignments = false;
                    match wrapper_kind {
                        ShellWrapperKind::Builtin => {
                            force_external = false;
                            require_builtin = true;
                            allow_keyword = false;
                        }
                        ShellWrapperKind::Command => {
                            force_external = false;
                            require_builtin = false;
                            allow_keyword = false;
                        }
                        ShellWrapperKind::Exec
                        | ShellWrapperKind::Nohup
                        | ShellWrapperKind::Time => {
                            force_external = true;
                            require_builtin = false;
                            allow_keyword = false;
                        }
                    }
                    continue;
                }
                WrapperTarget::NoExternalExecutable => {
                    return ShellCommandName::NoExternalExecutable {
                        external_wrappers,
                        changes_cwd,
                    };
                }
                WrapperTarget::Ambiguous => {
                    return ShellCommandName::AmbiguousWrapper {
                        external_wrappers,
                        changes_cwd,
                    };
                }
            }
        }
        if executable_is_named(&word, "env") && (!require_builtin || force_external) {
            external_wrappers.push(ExternalWrapperReference {
                index,
                path_lookup: if taint_immediate_external_lookup {
                    ShellPathLookup::Unverifiable
                } else {
                    path_lookup
                },
                changes_cwd,
            });
            taint_immediate_external_lookup = false;
            if taint_after_immediate_external_wrapper {
                path_lookup = ShellPathLookup::Unverifiable;
                taint_after_immediate_external_wrapper = false;
            }
            let env = parse_env_wrapper(words, index + 1);
            changes_cwd |= env.changes_directory;
            path_lookup = env_wrapper_path_lookup(words, &env, path_lookup);
            match env.target_index {
                Some(target) => {
                    index = target;
                    ambiguous_wrapper |= env.ambiguous;
                    allow_shell_assignments = false;
                    force_external = true;
                    require_builtin = false;
                    allow_keyword = false;
                    continue;
                }
                None if env.ambiguous => {
                    return ShellCommandName::AmbiguousWrapper {
                        external_wrappers,
                        changes_cwd,
                    };
                }
                None => {
                    return ShellCommandName::NoExternalExecutable {
                        external_wrappers,
                        changes_cwd,
                    };
                }
            }
        }
        return ShellCommandName::Executable {
            index,
            ambiguous_wrapper,
            force_external,
            changes_cwd,
            path_lookup: if taint_immediate_external_lookup {
                ShellPathLookup::Unverifiable
            } else {
                path_lookup
            },
            allow_keyword,
            external_wrappers,
        };
    }
    ShellCommandName::NoExternalExecutable {
        external_wrappers,
        changes_cwd,
    }
}

fn shell_wrapper(
    words: &[ShellWord],
    index: usize,
    word: &str,
    shell_builtin_dispatch: bool,
    shell_keyword_dispatch: bool,
) -> Option<(ShellWrapperKind, WrapperTarget)> {
    if shell_builtin_dispatch {
        return match word {
            "builtin" => Some((
                ShellWrapperKind::Builtin,
                builtin_wrapper_target(words, index + 1),
            )),
            "command" => Some((
                ShellWrapperKind::Command,
                command_wrapper_target(words, index + 1),
            )),
            "exec" => Some((
                ShellWrapperKind::Exec,
                exec_wrapper_target(words, index + 1),
            )),
            _ => None,
        };
    }
    if executable_is_named(word, "nohup") {
        return Some((
            ShellWrapperKind::Nohup,
            nohup_wrapper_target(words, index + 1),
        ));
    }
    (!shell_keyword_dispatch && executable_is_named(word, "time")).then(|| {
        (
            ShellWrapperKind::Time,
            time_wrapper_target(words, index + 1),
        )
    })
}

fn shell_command_has_ambiguous_wrapper(words: &[ShellWord]) -> bool {
    matches!(
        shell_command_name(words),
        ShellCommandName::Executable {
            ambiguous_wrapper: true,
            ..
        } | ShellCommandName::AmbiguousWrapper { .. }
    )
}

fn builtin_wrapper_target(words: &[ShellWord], mut index: usize) -> WrapperTarget {
    if words.get(index).map(shell_word_value).as_deref() == Some("--") {
        index += 1;
    }
    words
        .get(index)
        .map(|_| WrapperTarget::Executable {
            index,
            ambiguous: false,
        })
        .unwrap_or(WrapperTarget::NoExternalExecutable)
}

fn command_wrapper_target(words: &[ShellWord], mut index: usize) -> WrapperTarget {
    let mut uses_default_path = false;
    while let Some(word) = words.get(index).map(shell_word_value) {
        if word == "--" {
            index += 1;
            break;
        }
        let Some(options) = word.strip_prefix('-').filter(|options| !options.is_empty()) else {
            break;
        };
        if !options
            .chars()
            .all(|option| matches!(option, 'p' | 'v' | 'V'))
        {
            return WrapperTarget::Ambiguous;
        }
        if options.chars().any(|option| matches!(option, 'v' | 'V')) {
            return WrapperTarget::NoExternalExecutable;
        }
        uses_default_path |= options.contains('p');
        index += 1;
    }
    words
        .get(index)
        .map(|_| WrapperTarget::Executable {
            index,
            // `command -p` uses an implementation-defined default search
            // path, not the captured PATH doctor can resolve faithfully.
            ambiguous: uses_default_path,
        })
        .unwrap_or(WrapperTarget::NoExternalExecutable)
}

fn exec_wrapper_target(words: &[ShellWord], mut index: usize) -> WrapperTarget {
    let mut changes_argv_zero = false;
    while let Some(word) = words.get(index).map(shell_word_value) {
        if word == "--" {
            index += 1;
            break;
        }
        if word == "-a" {
            if words.get(index + 1).is_none() {
                return WrapperTarget::Ambiguous;
            }
            changes_argv_zero = true;
            index += 2;
            continue;
        }
        let Some(options) = word.strip_prefix('-').filter(|options| !options.is_empty()) else {
            break;
        };
        if !options.chars().all(|option| matches!(option, 'c' | 'l')) {
            return WrapperTarget::Ambiguous;
        }
        changes_argv_zero |= options.contains('l');
        index += 1;
    }
    words
        .get(index)
        .map(|_| WrapperTarget::Executable {
            index,
            // `-a` and `-l` alter argv[0], which the capability probe does
            // not reproduce portably.
            ambiguous: changes_argv_zero,
        })
        .unwrap_or(WrapperTarget::NoExternalExecutable)
}

fn nohup_wrapper_target(words: &[ShellWord], mut index: usize) -> WrapperTarget {
    if words.get(index).map(shell_word_value).as_deref() == Some("--") {
        index += 1;
        return words
            .get(index)
            .map(|_| WrapperTarget::Executable {
                index,
                ambiguous: false,
            })
            .unwrap_or(WrapperTarget::NoExternalExecutable);
    }
    match words.get(index).map(shell_word_value) {
        Some(word) if matches!(word.as_str(), "--help" | "--version") => {
            WrapperTarget::NoExternalExecutable
        }
        Some(word) if word.starts_with('-') => WrapperTarget::Ambiguous,
        Some(_) => WrapperTarget::Executable {
            index,
            ambiguous: false,
        },
        None => WrapperTarget::NoExternalExecutable,
    }
}

fn time_wrapper_target(words: &[ShellWord], mut index: usize) -> WrapperTarget {
    while let Some(word) = words.get(index).map(shell_word_value) {
        if word == "--" {
            index += 1;
            break;
        }
        if matches!(word.as_str(), "--help" | "--version") {
            return WrapperTarget::NoExternalExecutable;
        }
        if word == "-p" {
            index += 1;
            continue;
        }
        if word.starts_with('-') {
            return WrapperTarget::Ambiguous;
        }
        break;
    }
    words
        .get(index)
        .map(|_| WrapperTarget::Executable {
            index,
            ambiguous: false,
        })
        .unwrap_or(WrapperTarget::NoExternalExecutable)
}

fn cargo_subcommand(words: &[ShellWord], index: usize) -> Option<String> {
    let index = cargo_subcommand_index(words, index)?;
    Some(shell_word_value(&words[index]))
}

fn cargo_subcommand_index(words: &[ShellWord], mut index: usize) -> Option<usize> {
    while let Some(word) = words.get(index).map(shell_word_value) {
        if word.starts_with('+') {
            index += 1;
            continue;
        }
        if word.starts_with('-') {
            index += if cargo_global_option_takes_value(&word) {
                2
            } else {
                1
            };
            continue;
        }
        return Some(index);
    }
    None
}

fn cargo_global_option_takes_value(option: &str) -> bool {
    matches!(option, "--color" | "--config" | "-C" | "-Z")
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct EnvWrapperParse {
    target_index: Option<usize>,
    ambiguous: bool,
    clears_environment: bool,
    unset_names: Vec<String>,
    assignment_indices: Vec<usize>,
    changes_directory: bool,
    uses_alternate_path: bool,
    null_output: bool,
}

fn parse_env_wrapper(words: &[ShellWord], mut index: usize) -> EnvWrapperParse {
    let mut parsed = EnvWrapperParse::default();
    let mut options_allowed = true;

    while let Some(shell_word) = words.get(index) {
        let word = shell_word_value(shell_word);
        if options_allowed && (shell_word.active_dollar || shell_word.dynamic) {
            parsed.ambiguous = true;
            return parsed;
        }

        if options_allowed {
            if word == "--" {
                options_allowed = false;
                index += 1;
                continue;
            }
            if word == "-" {
                parsed.clears_environment = true;
                index += 1;
                continue;
            }
            if let Some(long) = word.strip_prefix("--") {
                match long {
                    "ignore-environment" => parsed.clears_environment = true,
                    "unset" => {
                        let Some(name) = words.get(index + 1) else {
                            parsed.ambiguous = true;
                            return parsed;
                        };
                        if name.active_dollar || name.dynamic {
                            parsed.ambiguous = true;
                        }
                        parsed.unset_names.push(shell_word_value(name));
                        index += 2;
                        continue;
                    }
                    "chdir" => {
                        if words.get(index + 1).is_none() {
                            parsed.ambiguous = true;
                            return parsed;
                        }
                        parsed.changes_directory = true;
                        index += 2;
                        continue;
                    }
                    "split-string" => {
                        parsed.ambiguous = true;
                        return parsed;
                    }
                    "argv0" => {
                        if words.get(index + 1).is_none() {
                            parsed.ambiguous = true;
                            return parsed;
                        }
                        parsed.ambiguous = true;
                        index += 2;
                        continue;
                    }
                    "help" | "version" => return parsed,
                    "null" => parsed.null_output = true,
                    "debug" => {}
                    _ if long.starts_with("unset=") => {
                        parsed
                            .unset_names
                            .push(long.trim_start_matches("unset=").to_string());
                    }
                    _ if long.starts_with("chdir=") => parsed.changes_directory = true,
                    _ if long.starts_with("split-string=") => {
                        parsed.ambiguous = true;
                        return parsed;
                    }
                    _ if long.starts_with("argv0=") => parsed.ambiguous = true,
                    _ => {
                        parsed.ambiguous = true;
                        return parsed;
                    }
                }
                index += 1;
                continue;
            }
            if let Some(short) = word.strip_prefix('-').filter(|short| !short.is_empty()) {
                let options = short.char_indices();
                let mut consumed_next = false;
                for (offset, option) in options {
                    match option {
                        '0' | 'i' | 'v' => {
                            if option == '0' {
                                parsed.null_output = true;
                            }
                            if option == 'i' {
                                parsed.clears_environment = true;
                            }
                        }
                        'S' => {
                            // Split-string recursively creates new env options,
                            // assignments, and the utility itself. Its quoting,
                            // escapes, comments, and substitution are deliberately
                            // not reimplemented here.
                            parsed.ambiguous = true;
                            return parsed;
                        }
                        'u' | 'C' | 'P' | 'a' => {
                            let value_offset = offset + option.len_utf8();
                            let attached = &short[value_offset..];
                            let value = if attached.is_empty() {
                                let Some(value) = words.get(index + 1) else {
                                    parsed.ambiguous = true;
                                    return parsed;
                                };
                                consumed_next = true;
                                shell_word_value(value)
                            } else {
                                attached.to_string()
                            };
                            match option {
                                'u' => parsed.unset_names.push(value),
                                'C' => parsed.changes_directory = true,
                                'P' => {
                                    parsed.uses_alternate_path = true;
                                    parsed.ambiguous = true;
                                }
                                'a' => parsed.ambiguous = true,
                                _ => unreachable!(),
                            }
                            break;
                        }
                        _ => {
                            parsed.ambiguous = true;
                            return parsed;
                        }
                    }
                }
                index += usize::from(consumed_next) + 1;
                continue;
            }
        }

        if env_assignment_name(&word).is_some() {
            options_allowed = false;
            parsed.assignment_indices.push(index);
            index += 1;
            continue;
        }
        if shell_word.active_dollar || shell_word.dynamic {
            parsed.ambiguous = true;
            return parsed;
        }
        if parsed.null_output {
            // GNU and BSD `env` reserve null-delimited output for printing the
            // environment; combining it with a utility is an error and never
            // executes that utility.
            parsed.ambiguous = true;
            return parsed;
        }
        parsed.target_index = Some(index);
        return parsed;
    }

    parsed
}

fn env_wrapper_path_lookup(
    words: &[ShellWord],
    env: &EnvWrapperParse,
    incoming: ShellPathLookup,
) -> ShellPathLookup {
    if env.uses_alternate_path {
        return ShellPathLookup::Unverifiable;
    }
    let mut lookup =
        if env.clears_environment || env.unset_names.iter().any(|name| path_variable_name(name)) {
            ShellPathLookup::Unverifiable
        } else {
            incoming
        };
    for index in &env.assignment_indices {
        let word = shell_word_value(&words[*index]);
        if env_assignment_name(&word).is_some_and(path_variable_name) {
            lookup = if path_assignment_is_literal(&words[*index]) {
                ShellPathLookup::CommandLocal(*index)
            } else {
                ShellPathLookup::Unverifiable
            };
        }
    }
    lookup
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SqlxInvocation {
    args_index: usize,
    no_dotenv: bool,
    has_ambiguous_cwd_option: bool,
}

fn sqlx_invocation(words: &[ShellWord], program_index: usize) -> Option<SqlxInvocation> {
    let program = shell_word_value(&words[program_index]);
    let basename = executable_basename(&program)?;
    let args_index = if basename.eq_ignore_ascii_case("cargo") {
        let sqlx_index = cargo_subcommand_index(words, program_index + 1)?;
        (shell_word_value(&words[sqlx_index]) == "sqlx").then_some(sqlx_index + 1)?
    } else if basename.eq_ignore_ascii_case("sqlx") {
        program_index + 1
    } else if basename.eq_ignore_ascii_case("cargo-sqlx") {
        let sqlx_index = program_index + 1;
        (words.get(sqlx_index).map(shell_word_value).as_deref() == Some("sqlx"))
            .then_some(sqlx_index + 1)?
    } else {
        return None;
    };
    let no_dotenv = words[args_index..]
        .iter()
        .take_while(|word| shell_word_value(word) != "--")
        .any(|word| shell_word_value(word) == "--no-dotenv");
    let prefix = &words[..args_index];
    let has_ambiguous_cwd_option = prefix.iter().any(|word| {
        let word = shell_word_value(word);
        matches!(word.as_str(), "-C" | "--chdir")
            || (word.starts_with("-C") && word.len() > 2)
            || word.starts_with("--chdir=")
    });
    Some(SqlxInvocation {
        args_index,
        no_dotenv,
        has_ambiguous_cwd_option,
    })
}

fn executable_basename(program: &str) -> Option<&str> {
    Path::new(program).file_name()?.to_str()
}

fn executable_is_named(program: &str, expected: &str) -> bool {
    executable_basename(program) == Some(expected)
}

fn sqlx_probe_style(program: &str) -> Option<SqlxProbeStyle> {
    let basename = executable_basename(program)?;
    if basename.eq_ignore_ascii_case("cargo-sqlx") {
        Some(SqlxProbeStyle::CargoSubcommand)
    } else if basename.eq_ignore_ascii_case("sqlx") {
        Some(SqlxProbeStyle::Direct)
    } else {
        None
    }
}

fn sqlx_driver_from_flag(
    words: &[ShellWord],
    mut index: usize,
    ambient_database_url: Option<&OsStr>,
) -> Option<SqlxDriverResolution> {
    let mut resolution = None;
    while let Some(word) = words.get(index) {
        let value = shell_word_value(word);
        if value == "--" {
            break;
        }
        let database_url = if matches!(value.as_str(), "--database-url" | "-D") {
            index += 1;
            let Some(value) = words.get(index) else {
                return Some(SqlxDriverResolution::Indeterminate(
                    "--database-url is missing its value",
                ));
            };
            Some(value.clone())
        } else if let Some(value) = value.strip_prefix("--database-url=") {
            Some(word.with_value(value))
        } else if let Some(value) = value.strip_prefix("-D=") {
            Some(word.with_value(value))
        } else if let Some(value) = value.strip_prefix("-D") {
            (!value.is_empty()).then(|| word.with_value(value))
        } else {
            None
        };
        if let Some(database_url) = database_url {
            if resolution.is_some() {
                return Some(SqlxDriverResolution::Indeterminate(
                    "the SQLx command contains multiple --database-url options",
                ));
            }
            resolution = Some(sqlx_driver_from_command_value(
                &database_url,
                SqlxDriverSource::CommandFlag,
                ambient_database_url,
            ));
        }
        index += 1;
    }
    resolution
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum CommandDatabaseUrlScope {
    Inherited,
    Assigned(ShellWord),
    Removed,
    Ambiguous,
}
