//! Tokens, words, and command-name types shared by the shell analysis.

use std::path::Path;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::doctor) enum ShellCommandName {
    Executable {
        index: usize,
        ambiguous_wrapper: bool,
        force_external: bool,
        changes_cwd: bool,
        path_lookup: ShellPathLookup,
        allow_keyword: bool,
        external_wrappers: Vec<ExternalWrapperReference>,
    },
    NoExternalExecutable {
        external_wrappers: Vec<ExternalWrapperReference>,
        changes_cwd: bool,
    },
    AmbiguousWrapper {
        external_wrappers: Vec<ExternalWrapperReference>,
        changes_cwd: bool,
    },
}

impl ShellCommandName {
    pub(in crate::doctor) fn external_wrappers(&self) -> &[ExternalWrapperReference] {
        match self {
            Self::Executable {
                external_wrappers, ..
            }
            | Self::NoExternalExecutable {
                external_wrappers, ..
            }
            | Self::AmbiguousWrapper {
                external_wrappers, ..
            } => external_wrappers,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::doctor) struct ExternalWrapperReference {
    pub(in crate::doctor) index: usize,
    pub(in crate::doctor) path_lookup: ShellPathLookup,
    pub(in crate::doctor) changes_cwd: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::doctor) enum ShellPathLookup {
    Captured,
    CommandLocal(usize),
    Unverifiable,
}

pub(in crate::doctor) fn executable_basename(program: &str) -> Option<&str> {
    Path::new(program).file_name()?.to_str()
}

pub(in crate::doctor) fn executable_is_named(program: &str, expected: &str) -> bool {
    executable_basename(program) == Some(expected)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::doctor) struct ShellWord {
    pub(in crate::doctor) value: String,
    pub(in crate::doctor) syntactically_plain: bool,
    pub(in crate::doctor) assignment_name_plain: bool,
    pub(in crate::doctor) active_dollar: bool,
    pub(in crate::doctor) literal_dollar: bool,
    pub(in crate::doctor) dynamic: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct ShellWordOrigin {
    pub(super) syntactically_plain: bool,
    pub(super) assignment_name_plain: bool,
}

impl Default for ShellWordOrigin {
    fn default() -> Self {
        Self {
            syntactically_plain: true,
            assignment_name_plain: true,
        }
    }
}

impl ShellWord {
    pub(in crate::doctor) fn with_value(&self, value: &str) -> Self {
        Self {
            value: value.to_string(),
            syntactically_plain: self.syntactically_plain,
            assignment_name_plain: self.assignment_name_plain,
            active_dollar: self.active_dollar,
            literal_dollar: self.literal_dollar,
            dynamic: self.dynamic,
        }
    }
}

pub(in crate::doctor) fn shell_word_value(word: &ShellWord) -> String {
    word.value.clone()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::doctor) enum ShellSeparator {
    And,
    Or,
    Sequence,
    Pipe,
    Background,
    Group,
}

#[derive(Debug)]
pub(in crate::doctor) struct ShellParse {
    pub(in crate::doctor) commands: Vec<Vec<ShellWord>>,
    pub(in crate::doctor) separators: Vec<ShellSeparator>,
    pub(in crate::doctor) ambiguous: bool,
}

#[derive(Debug, Eq, PartialEq)]
pub(super) enum ShellToken {
    Word(ShellWord),
    Separator(ShellSeparator),
    Redirection(String),
}

#[derive(Debug)]
pub(super) struct ShellLex {
    pub(super) tokens: Vec<ShellToken>,
    pub(super) ambiguous: bool,
}

pub(super) const fn is_shell_separator_char(ch: char) -> bool {
    matches!(ch, ';' | '&' | '|' | '(' | ')')
}
