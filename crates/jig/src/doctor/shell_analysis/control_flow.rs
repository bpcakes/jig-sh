//! Directory changes and exit guards between commands.

use std::fs;
use std::path::{Path, PathBuf};

use super::command_name::shell_command_name;
use super::syntax::{ShellCommandName, ShellWord, shell_word_value};

pub(in crate::doctor) fn shell_command_changes_directory(words: &[ShellWord]) -> bool {
    let ShellCommandName::Executable {
        index,
        force_external: false,
        ..
    } = shell_command_name(words)
    else {
        return false;
    };
    matches!(
        shell_word_value(&words[index]).as_str(),
        "cd" | "pushd" | "popd"
    )
}

pub(in crate::doctor) fn resolve_literal_cd(
    root: &Path,
    cwd: &Path,
    words: &[ShellWord],
) -> Option<PathBuf> {
    if shell_word_value(words.first()?) != "cd" {
        return None;
    }
    let path_word = match words {
        [_, path] => path,
        [_, option, path] if shell_word_value(option) == "--" => path,
        _ => return None,
    };
    let value = shell_word_value(path_word);
    if value.is_empty()
        || value.starts_with('~')
        || path_word.active_dollar
        || path_word.literal_dollar
        || path_word.dynamic
        || value.chars().any(|ch| matches!(ch, '*' | '?' | '['))
    {
        return None;
    }
    let candidate = PathBuf::from(value);
    let candidate = if candidate.is_absolute() {
        candidate
    } else {
        cwd.join(candidate)
    };
    let root = fs::canonicalize(root).ok()?;
    let candidate = fs::canonicalize(candidate).ok()?;
    (candidate.is_dir() && candidate.starts_with(root)).then_some(candidate)
}

pub(in crate::doctor) fn literal_exit_guard(words: &[ShellWord]) -> bool {
    match words {
        [command] => shell_word_value(command) == "exit",
        [command, status] => {
            shell_word_value(command) == "exit"
                && !status.active_dollar
                && !status.literal_dollar
                && !status.dynamic
                && !status.value.is_empty()
                && status.value.chars().all(|ch| ch.is_ascii_digit())
        }
        _ => false,
    }
}
