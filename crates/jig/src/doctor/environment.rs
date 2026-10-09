//! The process environment and process control shared by every probe.

use std::env;
use std::ffi::{OsStr, OsString};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ShellEnvironmentIssue {
    BashEnv,
    PosixEnv,
    CdPath,
    ShellOptions,
    BashOptions,
    TracePrompt,
    TraceFileDescriptor,
    ImportedFunction,
}

impl ShellEnvironmentIssue {
    pub(super) const fn description(self) -> &'static str {
        match self {
            Self::BashEnv => "BASH_ENV can execute a startup file",
            Self::PosixEnv => "ENV can execute a startup file in POSIX shell mode",
            Self::CdPath => "CDPATH can change shell directory resolution",
            Self::ShellOptions => "SHELLOPTS can enable inherited shell options",
            Self::BashOptions => "BASHOPTS can enable inherited Bash options",
            Self::TracePrompt => "PS4 can execute expansions or alter trace output",
            Self::TraceFileDescriptor => "BASH_XTRACEFD can redirect shell trace output",
            Self::ImportedFunction => "an exported Bash function can change command dispatch",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(super) struct DoctorEnvironment {
    pub(super) search_path: Option<OsString>,
    pub(super) database_url: Option<OsString>,
    pub(super) cargo_alias_sqlx: Option<OsString>,
    pub(super) cargo_home: Option<OsString>,
    pub(super) home: Option<OsString>,
    pub(super) probe_environment: Vec<(OsString, OsString)>,
    pub(super) shell_environment_issue: Option<ShellEnvironmentIssue>,
}

#[derive(Clone, Copy)]
pub(super) struct DoctorProcessControl<'a> {
    pub(super) cancellation: Option<&'a dyn Fn() -> bool>,
    pub(super) unavailable_reason: Option<&'static str>,
}

impl DoctorProcessControl<'_> {
    pub(super) const fn allowed_without_signal_session() -> Self {
        Self {
            cancellation: None,
            unavailable_reason: None,
        }
    }

    #[cfg(unix)]
    pub(super) const fn unavailable(reason: &'static str) -> Self {
        Self {
            cancellation: None,
            unavailable_reason: Some(reason),
        }
    }
}

impl DoctorEnvironment {
    pub(super) fn capture() -> Self {
        let bash_env = env::var_os("BASH_ENV");
        let posix_env = env::var_os("ENV");
        let cdpath = env::var_os("CDPATH");
        let shell_options = env::var_os("SHELLOPTS");
        let bash_options = env::var_os("BASHOPTS");
        let trace_prompt = env::var_os("PS4");
        let trace_file_descriptor = env::var_os("BASH_XTRACEFD");
        let shell_environment_issue = inherited_shell_environment_issue(
            [
                (ShellEnvironmentIssue::BashEnv, bash_env.as_deref()),
                (ShellEnvironmentIssue::PosixEnv, posix_env.as_deref()),
                (ShellEnvironmentIssue::CdPath, cdpath.as_deref()),
                (
                    ShellEnvironmentIssue::ShellOptions,
                    shell_options.as_deref(),
                ),
                (ShellEnvironmentIssue::BashOptions, bash_options.as_deref()),
                (ShellEnvironmentIssue::TracePrompt, trace_prompt.as_deref()),
                (
                    ShellEnvironmentIssue::TraceFileDescriptor,
                    trace_file_descriptor.as_deref(),
                ),
            ],
            env::vars_os(),
        );
        Self {
            search_path: env::var_os("PATH"),
            database_url: env::var_os("DATABASE_URL"),
            cargo_alias_sqlx: env::var_os("CARGO_ALIAS_SQLX"),
            cargo_home: env::var_os("CARGO_HOME"),
            home: env::var_os("HOME"),
            probe_environment: ["RUSTUP_HOME", "RUSTUP_TOOLCHAIN"]
                .into_iter()
                .filter_map(|key| env::var_os(key).map(|value| (key.into(), value)))
                .collect(),
            shell_environment_issue,
        }
    }
}

pub(super) fn inherited_shell_environment_issue(
    controls: [(ShellEnvironmentIssue, Option<&OsStr>); 7],
    variables: impl IntoIterator<Item = (OsString, OsString)>,
) -> Option<ShellEnvironmentIssue> {
    for (issue, value) in controls {
        if value.is_some_and(|value| !value.is_empty()) {
            return Some(issue);
        }
    }
    variables
        .into_iter()
        .any(|(key, _)| jig_owned_process::is_exported_bash_function_environment_key(&key))
        .then_some(ShellEnvironmentIssue::ImportedFunction)
}
