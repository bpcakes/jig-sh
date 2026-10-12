//! Where a command's `DATABASE_URL` comes from: the environment, `.env`, or the command itself.

use std::fs;
use std::path::{Path, PathBuf};

use super::shell_analysis::{
    ShellWord, WrapperTarget, bash_assignment_base_name, bash_builtin, builtin_wrapper_target,
    command_wrapper_target, exec_wrapper_clears_environment, exec_wrapper_target,
    executable_is_named, nohup_wrapper_target, parse_env_wrapper,
    shell_command_may_persist_variable_change, shell_word_is_assignment,
    shell_word_is_prefix_keyword, shell_word_value, time_wrapper_target,
};

#[derive(Debug, Eq, PartialEq)]
pub(super) enum DotenvLookup {
    Missing,
    Present(Option<DotenvDatabaseUrl>),
}

#[derive(Debug, Eq, PartialEq)]
pub(super) enum DotenvDatabaseUrl {
    Literal(String),
    Substitution,
}

pub(super) fn nearest_database_url_from_dotenv(
    cwd: &Path,
    root: &Path,
    name: &str,
) -> std::result::Result<DotenvLookup, ()> {
    for directory in repo_ancestors(cwd, root) {
        let path = directory.join(name);
        match fs::metadata(&path) {
            Ok(metadata) if metadata.is_file() => {
                return database_url_from_dotenv(&path).map(DotenvLookup::Present);
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(()),
        }
    }
    Ok(DotenvLookup::Missing)
}

fn repo_ancestors(cwd: &Path, root: &Path) -> Vec<PathBuf> {
    let root = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let cwd = fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    let mut directories = Vec::new();
    for directory in cwd.ancestors() {
        if !directory.starts_with(&root) {
            break;
        }
        directories.push(directory.to_path_buf());
        if directory == root {
            break;
        }
    }
    directories
}

pub(super) fn dotenv_exists_above_repo(root: &Path, name: &str) -> std::result::Result<bool, ()> {
    let root = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let Some(parent) = root.parent() else {
        return Ok(false);
    };
    for directory in parent.ancestors() {
        match fs::metadata(directory.join(name)) {
            Ok(metadata) if metadata.is_file() => return Ok(true),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(()),
        }
    }
    Ok(false)
}

pub(super) fn database_url_from_dotenv(
    path: &Path,
) -> std::result::Result<Option<DotenvDatabaseUrl>, ()> {
    let bytes = fs::read(path).map_err(|_| ())?;
    let bytes = bytes
        .strip_prefix(&[0xef, 0xbb, 0xbf])
        .unwrap_or(bytes.as_slice());
    let text = std::str::from_utf8(bytes).map_err(|_| ())?;
    if first_raw_database_url_uses_substitution(text) == Some(true) {
        return Ok(Some(DotenvDatabaseUrl::Substitution));
    }
    let variables = dotenvy::from_read_iter(bytes);
    for variable in variables {
        let (key, value) = variable.map_err(|_| ())?;
        if dotenv_database_url_key(&key) {
            return Ok(Some(DotenvDatabaseUrl::Literal(value)));
        }
    }
    Ok(None)
}

fn first_raw_database_url_uses_substitution(text: &str) -> Option<bool> {
    let mut start = 0;
    let mut quote = None;
    let mut escaped = false;
    let mut comment = false;
    for (index, ch) in text.char_indices() {
        if comment {
            if matches!(ch, '\n' | '\r') {
                if let Some(result) = raw_database_url_line_uses_substitution(&text[start..index]) {
                    return Some(result);
                }
                start = index + ch.len_utf8();
                comment = false;
            }
            continue;
        }
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '\'' | '"' if quote == Some(ch) => quote = None,
            '\'' | '"' if quote.is_none() => quote = Some(ch),
            '#' if quote.is_none() => comment = true,
            '\n' | '\r' if quote.is_none() => {
                if let Some(result) = raw_database_url_line_uses_substitution(&text[start..index]) {
                    return Some(result);
                }
                start = index + ch.len_utf8();
            }
            _ => {}
        }
    }
    raw_database_url_line_uses_substitution(&text[start..])
}

fn raw_database_url_line_uses_substitution(line: &str) -> Option<bool> {
    let line = line.trim_start();
    let line = line
        .strip_prefix("export")
        .filter(|rest| rest.starts_with(char::is_whitespace))
        .map(str::trim_start)
        .unwrap_or(line);
    let (key, value) = line.split_once('=')?;
    if !dotenv_database_url_key(key.trim()) {
        return None;
    }
    Some(dotenv_value_uses_substitution(value))
}

fn dotenv_value_uses_substitution(value: &str) -> bool {
    let mut quote = None;
    let mut escaped = false;
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '\'' | '"' if quote == Some(ch) => quote = None,
            '\'' | '"' if quote.is_none() => quote = Some(ch),
            '#' if quote.is_none() => return false,
            '$' if quote != Some('\'')
                && chars.peek().is_some_and(|next| {
                    *next == '{' || *next == '_' || next.is_ascii_alphabetic()
                }) =>
            {
                return true;
            }
            _ => {}
        }
    }
    false
}

