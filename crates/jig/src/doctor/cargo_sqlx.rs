//! Whether a configured `cargo sqlx` command really dispatches to the SQLx CLI.

use std::fs;
use std::path::{Path, PathBuf};

use super::environment::DoctorEnvironment;
use super::shell_analysis::{
    ShellCommandName, ShellSeparator, ShellWord, bash_assignment_name, command_program_index,
    exec_wrapper_clears_environment, executable_is_named, parse_shell_commands, resolve_literal_cd,
    shell_assignment_name, shell_command_changes_directory,
    shell_command_may_persist_variable_change, shell_command_name, shell_word_is_assignment,
    shell_word_value,
};

pub(super) fn command_uses_cargo_sqlx(command: &str) -> bool {
    parse_shell_commands(command).commands.iter().any(|words| {
        let Some(index) = command_program_index(words) else {
            return false;
        };
        executable_is_named(&shell_word_value(&words[index]), "cargo")
            && cargo_subcommand(words, index + 1).as_deref() == Some("sqlx")
    })
}

pub(super) fn cargo_subcommand(words: &[ShellWord], index: usize) -> Option<String> {
    let index = cargo_subcommand_index(words, index)?;
    Some(shell_word_value(&words[index]))
}

pub(super) fn cargo_subcommand_index(words: &[ShellWord], mut index: usize) -> Option<usize> {
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

pub(super) fn cargo_sqlx_dispatch_issue(
    root: &Path,
    command: &str,
    environment: &DoctorEnvironment,
) -> &'static str {
    if environment.cargo_alias_sqlx.is_some() {
        return "CARGO_ALIAS_SQLX is set";
    }
    if cargo_sqlx_command_changes_dispatch_environment(command) {
        return "the command changes Cargo alias or home environment";
    }
    if cargo_sqlx_command_has_inline_config(command) {
        return "the cargo command has an inline --config override";
    }
    if effective_cargo_config_obscures_sqlx_alias(root, command, environment) {
        return "an effective Cargo config may change subcommand dispatch";
    }
    "an external cargo path does not prove which sqlx subcommand will run"
}

pub(super) fn cargo_sqlx_command_changes_dispatch_environment(command: &str) -> bool {
    parse_shell_commands(command)
        .commands
        .iter()
        .any(|words| shell_command_changes_cargo_dispatch_environment(words))
}

fn shell_command_changes_cargo_dispatch_environment(words: &[ShellWord]) -> bool {
    if shell_command_may_persist_variable_change(words, &cargo_dispatch_environment_name) {
        return true;
    }
    let command_name = shell_command_name(words);
    let (program_index, force_external) = match command_name {
        ShellCommandName::Executable {
            index,
            force_external,
            ..
        } => (Some(index), force_external),
        ShellCommandName::NoExternalExecutable { .. }
        | ShellCommandName::AmbiguousWrapper { .. } => (None, false),
    };
    let mut leading_assignment_end = 0;
    while words
        .get(leading_assignment_end)
        .is_some_and(shell_word_is_assignment)
    {
        if bash_assignment_name(&shell_word_value(&words[leading_assignment_end]))
            .is_some_and(cargo_dispatch_environment_name)
        {
            return true;
        }
        leading_assignment_end += 1;
    }
    let prefix_end = program_index.unwrap_or(words.len());
    if words[..prefix_end].iter().any(|word| {
        shell_assignment_name(&shell_word_value(word)).is_some_and(cargo_dispatch_environment_name)
    }) {
        return true;
    }
    if program_index.is_none()
        && words.iter().any(|word| {
            shell_assignment_name(&shell_word_value(word))
                .is_some_and(cargo_dispatch_environment_name)
        })
    {
        return true;
    }

    let Some(program_index) = program_index else {
        return false;
    };
    let program = shell_word_value(&words[program_index]);
    if !force_external
        && matches!(
            program.as_str(),
            "export" | "local" | "readonly" | "typeset"
        )
        && words[program_index + 1..].iter().any(|word| {
            let word = shell_word_value(word);
            cargo_dispatch_environment_name(shell_assignment_name(&word).unwrap_or(word.as_str()))
        })
    {
        return true;
    }
    if !force_external
        && program == "unset"
        && words[program_index + 1..]
            .iter()
            .map(shell_word_value)
            .any(|name| cargo_dispatch_environment_name(&name))
    {
        return true;
    }

    let mut saw_env = false;
    let mut index = 0;
    while index < program_index {
        let word = shell_word_value(&words[index]);
        if word == "exec" && exec_wrapper_clears_environment(words, index + 1, program_index) {
            return true;
        }
        if executable_is_named(&word, "env") {
            saw_env = true;
        } else if saw_env {
            if matches!(word.as_str(), "-" | "-i" | "--ignore-environment") {
                return true;
            }
            if shell_assignment_name(&word).is_some_and(cargo_dispatch_environment_name) {
                return true;
            }
            if word == "-u" || word == "--unset" {
                index += 1;
                if words
                    .get(index)
                    .map(shell_word_value)
                    .as_deref()
                    .is_some_and(cargo_dispatch_environment_name)
                {
                    return true;
                }
            } else if word
                .strip_prefix("--unset=")
                .or_else(|| word.strip_prefix("-u"))
                .is_some_and(cargo_dispatch_environment_name)
            {
                return true;
            }
        }
        index += 1;
    }
    false
}

