//! Resolving the program a simple command runs.

use super::assignments::{
    bash_assignment_name, looks_like_shell_assignment, path_variable_name,
    shell_path_assignment_is_literal, shell_word_is_assignment,
};
use super::builtins::{bash_builtin, shell_word_is_keyword, shell_word_is_prefix_keyword};
use super::syntax::{
    ExternalWrapperReference, ShellCommandName, ShellPathLookup, ShellWord, executable_is_named,
    shell_word_value,
};
use super::wrappers::{
    ShellWrapperKind, WrapperTarget, env_wrapper_path_lookup, exec_wrapper_clears_environment,
    parse_env_wrapper, shell_wrapper,
};

pub(in crate::doctor) fn command_program_index(words: &[ShellWord]) -> Option<usize> {
    let ShellCommandName::Executable {
        index,
        force_external,
        allow_keyword,
        ..
    } = shell_command_name(words)
    else {
        return None;
    };
    (force_external
        || (!bash_builtin(&shell_word_value(&words[index]))
            && (!allow_keyword || !shell_word_is_keyword(&words[index]))))
    .then_some(index)
}

pub(in crate::doctor) fn shell_command_name(words: &[ShellWord]) -> ShellCommandName {
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

pub(in crate::doctor) fn shell_command_has_ambiguous_wrapper(words: &[ShellWord]) -> bool {
    matches!(
        shell_command_name(words),
        ShellCommandName::Executable {
            ambiguous_wrapper: true,
            ..
        } | ShellCommandName::AmbiguousWrapper { .. }
    )
}
