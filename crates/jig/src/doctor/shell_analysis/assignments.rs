//! Variable assignment words such as `PATH=...`.

use std::env;
use std::ffi::OsString;

use super::mutations::bash_assignment_base_name;
use super::syntax::ShellWord;

pub(super) fn path_variable_name(name: &str) -> bool {
    name == "PATH"
}

pub(super) fn env_assignment_name(value: &str) -> Option<&str> {
    let (name, _) = value.split_once('=')?;
    (!name.is_empty()).then_some(name)
}

pub(in crate::doctor) fn shell_assignment_name(value: &str) -> Option<&str> {
    let (name, _) = value.split_once('=')?;
    let name = name.strip_suffix('+').unwrap_or(name);
    let mut chars = name.chars();
    let first = chars.next()?;
    ((first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric()))
    .then_some(name)
}

pub(in crate::doctor) fn bash_assignment_name(value: &str) -> Option<&str> {
    let (name, _) = value.split_once('=')?;
    bash_assignment_base_name(name)
}

pub(in crate::doctor) fn shell_word_is_assignment(word: &ShellWord) -> bool {
    word.assignment_name_plain && bash_assignment_name(&word.value).is_some()
}

pub(super) fn shell_path_assignment_is_literal(word: &ShellWord) -> bool {
    let Some((name, _)) = word.value.split_once('=') else {
        return false;
    };
    !name.ends_with('+')
        && !name.contains('[')
        && bash_assignment_base_name(name).is_some_and(path_variable_name)
        && path_assignment_is_literal(word)
}

pub(super) fn path_assignment_is_literal(word: &ShellWord) -> bool {
    !word.active_dollar
        && !word.dynamic
        && path_assignment_value(word).is_some_and(|value| {
            env::split_paths(&value)
                .all(|entry| !entry.to_str().is_some_and(|entry| entry.starts_with('~')))
        })
}

pub(in crate::doctor) fn path_assignment_value(word: &ShellWord) -> Option<OsString> {
    word.value
        .split_once('=')
        .map(|(_, value)| OsString::from(value))
}

pub(super) fn looks_like_shell_assignment(value: &str) -> bool {
    bash_assignment_name(value).is_some()
}
