//! Programs that configured commands need, and how each one resolves on `PATH`.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::{env, fs};

use jig_repository::shell;

use super::cargo_sqlx::cargo_subcommand;
use super::shell_analysis::{
    ShellCommandName, ShellPathLookup, ShellWord, bash_builtin, executable_is_named,
    parse_shell_commands, path_assignment_value, shell_command_changes_directory,
    shell_command_may_change_dispatch_or_inject_execution, shell_command_may_persist_path_change,
    shell_command_name, shell_word_is_keyword, shell_word_value,
};

pub(crate) fn program_available_on_path(program: &str) -> bool {
    let Ok(command_cwd) = env::current_dir() else {
        return false;
    };
    let Some(search_path) = env::var_os("PATH") else {
        return false;
    };
    resolve_program(&command_cwd, program, Some(&search_path)).is_some()
}

#[cfg(test)]
pub(super) fn command_programs(root: &Path, command: &str) -> Vec<String> {
    required_command_programs(root, command)
        .programs
        .into_iter()
        .map(|program| program.program)
        .collect()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RequiredProgramAmbiguity {
    ShellSyntax,
    ShellState,
    Wrapper,
}

impl RequiredProgramAmbiguity {
    pub(super) const fn description(self) -> &'static str {
        match self {
            Self::ShellSyntax => "the configured Bash syntax cannot be analyzed safely",
            Self::ShellState => {
                "an earlier Bash builtin can change command dispatch or execute hidden commands"
            }
            Self::Wrapper => "a command wrapper can change which executable runs",
        }
    }
}

