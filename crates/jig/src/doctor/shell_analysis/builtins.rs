//! Bash builtins and reserved words.

use super::syntax::ShellWord;

fn shell_command_prefix_keyword(program: &str) -> bool {
    matches!(
        program,
        "!" | "do" | "done" | "elif" | "else" | "esac" | "fi" | "if" | "then" | "until" | "while"
    )
}

pub(in crate::doctor) fn shell_word_is_prefix_keyword(
    word: &ShellWord,
    allow_prefix_keyword: bool,
) -> bool {
    allow_prefix_keyword && word.syntactically_plain && shell_command_prefix_keyword(&word.value)
}

pub(in crate::doctor) fn shell_word_is_keyword(word: &ShellWord) -> bool {
    word.syntactically_plain && bash_keyword(&word.value)
}

pub(in crate::doctor) fn bash_builtin(program: &str) -> bool {
    matches!(
        program,
        "." | ":"
            | "["
            | "alias"
            | "bg"
            | "bind"
            | "break"
            | "builtin"
            | "caller"
            | "cd"
            | "command"
            | "compgen"
            | "complete"
            | "compopt"
            | "continue"
            | "declare"
            | "dirs"
            | "disown"
            | "echo"
            | "enable"
            | "eval"
            | "exec"
            | "exit"
            | "export"
            | "false"
            | "fc"
            | "fg"
            | "getopts"
            | "hash"
            | "help"
            | "history"
            | "jobs"
            | "kill"
            | "let"
            | "local"
            | "logout"
            | "mapfile"
            | "popd"
            | "printf"
            | "pushd"
            | "pwd"
            | "read"
            | "readarray"
            | "readonly"
            | "return"
            | "set"
            | "shift"
            | "shopt"
            | "source"
            | "suspend"
            | "test"
            | "times"
            | "trap"
            | "true"
            | "type"
            | "typeset"
            | "ulimit"
            | "umask"
            | "unalias"
            | "unset"
            | "wait"
    )
}

pub(in crate::doctor) fn bash_keyword(program: &str) -> bool {
    matches!(
        program,
        "!" | "[["
            | "]]"
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
            | "time"
            | "until"
            | "while"
            | "{"
            | "}"
    )
}
