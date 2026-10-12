//! Wrapper commands such as `env`, `exec`, `command`, `nohup`, and `time`.

use super::assignments::{env_assignment_name, path_assignment_is_literal, path_variable_name};
use super::syntax::{ShellPathLookup, ShellWord, executable_is_named, shell_word_value};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ShellWrapperKind {
    Builtin,
    Command,
    Exec,
    Nohup,
    Time,
}

impl ShellWrapperKind {
    pub(super) const fn is_external(self) -> bool {
        matches!(self, Self::Nohup | Self::Time)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::doctor) enum WrapperTarget {
    Executable { index: usize, ambiguous: bool },
    NoExternalExecutable,
    Ambiguous,
}

pub(super) fn shell_wrapper(
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

pub(in crate::doctor) fn builtin_wrapper_target(
    words: &[ShellWord],
    mut index: usize,
) -> WrapperTarget {
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

pub(in crate::doctor) fn command_wrapper_target(
    words: &[ShellWord],
    mut index: usize,
) -> WrapperTarget {
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

pub(in crate::doctor) fn exec_wrapper_target(
    words: &[ShellWord],
    mut index: usize,
) -> WrapperTarget {
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

pub(in crate::doctor) fn nohup_wrapper_target(
    words: &[ShellWord],
    mut index: usize,
) -> WrapperTarget {
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

pub(in crate::doctor) fn time_wrapper_target(
    words: &[ShellWord],
    mut index: usize,
) -> WrapperTarget {
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

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::doctor) struct EnvWrapperParse {
    pub(in crate::doctor) target_index: Option<usize>,
    pub(in crate::doctor) ambiguous: bool,
    pub(in crate::doctor) clears_environment: bool,
    pub(in crate::doctor) unset_names: Vec<String>,
    pub(in crate::doctor) assignment_indices: Vec<usize>,
    pub(super) changes_directory: bool,
    uses_alternate_path: bool,
    null_output: bool,
}

pub(in crate::doctor) fn parse_env_wrapper(
    words: &[ShellWord],
    mut index: usize,
) -> EnvWrapperParse {
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

pub(super) fn env_wrapper_path_lookup(
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

pub(in crate::doctor) fn exec_wrapper_clears_environment(
    words: &[ShellWord],
    mut index: usize,
    program_index: usize,
) -> bool {
    while index < program_index {
        let word = shell_word_value(&words[index]);
        if word == "--" {
            return false;
        }
        if word == "-a" {
            index += 2;
            continue;
        }
        let Some(options) = word.strip_prefix('-').filter(|options| !options.is_empty()) else {
            return false;
        };
        if !options.chars().all(|option| matches!(option, 'c' | 'l')) {
            return false;
        }
        if options.contains('c') {
            return true;
        }
        index += 1;
    }
    false
}