#[derive(Debug, Default)]
pub(super) struct RequiredCommandPrograms {
    pub(super) programs: Vec<RequiredProgram>,
    pub(super) ambiguity: Option<RequiredProgramAmbiguity>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct RequiredProgram {
    pub(super) program: String,
    pub(super) cargo_sqlx_dispatch: bool,
    pub(super) path_lookup: ProgramPathLookup,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum ProgramPathLookup {
    Explicit,
    Captured,
    CommandLocal(OsString),
    CapturedAfterCwdChange,
    Unverifiable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum ProgramPresence {
    Present(ProgramResolution),
    Missing,
    Unverified,
}

pub(super) fn required_command_programs(root: &Path, command: &str) -> RequiredCommandPrograms {
    if let Some(branch) = active_optional_cargo_branch(root, command) {
        return required_command_programs_for_shell(&branch);
    }
    required_command_programs_for_shell(command)
}

pub(super) fn active_optional_cargo_branch(root: &Path, command: &str) -> Option<String> {
    let (then_branch, else_branch) = shell::optional_cargo_command_branches(command)?;
    Some(
        if root.join("Cargo.toml").exists() {
            then_branch
        } else {
            else_branch
        }
        .to_string(),
    )
}

#[cfg(test)]
pub(super) fn command_program(command: &str) -> Option<String> {
    // Best-effort shell token recognition for diagnostics only. Runtime command
    // execution still goes through the configured shell command unchanged.
    command_programs_for_shell(command).into_iter().next()
}

#[cfg(test)]
pub(super) fn command_programs_for_shell(command: &str) -> Vec<String> {
    required_command_programs_for_shell(command)
        .programs
        .into_iter()
        .map(|program| program.program)
        .collect()
}

pub(super) fn required_command_programs_for_shell(command: &str) -> RequiredCommandPrograms {
    let parsed = parse_shell_commands(command);
    let mut discovery = RequiredCommandPrograms {
        ambiguity: parsed
            .ambiguous
            .then_some(RequiredProgramAmbiguity::ShellSyntax),
        ..RequiredCommandPrograms::default()
    };
    let mut prior_path_lookup_is_unverifiable = false;
    let mut prior_cwd_is_unverifiable = false;
    let mut prior_dispatch_is_unverifiable = false;
    for words in &parsed.commands {
        let (programs, ambiguity) = required_command_programs_for_words(
            words,
            prior_path_lookup_is_unverifiable || prior_dispatch_is_unverifiable,
            prior_cwd_is_unverifiable,
        );
        discovery.programs.extend(programs);
        if discovery.ambiguity.is_none() {
            discovery.ambiguity = ambiguity;
        }
        prior_path_lookup_is_unverifiable |= shell_command_may_persist_path_change(words);
        prior_cwd_is_unverifiable |= shell_command_changes_directory(words);
        if shell_command_may_change_dispatch_or_inject_execution(words) {
            prior_dispatch_is_unverifiable = true;
            if discovery.ambiguity.is_none() {
                discovery.ambiguity = Some(RequiredProgramAmbiguity::ShellState);
            }
        }
    }
    if discovery.ambiguity == Some(RequiredProgramAmbiguity::ShellSyntax) {
        for program in &mut discovery.programs {
            program.path_lookup = ProgramPathLookup::Unverifiable;
        }
    }
    discovery
}

fn required_command_programs_for_words(
    words: &[ShellWord],
    prior_path_lookup_is_unverifiable: bool,
    prior_cwd_is_unverifiable: bool,
) -> (Vec<RequiredProgram>, Option<RequiredProgramAmbiguity>) {
    let command_name = shell_command_name(words);
    let external_wrappers = command_name.external_wrappers().to_vec();
    let (target, mut ambiguity) = match &command_name {
        ShellCommandName::Executable {
            index,
            ambiguous_wrapper,
            force_external,
            changes_cwd,
            path_lookup,
            allow_keyword,
            ..
        } => (
            Some((
                *index,
                *force_external,
                *allow_keyword,
                *path_lookup,
                *changes_cwd,
            )),
            ambiguous_wrapper.then_some(RequiredProgramAmbiguity::Wrapper),
        ),
        ShellCommandName::NoExternalExecutable { .. } => (None, None),
        ShellCommandName::AmbiguousWrapper { .. } => {
            (None, Some(RequiredProgramAmbiguity::Wrapper))
        }
    };

    let mut programs = external_wrappers
        .into_iter()
        .map(|wrapper| {
            required_program_for_index(
                words,
                wrapper.index,
                false,
                prior_path_lookup_is_unverifiable,
                wrapper.path_lookup,
                prior_cwd_is_unverifiable || wrapper.changes_cwd,
            )
        })
        .collect::<Vec<_>>();

    if let Some((index, force_external, allow_keyword, path_lookup, changes_cwd)) = target {
        if words[index].active_dollar || words[index].dynamic {
            ambiguity = Some(RequiredProgramAmbiguity::Wrapper);
        } else if !force_external
            && (bash_builtin(&shell_word_value(&words[index]))
                || allow_keyword && shell_word_is_keyword(&words[index]))
        {
            if matches!(
                shell_word_value(&words[index]).as_str(),
                "." | "eval" | "source"
            ) {
                ambiguity = Some(RequiredProgramAmbiguity::ShellSyntax);
            }
        } else {
            let cargo_sqlx_dispatch =
                executable_is_named(&shell_word_value(&words[index]), "cargo")
                    && cargo_subcommand(words, index + 1).as_deref() == Some("sqlx");
            programs.push(required_program_for_index(
                words,
                index,
                cargo_sqlx_dispatch,
                prior_path_lookup_is_unverifiable,
                path_lookup,
                prior_cwd_is_unverifiable || changes_cwd,
            ));
        }
    }

    (programs, ambiguity)
}

fn required_program_for_index(
    words: &[ShellWord],
    index: usize,
    cargo_sqlx_dispatch: bool,
    prior_path_lookup_is_unverifiable: bool,
    shell_path_lookup: ShellPathLookup,
    cwd_lookup_is_unverifiable: bool,
) -> RequiredProgram {
    let word = shell_word_value(&words[index]);
    let path_lookup = if program_has_explicit_path(&word) {
        if cwd_lookup_is_unverifiable && !Path::new(&word).is_absolute() {
            ProgramPathLookup::Unverifiable
        } else {
            ProgramPathLookup::Explicit
        }
    } else if prior_path_lookup_is_unverifiable {
        ProgramPathLookup::Unverifiable
    } else {
        match shell_path_lookup {
            ShellPathLookup::Captured if cwd_lookup_is_unverifiable => {
                ProgramPathLookup::CapturedAfterCwdChange
            }
            ShellPathLookup::Captured => ProgramPathLookup::Captured,
            ShellPathLookup::CommandLocal(index) => {
                let value = path_assignment_value(&words[index])
                    .expect("command-local PATH state must reference an assignment");
                if cwd_lookup_is_unverifiable
                    && !search_path_is_cwd_independent(Some(value.as_os_str()))
                {
                    ProgramPathLookup::Unverifiable
                } else {
                    ProgramPathLookup::CommandLocal(value)
                }
            }
            ShellPathLookup::Unverifiable => ProgramPathLookup::Unverifiable,
        }
    };
    RequiredProgram {
        cargo_sqlx_dispatch,
        program: word,
        path_lookup,
    }
}

pub(super) fn reported_program(command_key: &str, program: &str) -> (String, bool) {
    let sqlx_safe_name = matches!(program, "cargo" | "cargo-sqlx" | "sqlx");
    let redact = (command_key == "sqlx_check_command" && !sqlx_safe_name)
        || credential_like_program(program);
    if redact {
        ("<redacted: command executable>".to_string(), true)
    } else {
        (program.to_string(), false)
    }
}

fn credential_like_program(program: &str) -> bool {
    let lowercase = program.to_ascii_lowercase();
    program.contains("://")
        || (program.contains('@') && program.contains(':'))
        || ["password", "passwd", "secret", "token", "apikey", "api_key"]
            .iter()
            .any(|marker| lowercase.contains(marker))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum ProgramOrigin {
    ExplicitPath,
    SearchPath { entry: PathBuf },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ProgramResolution {
    pub(super) path: PathBuf,
    pub(super) origin: ProgramOrigin,
}

pub(super) fn search_path_is_cwd_independent(search_path: Option<&OsStr>) -> bool {
    let Some(search_path) = search_path else {
        return false;
    };
    env::split_paths(search_path).all(|entry| !entry.as_os_str().is_empty() && entry.is_absolute())
}

pub(super) fn resolve_program(
    command_cwd: &Path,
    program: &str,
    search_path: Option<&OsStr>,
) -> Option<ProgramResolution> {
    if program_has_explicit_path(program) {
        let path = PathBuf::from(program);
        let path = if path.is_absolute() {
            path
        } else {
            command_cwd.join(path)
        };
        return executable_exists(&path).then_some(ProgramResolution {
            path,
            origin: ProgramOrigin::ExplicitPath,
        });
    }

    let search_path = search_path?;
    for entry in env::split_paths(search_path) {
        let directory = if entry.is_absolute() {
            entry.clone()
        } else {
            command_cwd.join(&entry)
        };
        let path = directory.join(program);
        if executable_exists(&path) {
            return Some(ProgramResolution {
                path,
                origin: ProgramOrigin::SearchPath { entry },
            });
        }
    }
    None
}

pub(super) fn program_has_explicit_path(program: &str) -> bool {
    let path = Path::new(program);
    path.is_absolute() || path.components().count() > 1 || program.contains('/')
}

pub(super) fn program_presence(
    root: &Path,
    program: &str,
    resolved: Option<&Path>,
) -> (bool, String) {
    match resolved {
        Some(_) => (true, format!("{program} is available")),
        None if program_has_explicit_path(program) => {
            let path = PathBuf::from(program);
            let path = if path.is_absolute() {
                path
            } else {
                root.join(path)
            };
            (
                false,
                format!("{} is missing or not executable", path.display()),
            )
        }
        None => (false, format!("{program} was not found on PATH")),
    }
}

fn executable_exists(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}