pub(super) fn dotenv_database_url_key(key: &str) -> bool {
    key == "DATABASE_URL"
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum CommandDatabaseUrlScope {
    Inherited,
    Assigned(ShellWord),
    Removed,
    Ambiguous,
}

pub(super) fn command_database_url_scope(
    words: &[ShellWord],
    program_index: usize,
) -> CommandDatabaseUrlScope {
    let (scope, index) = database_url_prefix_scope(words, program_index);
    command_database_url_scope_after_prefix(words, program_index, scope, index)
}

fn database_url_prefix_scope(
    words: &[ShellWord],
    program_index: usize,
) -> (CommandDatabaseUrlScope, usize) {
    let mut scope = CommandDatabaseUrlScope::Inherited;
    let mut index = 0;
    let mut allow_prefix_keyword = true;

    while index < program_index {
        let Some(word) = words.get(index) else {
            break;
        };
        if shell_word_is_assignment(word) {
            allow_prefix_keyword = false;
            apply_database_url_assignment(&mut scope, word, true);
        } else if !shell_word_is_prefix_keyword(word, allow_prefix_keyword) {
            break;
        }
        index += 1;
    }
    (scope, index)
}

fn command_database_url_scope_after_prefix(
    words: &[ShellWord],
    program_index: usize,
    mut scope: CommandDatabaseUrlScope,
    mut index: usize,
) -> CommandDatabaseUrlScope {
    while index < program_index {
        let word = shell_word_value(&words[index]);
        match word.as_str() {
            "builtin" => match builtin_wrapper_target(words, index + 1) {
                WrapperTarget::Executable { index: target, .. }
                    if bash_builtin(&shell_word_value(&words[target])) =>
                {
                    index = target;
                }
                WrapperTarget::Executable { .. }
                | WrapperTarget::NoExternalExecutable
                | WrapperTarget::Ambiguous => return CommandDatabaseUrlScope::Ambiguous,
            },
            "command" => match command_wrapper_target(words, index + 1) {
                WrapperTarget::Executable {
                    index: target,
                    ambiguous,
                } => {
                    if ambiguous {
                        scope = CommandDatabaseUrlScope::Ambiguous;
                    }
                    index = target;
                }
                WrapperTarget::NoExternalExecutable | WrapperTarget::Ambiguous => {
                    return CommandDatabaseUrlScope::Ambiguous;
                }
            },
            "exec" => match exec_wrapper_target(words, index + 1) {
                WrapperTarget::Executable {
                    index: target,
                    ambiguous,
                } => {
                    if exec_wrapper_clears_environment(words, index + 1, target) {
                        scope = CommandDatabaseUrlScope::Removed;
                    }
                    if ambiguous {
                        scope = CommandDatabaseUrlScope::Ambiguous;
                    }
                    index = target;
                }
                WrapperTarget::NoExternalExecutable | WrapperTarget::Ambiguous => {
                    return CommandDatabaseUrlScope::Ambiguous;
                }
            },
            _ if executable_is_named(&word, "nohup") => {
                match nohup_wrapper_target(words, index + 1) {
                    WrapperTarget::Executable {
                        index: target,
                        ambiguous,
                    } => {
                        if ambiguous {
                            scope = CommandDatabaseUrlScope::Ambiguous;
                        }
                        index = target;
                    }
                    WrapperTarget::NoExternalExecutable | WrapperTarget::Ambiguous => {
                        return CommandDatabaseUrlScope::Ambiguous;
                    }
                }
            }
            _ if executable_is_named(&word, "time") => {
                match time_wrapper_target(words, index + 1) {
                    WrapperTarget::Executable {
                        index: target,
                        ambiguous,
                    } => {
                        if ambiguous {
                            scope = CommandDatabaseUrlScope::Ambiguous;
                        }
                        index = target;
                    }
                    WrapperTarget::NoExternalExecutable | WrapperTarget::Ambiguous => {
                        return CommandDatabaseUrlScope::Ambiguous;
                    }
                }
            }
            _ if executable_is_named(&word, "env") => {
                let env = parse_env_wrapper(words, index + 1);
                if env.ambiguous {
                    scope = CommandDatabaseUrlScope::Ambiguous;
                }
                if env.clears_environment
                    || env.unset_names.iter().any(|name| database_url_name(name))
                {
                    scope = CommandDatabaseUrlScope::Removed;
                }
                for assignment_index in env.assignment_indices {
                    apply_database_url_assignment(&mut scope, &words[assignment_index], false);
                }
                let Some(target) = env.target_index else {
                    return if env.ambiguous {
                        CommandDatabaseUrlScope::Ambiguous
                    } else {
                        scope
                    };
                };
                index = target;
            }
            _ => break,
        }
    }

    scope
}

fn apply_database_url_assignment(
    scope: &mut CommandDatabaseUrlScope,
    word: &ShellWord,
    require_plain_name: bool,
) {
    if require_plain_name && !word.assignment_name_plain {
        return;
    }
    let value = shell_word_value(word);
    let Some((raw_name, value)) = value.split_once('=') else {
        return;
    };
    let append = require_plain_name && raw_name.ends_with('+');
    let name = if require_plain_name {
        bash_assignment_base_name(raw_name).unwrap_or(raw_name)
    } else {
        raw_name
    };
    if database_url_name(name) {
        *scope = if append || require_plain_name && raw_name.contains('[') {
            CommandDatabaseUrlScope::Ambiguous
        } else {
            CommandDatabaseUrlScope::Assigned(word.with_value(value))
        };
    }
}

fn database_url_name(name: &str) -> bool {
    name == "DATABASE_URL"
}

pub(super) fn shell_command_mutates_database_url(words: &[ShellWord]) -> bool {
    shell_command_may_persist_variable_change(words, &database_url_name)
}
