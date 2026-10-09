//! Commands that change variables, dispatch, or execution in the current shell.

use super::assignments::{bash_assignment_name, path_variable_name, shell_word_is_assignment};
use super::command_name::shell_command_name;
use super::syntax::{ShellCommandName, ShellWord, shell_word_value};

pub(in crate::doctor) fn shell_command_may_persist_path_change(words: &[ShellWord]) -> bool {
    shell_command_may_persist_variable_change(words, &path_variable_name)
}

pub(in crate::doctor) fn shell_command_may_change_dispatch_or_inject_execution(
    words: &[ShellWord],
) -> bool {
    let ShellCommandName::Executable {
        index,
        force_external: false,
        ..
    } = shell_command_name(words)
    else {
        return false;
    };
    let command = shell_word_value(&words[index]);
    let arguments = &words[index + 1..];
    if arguments
        .iter()
        .any(|word| word.active_dollar || word.dynamic)
    {
        return matches!(command.as_str(), "hash" | "enable" | "trap");
    }
    match command.as_str() {
        "hash" => hash_command_may_change_dispatch(arguments),
        "enable" => enable_command_may_change_dispatch(arguments),
        "trap" => trap_command_may_inject_execution(arguments),
        _ => false,
    }
}

fn hash_command_may_change_dispatch(arguments: &[ShellWord]) -> bool {
    let mut query_or_delete_only = false;
    for argument in arguments {
        let value = shell_word_value(argument);
        if value == "--" {
            query_or_delete_only = false;
            continue;
        }
        if let Some(options) = value
            .strip_prefix('-')
            .filter(|options| !options.is_empty())
        {
            if options.contains('p') {
                return true;
            }
            if !options
                .chars()
                .all(|option| matches!(option, 'd' | 'l' | 'r' | 't'))
            {
                return true;
            }
            query_or_delete_only = options
                .chars()
                .any(|option| matches!(option, 'd' | 'l' | 't'));
            continue;
        }
        if !query_or_delete_only {
            return true;
        }
    }
    false
}

fn enable_command_may_change_dispatch(arguments: &[ShellWord]) -> bool {
    for argument in arguments {
        let value = shell_word_value(argument);
        if value == "--" {
            continue;
        }
        if let Some(options) = value
            .strip_prefix('-')
            .filter(|options| !options.is_empty())
        {
            if options
                .chars()
                .any(|option| matches!(option, 'd' | 'f' | 'n'))
            {
                return true;
            }
            if !options
                .chars()
                .all(|option| matches!(option, 'a' | 'p' | 's'))
            {
                return true;
            }
            continue;
        }
        return true;
    }
    false
}

fn trap_command_may_inject_execution(arguments: &[ShellWord]) -> bool {
    let Some(first) = arguments.first().map(shell_word_value) else {
        return false;
    };
    !matches!(first.as_str(), "-l" | "-p")
}

pub(in crate::doctor) fn shell_command_may_persist_variable_change(
    words: &[ShellWord],
    variable_matches: &dyn Fn(&str) -> bool,
) -> bool {
    let mut leading_assignment_end = 0;
    while words
        .get(leading_assignment_end)
        .is_some_and(shell_word_is_assignment)
    {
        leading_assignment_end += 1;
    }
    let leading_variable_assignment = words[..leading_assignment_end]
        .iter()
        .any(|word| bash_assignment_name(&shell_word_value(word)).is_some_and(variable_matches));
    if leading_assignment_end == words.len() {
        return leading_variable_assignment;
    }

    let index = match shell_command_name(words) {
        ShellCommandName::Executable {
            index,
            force_external: false,
            ..
        } => index,
        ShellCommandName::Executable {
            force_external: true,
            ..
        }
        | ShellCommandName::NoExternalExecutable { .. } => return false,
        ShellCommandName::AmbiguousWrapper { .. } => return true,
    };
    let command = shell_word_value(&words[index]);
    if matches!(command.as_str(), "." | "source" | "eval") {
        return true;
    }
    if matches!(
        command.as_str(),
        "declare" | "export" | "local" | "readonly" | "typeset" | "unset"
    ) {
        return leading_variable_assignment
            || declaration_uses_nameref(words, index + 1)
            || words
                .iter()
                .skip(index + 1)
                .any(|word| shell_word_names_variable(word, variable_matches));
    }
    if command == "read" {
        return leading_variable_assignment
            || read_mutates_variable(words, index + 1, variable_matches);
    }
    if command == "printf" {
        return leading_variable_assignment
            || printf_mutates_variable(words, index + 1, variable_matches);
    }
    if matches!(command.as_str(), "mapfile" | "readarray") {
        return leading_variable_assignment
            || mapfile_mutates_variable(words, index + 1, variable_matches);
    }
    if command == "getopts" {
        return leading_variable_assignment
            || getopts_mutates_variable(words, index + 1, variable_matches);
    }
    if command == "let" {
        // Arithmetic expressions can assign through array subscripts and
        // namerefs. Reimplementing Bash arithmetic expansion here would risk
        // a false-negative driver or PATH result, so treat it conservatively.
        return true;
    }
    false
}