fn cargo_dispatch_environment_name(name: &str) -> bool {
    const NAMES: [&str; 3] = ["CARGO_ALIAS_SQLX", "CARGO_HOME", "HOME"];
    NAMES.contains(&name)
}

pub(super) fn cargo_sqlx_command_has_inline_config(command: &str) -> bool {
    parse_shell_commands(command).commands.iter().any(|words| {
        let Some(program_index) = command_program_index(words) else {
            return false;
        };
        if !executable_is_named(&shell_word_value(&words[program_index]), "cargo") {
            return false;
        }
        let Some(sqlx_index) = cargo_subcommand_index(words, program_index + 1) else {
            return false;
        };
        if shell_word_value(&words[sqlx_index]) != "sqlx" {
            return false;
        }
        let mut index = program_index + 1;
        while index < sqlx_index {
            let value = shell_word_value(&words[index]);
            let config = if value == "--config" {
                index += 1;
                words.get(index).cloned()
            } else {
                value
                    .strip_prefix("--config=")
                    .map(|value| words[index].with_value(value))
            };
            if config
                .as_ref()
                .is_some_and(cargo_config_override_obscures_sqlx_alias)
            {
                return true;
            }
            index += 1;
        }
        false
    })
}

fn cargo_config_override_obscures_sqlx_alias(config: &ShellWord) -> bool {
    if config.active_dollar || config.literal_dollar || config.dynamic {
        return true;
    }
    let text = config.value.trim();
    if !text.contains('=') {
        return true;
    }
    let Ok(config) = text.parse::<toml::Table>() else {
        return true;
    };
    if config.contains_key("include") {
        return true;
    }
    match config.get("alias") {
        None => false,
        Some(toml::Value::Table(aliases)) => aliases.contains_key("sqlx"),
        Some(_) => true,
    }
}

fn effective_cargo_config_obscures_sqlx_alias(
    root: &Path,
    command: &str,
    environment: &DoctorEnvironment,
) -> bool {
    let root = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let Some(invocation_directories) = cargo_sqlx_invocation_directories(&root, command) else {
        return true;
    };
    for invocation_directory in invocation_directories {
        for directory in invocation_directory.ancestors() {
            if cargo_config_directory_obscures_sqlx_alias(&directory.join(".cargo")) {
                return true;
            }
        }
    }

    let cargo_home = if let Some(cargo_home) = environment.cargo_home.as_deref() {
        let cargo_home = PathBuf::from(cargo_home);
        if !cargo_home.is_absolute() {
            return true;
        }
        Some(cargo_home)
    } else if let Some(home) = environment.home.as_deref() {
        let home = PathBuf::from(home);
        if !home.is_absolute() {
            return true;
        }
        Some(home.join(".cargo"))
    } else {
        None
    };
    cargo_home
        .as_deref()
        .is_some_and(cargo_config_directory_obscures_sqlx_alias)
}

fn cargo_sqlx_invocation_directories(root: &Path, command: &str) -> Option<Vec<PathBuf>> {
    let parsed = parse_shell_commands(command);
    if parsed.ambiguous {
        return None;
    }
    let mut cwd = root.to_path_buf();
    let mut directories = Vec::new();
    for (command_index, words) in parsed.commands.iter().enumerate() {
        let incoming = command_index
            .checked_sub(1)
            .and_then(|index| parsed.separators.get(index))
            .copied();
        let outgoing = parsed.separators.get(command_index).copied();
        if shell_command_changes_directory(words) {
            if !matches!(incoming, None | Some(ShellSeparator::Sequence))
                || outgoing != Some(ShellSeparator::And)
            {
                return None;
            }
            cwd = resolve_literal_cd(root, &cwd, words)?;
            continue;
        }
        let Some(program_index) = command_program_index(words) else {
            continue;
        };
        if executable_is_named(&shell_word_value(&words[program_index]), "cargo")
            && cargo_subcommand(words, program_index + 1).as_deref() == Some("sqlx")
        {
            directories.push(cwd.clone());
        }
    }
    (!directories.is_empty()).then_some(directories)
}

fn cargo_config_directory_obscures_sqlx_alias(directory: &Path) -> bool {
    [directory.join("config.toml"), directory.join("config")]
        .into_iter()
        .any(|path| cargo_config_obscures_sqlx_alias(&path))
}

fn cargo_config_obscures_sqlx_alias(path: &Path) -> bool {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return false,
        Err(_) => return true,
    };
    let Ok(config) = text.parse::<toml::Value>() else {
        return true;
    };
    if config.get("include").is_some() {
        return true;
    }
    match config.get("alias") {
        None => false,
        Some(toml::Value::Table(aliases)) => aliases.contains_key("sqlx"),
        Some(_) => true,
    }
}

pub(super) fn path_has_symlink_or_reparse_component(path: &Path) -> Option<bool> {
    use std::path::Component;

    if !path.is_absolute() {
        return None;
    }
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => {
                current.push(component.as_os_str());
            }
            Component::CurDir => {}
            // Parent components make the raw lookup identity harder to
            // reason about. Bare PATH entries do not need them.
            Component::ParentDir => return None,
            Component::Normal(_) => {
                current.push(component.as_os_str());
                let metadata = fs::symlink_metadata(&current).ok()?;
                if metadata_is_symlink_or_reparse_point(&metadata) {
                    return Some(true);
                }
            }
        }
    }
    Some(false)
}

fn metadata_is_symlink_or_reparse_point(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}
