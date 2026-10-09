//! Static analysis of configured shell commands.
//!
//! The children are layered: `syntax` holds the shared vocabulary, `tokenizer` and
//! `heredoc` produce words, `builtins`, `assignments`, and `wrappers` classify them,
//! `command_name` resolves the program a command runs, and `control_flow` and
//! `mutations` reason about what a command changes. Everything the rest of `doctor`
//! uses is re-exported here.

mod assignments;
mod builtins;
mod command_name;
mod control_flow;
mod heredoc;
mod mutations;
mod syntax;
mod tokenizer;
mod wrappers;

pub(in crate::doctor) use self::assignments::{
    bash_assignment_name, path_assignment_value, shell_assignment_name, shell_word_is_assignment,
};
#[cfg(test)]
pub(in crate::doctor) use self::builtins::bash_keyword;
pub(in crate::doctor) use self::builtins::{
    bash_builtin, shell_word_is_keyword, shell_word_is_prefix_keyword,
};
pub(in crate::doctor) use self::command_name::{
    command_program_index, shell_command_has_ambiguous_wrapper, shell_command_name,
};
pub(in crate::doctor) use self::control_flow::{
    literal_exit_guard, resolve_literal_cd, shell_command_changes_directory,
};
pub(in crate::doctor) use self::mutations::{
    bash_assignment_base_name, shell_command_may_change_dispatch_or_inject_execution,
    shell_command_may_persist_path_change, shell_command_may_persist_variable_change,
};
pub(in crate::doctor) use self::syntax::{
    ShellCommandName, ShellPathLookup, ShellSeparator, ShellWord, executable_basename,
    executable_is_named, shell_word_value,
};
pub(in crate::doctor) use self::wrappers::{
    WrapperTarget, builtin_wrapper_target, command_wrapper_target, exec_wrapper_clears_environment,
    exec_wrapper_target, nohup_wrapper_target, parse_env_wrapper, time_wrapper_target,
};

use self::heredoc::strip_heredoc_bodies;
use self::syntax::{ShellParse, ShellToken};
use self::tokenizer::{redirection_has_inline_target, shell_tokens};

pub(super) fn parse_shell_commands(command: &str) -> ShellParse {
    let (command, heredoc_ambiguous) = strip_heredoc_bodies(command);
    let mut lexed = shell_tokens(&command);
    lexed.ambiguous |= heredoc_ambiguous;
    let mut commands = Vec::new();
    let mut separators = Vec::new();
    let mut current = Vec::new();
    let mut skip_next_word = false;

    for token in lexed.tokens {
        match token {
            ShellToken::Word(word) => {
                if skip_next_word {
                    skip_next_word = false;
                } else {
                    current.push(word);
                }
            }
            ShellToken::Redirection(redirection) => {
                skip_next_word = !redirection_has_inline_target(&redirection);
            }
            ShellToken::Separator(separator) => {
                if !current.is_empty() {
                    commands.push(std::mem::take(&mut current));
                    separators.push(separator);
                }
                skip_next_word = false;
            }
        }
    }

    if !current.is_empty() {
        commands.push(current);
    }

    let uses_control_flow = commands.iter().any(|words| {
        let uses_non_time_control_flow = words.iter().any(|word| {
            word.syntactically_plain
                && matches!(
                    shell_word_value(word).as_str(),
                    "[[" | "]]"
                        | "case"
                        | "coproc"
                        | "do"
                        | "done"
                        | "elif"
                        | "else"
                        | "esac"
                        | "fi"
                        | "for"
                        | "function"
                        | "if"
                        | "in"
                        | "select"
                        | "then"
                        | "until"
                        | "while"
                        | "{"
                        | "}"
                )
        });
        let uses_time_keyword = matches!(
            shell_command_name(words),
            ShellCommandName::Executable {
                index,
                force_external: false,
                allow_keyword: true,
                ..
            } if shell_word_value(&words[index]) == "time" && shell_word_is_keyword(&words[index])
        );
        uses_non_time_control_flow || uses_time_keyword
    });
    ShellParse {
        commands,
        separators,
        ambiguous: lexed.ambiguous || uses_control_flow,
    }
}