fn declaration_uses_nameref(words: &[ShellWord], mut index: usize) -> bool {
    while let Some(word) = words.get(index).map(shell_word_value) {
        if word == "--" {
            return false;
        }
        let Some(options) = word
            .strip_prefix('-')
            .or_else(|| word.strip_prefix('+'))
            .filter(|options| !options.is_empty())
        else {
            return false;
        };
        if options.contains('n') {
            return true;
        }
        index += 1;
    }
    false
}

fn shell_word_names_variable(word: &ShellWord, variable_matches: &dyn Fn(&str) -> bool) -> bool {
    if word.active_dollar || word.dynamic {
        return true;
    }
    let word = shell_word_value(word);
    shell_variable_base_name(&word).is_some_and(variable_matches)
}

fn shell_variable_base_name(value: &str) -> Option<&str> {
    let candidate = value.split_once('=').map_or(value, |(name, _)| name);
    bash_assignment_base_name(candidate)
}

pub(in crate::doctor) fn bash_assignment_base_name(candidate: &str) -> Option<&str> {
    let candidate = candidate.strip_suffix('+').unwrap_or(candidate);
    let candidate = candidate
        .split_once('[')
        .map_or(candidate, |(name, _)| name);
    let mut chars = candidate.chars();
    let first = chars.next()?;
    ((first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric()))
    .then_some(candidate)
}

fn read_mutates_variable(
    words: &[ShellWord],
    mut index: usize,
    variable_matches: &dyn Fn(&str) -> bool,
) -> bool {
    let mut options_allowed = true;
    while let Some(word) = words.get(index) {
        if word.active_dollar || word.dynamic {
            return true;
        }
        let value = shell_word_value(word);
        if options_allowed && value == "--" {
            options_allowed = false;
            index += 1;
            continue;
        }
        if options_allowed
            && let Some(options) = value.strip_prefix('-').filter(|value| !value.is_empty())
        {
            let chars = options.char_indices();
            let mut consumed_next = false;
            for (offset, option) in chars {
                match option {
                    'e' | 'r' | 's' => {}
                    'a' | 'd' | 'i' | 'n' | 'N' | 'p' | 't' | 'u' => {
                        let value_offset = offset + option.len_utf8();
                        let attached = &options[value_offset..];
                        let argument = if attached.is_empty() {
                            let Some(argument) = words.get(index + 1) else {
                                return true;
                            };
                            consumed_next = true;
                            argument
                        } else {
                            word
                        };
                        if option == 'a' {
                            if attached.is_empty() {
                                if shell_word_names_variable(argument, variable_matches) {
                                    return true;
                                }
                            } else if bash_assignment_base_name(attached)
                                .is_some_and(variable_matches)
                            {
                                return true;
                            }
                        }
                        break;
                    }
                    _ => return true,
                }
            }
            index += usize::from(consumed_next) + 1;
            continue;
        }
        options_allowed = false;
        if shell_word_names_variable(word, variable_matches) {
            return true;
        }
        index += 1;
    }
    false
}

fn printf_mutates_variable(
    words: &[ShellWord],
    mut index: usize,
    variable_matches: &dyn Fn(&str) -> bool,
) -> bool {
    while let Some(word) = words.get(index) {
        if word.active_dollar || word.dynamic {
            return true;
        }
        let value = shell_word_value(word);
        if value == "--" {
            return false;
        }
        if value == "-v" {
            let Some(variable) = words.get(index + 1) else {
                return true;
            };
            return shell_word_names_variable(variable, variable_matches);
        }
        if let Some(variable) = value.strip_prefix("-v").filter(|value| !value.is_empty()) {
            return bash_assignment_base_name(variable).is_some_and(variable_matches);
        }
        if value.starts_with('-') {
            index += 1;
            continue;
        }
        return false;
    }
    false
}

fn mapfile_mutates_variable(
    words: &[ShellWord],
    mut index: usize,
    variable_matches: &dyn Fn(&str) -> bool,
) -> bool {
    let mut options_allowed = true;
    while let Some(word) = words.get(index) {
        if word.active_dollar || word.dynamic {
            return true;
        }
        let value = shell_word_value(word);
        if options_allowed && value == "--" {
            options_allowed = false;
            index += 1;
            continue;
        }
        if options_allowed
            && let Some(options) = value.strip_prefix('-').filter(|value| !value.is_empty())
        {
            let chars = options.char_indices();
            let mut consumed_next = false;
            for (offset, option) in chars {
                match option {
                    't' => {}
                    'C' => {
                        // The callback is evaluated as Bash code and can
                        // mutate arbitrary variables through namerefs.
                        return true;
                    }
                    'c' | 'd' | 'n' | 'O' | 's' | 'u' => {
                        let value_offset = offset + option.len_utf8();
                        if options[value_offset..].is_empty() {
                            if words.get(index + 1).is_none() {
                                return true;
                            }
                            consumed_next = true;
                        }
                        break;
                    }
                    _ => return true,
                }
            }
            index += usize::from(consumed_next) + 1;
            continue;
        }
        return shell_word_names_variable(word, variable_matches);
    }
    false
}

fn getopts_mutates_variable(
    words: &[ShellWord],
    index: usize,
    variable_matches: &dyn Fn(&str) -> bool,
) -> bool {
    let Some(optstring) = words.get(index) else {
        return false;
    };
    if optstring.active_dollar || optstring.dynamic {
        return true;
    }
    let Some(name) = words.get(index + 1) else {
        return false;
    };
    shell_word_names_variable(name, variable_matches)
}
