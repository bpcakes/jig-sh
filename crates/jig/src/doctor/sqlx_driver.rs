//! Which SQLx database driver a repository's commands select.

use std::ffi::OsStr;
use std::path::Path;

use super::cargo_sqlx::cargo_subcommand_index;
use super::database_url::{
    CommandDatabaseUrlScope, DotenvDatabaseUrl, DotenvLookup, command_database_url_scope,
    dotenv_exists_above_repo, nearest_database_url_from_dotenv, shell_command_mutates_database_url,
};
use super::programs::active_optional_cargo_branch;
use super::shell_analysis::{
    ShellSeparator, ShellWord, command_program_index, executable_basename, literal_exit_guard,
    parse_shell_commands, resolve_literal_cd, shell_command_changes_directory,
    shell_command_has_ambiguous_wrapper, shell_word_value,
};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) enum SqlxDriver {
    Postgres,
    Sqlite,
}

impl SqlxDriver {
    pub(super) fn from_database_url(database_url: &str) -> Option<Self> {
        let scheme = database_url.trim().split_once(':')?.0;
        if scheme.eq_ignore_ascii_case("sqlite") {
            Some(Self::Sqlite)
        } else if scheme.eq_ignore_ascii_case("postgres")
            || scheme.eq_ignore_ascii_case("postgresql")
        {
            Some(Self::Postgres)
        } else {
            None
        }
    }

    pub(super) const fn key(self) -> &'static str {
        match self {
            Self::Postgres => "postgres",
            Self::Sqlite => "sqlite",
        }
    }

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Postgres => "PostgreSQL",
            Self::Sqlite => "SQLite",
        }
    }

    pub(super) const fn probe_url(self) -> &'static str {
        match self {
            // The generic URL parser accepts this URL, then the PostgreSQL
            // driver rejects the invalid sslmode before opening a socket.
            Self::Postgres => "postgres://127.0.0.1/jig_doctor_probe?sslmode=jig-doctor-invalid",
            // Any migration bookkeeping is confined to this process-local DB.
            Self::Sqlite => "sqlite::memory:",
        }
    }

    pub(super) const fn install_command(self) -> &'static str {
        match self {
            Self::Postgres => {
                "cargo install sqlx-cli --force --no-default-features --features rustls,postgres"
            }
            Self::Sqlite => {
                "cargo install sqlx-cli --force --no-default-features --features sqlite"
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SqlxDriverSource {
    CommandFlag,
    CommandAssignment,
    Environment,
    Dotenv,
    DotenvExample,
}

impl SqlxDriverSource {
    pub(super) const fn key(self) -> &'static str {
        match self {
            Self::CommandFlag => "command_flag",
            Self::CommandAssignment => "command_assignment",
            Self::Environment => "environment",
            Self::Dotenv => ".env",
            Self::DotenvExample => ".env.example",
        }
    }

    const fn description(self) -> &'static str {
        match self {
            Self::CommandFlag => "a --database-url command option",
            Self::CommandAssignment => "a command-local DATABASE_URL assignment",
            Self::Environment => "the DATABASE_URL environment variable",
            Self::Dotenv => "DATABASE_URL in .env",
            Self::DotenvExample => "DATABASE_URL in .env.example",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct SqlxDriverRequirement {
    pub(super) driver: SqlxDriver,
    pub(super) source: SqlxDriverSource,
}

impl SqlxDriverRequirement {
    pub(super) fn description(&self) -> String {
        format!(
            "{} driver required by {}",
            self.driver.label(),
            self.source.description()
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SqlxDriverResolution {
    Known(SqlxDriverRequirement),
    Absent,
    Indeterminate(&'static str),
}

pub(super) fn configured_sqlx_driver(
    root: &Path,
    command: &str,
    ambient_database_url: Option<&OsStr>,
) -> SqlxDriverResolution {
    let command =
        active_optional_cargo_branch(root, command).unwrap_or_else(|| command.to_string());
    let parsed = parse_shell_commands(&command);
    let mut requirements = Vec::new();
    let mut saw_sqlx = false;
    let mut mutates_database_url = false;
    let mut effective_cwd = root.to_path_buf();
    let mut cwd_is_ambiguous = false;
    let mut guarded_cd = None;
    let mut wrapper_semantics_are_ambiguous = false;

    for (command_index, words) in parsed.commands.iter().enumerate() {
        let incoming_separator = command_index
            .checked_sub(1)
            .and_then(|index| parsed.separators.get(index))
            .copied();
        let outgoing_separator = parsed.separators.get(command_index).copied();
        if let Some(path) = guarded_cd.take() {
            if literal_exit_guard(words)
                && matches!(
                    outgoing_separator,
                    Some(ShellSeparator::Sequence | ShellSeparator::And)
                )
            {
                effective_cwd = path;
                continue;
            }
            cwd_is_ambiguous = true;
        }
        wrapper_semantics_are_ambiguous |= shell_command_has_ambiguous_wrapper(words);
        if shell_command_changes_directory(words) {
            let standalone = matches!(incoming_separator, None | Some(ShellSeparator::Sequence));
            let resolved = standalone
                .then(|| resolve_literal_cd(root, &effective_cwd, words))
                .flatten();
            match (resolved, outgoing_separator) {
                (Some(path), Some(ShellSeparator::And)) => effective_cwd = path,
                (Some(path), Some(ShellSeparator::Or)) => guarded_cd = Some(path),
                _ => cwd_is_ambiguous = true,
            }
            continue;
        }
        let Some(program_index) = command_program_index(words) else {
            mutates_database_url |= shell_command_mutates_database_url(words);
            continue;
        };
        let Some(invocation) = sqlx_invocation(words, program_index) else {
            mutates_database_url |= shell_command_mutates_database_url(words);
            continue;
        };

        saw_sqlx = true;
        let explicit = sqlx_driver_from_flag(words, invocation.args_index, ambient_database_url);
        let prefix_database_url = command_database_url_scope(words, program_index);
        requirements.push(match (explicit, prefix_database_url) {
            (Some(resolution), _) => resolution,
            (None, CommandDatabaseUrlScope::Assigned(value)) => sqlx_driver_from_command_value(
                &value,
                SqlxDriverSource::CommandAssignment,
                ambient_database_url,
            ),
            (None, CommandDatabaseUrlScope::Removed) => SqlxDriverResolution::Indeterminate(
                "the SQLx command removes DATABASE_URL from its environment",
            ),
            (None, CommandDatabaseUrlScope::Ambiguous) => SqlxDriverResolution::Indeterminate(
                "the SQLx command changes DATABASE_URL through an ambiguous wrapper",
            ),
            (None, CommandDatabaseUrlScope::Inherited)
                if cwd_is_ambiguous || invocation.has_ambiguous_cwd_option =>
            {
                SqlxDriverResolution::Indeterminate(
                    "the SQLx command changes directory in a way doctor cannot resolve safely",
                )
            }
            (None, CommandDatabaseUrlScope::Inherited) => configured_sqlx_driver_fallback(
                root,
                &effective_cwd,
                ambient_database_url,
                invocation.no_dotenv,
            ),
        });
    }

    if parsed.ambiguous || mutates_database_url || wrapper_semantics_are_ambiguous {
        return SqlxDriverResolution::Indeterminate(
            "the SQLx command uses shell syntax whose DATABASE_URL scope is ambiguous",
        );
    }
    if !saw_sqlx {
        return SqlxDriverResolution::Indeterminate(
            "doctor could not identify a supported SQLx CLI invocation",
        );
    }

    let mut known: Option<SqlxDriverRequirement> = None;
    let mut saw_absent = false;
    for resolution in requirements {
        match resolution {
            SqlxDriverResolution::Known(requirement) => {
                if let Some(previous) = known {
                    if previous.driver != requirement.driver {
                        return SqlxDriverResolution::Indeterminate(
                            "the SQLx command invokes different database drivers",
                        );
                    }
                } else {
                    known = Some(requirement);
                }
            }
            SqlxDriverResolution::Absent => saw_absent = true,
            SqlxDriverResolution::Indeterminate(reason) => {
                return SqlxDriverResolution::Indeterminate(reason);
            }
        }
    }
    if known.is_some() && saw_absent {
        return SqlxDriverResolution::Indeterminate(
            "some SQLx invocations have no discoverable database URL",
        );
    }
    known
        .map(SqlxDriverResolution::Known)
        .unwrap_or(SqlxDriverResolution::Indeterminate(
            "no database URL is discoverable for the SQLx command",
        ))
}

pub(super) fn configured_sqlx_driver_fallback(
    root: &Path,
    cwd: &Path,
    ambient_database_url: Option<&OsStr>,
    no_dotenv: bool,
) -> SqlxDriverResolution {
    if let Some(database_url) = ambient_database_url {
        let Some(database_url) = database_url.to_str() else {
            return SqlxDriverResolution::Indeterminate(
                "DATABASE_URL in the environment is not valid UTF-8",
            );
        };
        return sqlx_driver_from_literal(database_url, SqlxDriverSource::Environment);
    }

    if no_dotenv {
        return SqlxDriverResolution::Absent;
    }

    match nearest_database_url_from_dotenv(cwd, root, ".env") {
        Ok(DotenvLookup::Present(Some(DotenvDatabaseUrl::Literal(database_url)))) => {
            return sqlx_driver_from_literal(&database_url, SqlxDriverSource::Dotenv);
        }
        Ok(DotenvLookup::Present(Some(DotenvDatabaseUrl::Substitution))) => {
            return SqlxDriverResolution::Indeterminate(
                "DATABASE_URL in dotenv uses variable substitution",
            );
        }
        // dotenvy stops at the nearest file even when it does not define the
        // requested variable. Do not fall through to a parent or example file.
        Ok(DotenvLookup::Present(None)) => return SqlxDriverResolution::Absent,
        Ok(DotenvLookup::Missing) => {}
        Err(()) => {
            return SqlxDriverResolution::Indeterminate("a dotenv file could not be parsed safely");
        }
    }

    match dotenv_exists_above_repo(root, ".env") {
        Ok(true) => {
            return SqlxDriverResolution::Indeterminate(
                "SQLx may load a .env file above the Jig repository",
            );
        }
        Ok(false) => {}
        Err(()) => {
            return SqlxDriverResolution::Indeterminate(
                "the dotenv search path could not be inspected safely",
            );
        }
    }

    // `.env.example` is a Jig-specific intended-driver hint, not a file SQLx
    // loads. It is only authoritative enough to inspect when no `.env` exists
    // anywhere in the real dotenv search chain.
    match nearest_database_url_from_dotenv(cwd, root, ".env.example") {
        Ok(DotenvLookup::Present(Some(DotenvDatabaseUrl::Literal(database_url)))) => {
            sqlx_driver_from_literal(&database_url, SqlxDriverSource::DotenvExample)
        }
        Ok(DotenvLookup::Present(Some(DotenvDatabaseUrl::Substitution))) => {
            SqlxDriverResolution::Indeterminate("DATABASE_URL in dotenv uses variable substitution")
        }
        Ok(DotenvLookup::Missing | DotenvLookup::Present(None)) => SqlxDriverResolution::Absent,
        Err(()) => SqlxDriverResolution::Indeterminate("a dotenv file could not be parsed safely"),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SqlxInvocation {
    args_index: usize,
    no_dotenv: bool,
    has_ambiguous_cwd_option: bool,
}

fn sqlx_invocation(words: &[ShellWord], program_index: usize) -> Option<SqlxInvocation> {
    let program = shell_word_value(&words[program_index]);
    let basename = executable_basename(&program)?;
    let args_index = if basename.eq_ignore_ascii_case("cargo") {
        let sqlx_index = cargo_subcommand_index(words, program_index + 1)?;
        (shell_word_value(&words[sqlx_index]) == "sqlx").then_some(sqlx_index + 1)?
    } else if basename.eq_ignore_ascii_case("sqlx") {
        program_index + 1
    } else if basename.eq_ignore_ascii_case("cargo-sqlx") {
        let sqlx_index = program_index + 1;
        (words.get(sqlx_index).map(shell_word_value).as_deref() == Some("sqlx"))
            .then_some(sqlx_index + 1)?
    } else {
        return None;
    };
    let no_dotenv = words[args_index..]
        .iter()
        .take_while(|word| shell_word_value(word) != "--")
        .any(|word| shell_word_value(word) == "--no-dotenv");
    let prefix = &words[..args_index];
    let has_ambiguous_cwd_option = prefix.iter().any(|word| {
        let word = shell_word_value(word);
        matches!(word.as_str(), "-C" | "--chdir")
            || (word.starts_with("-C") && word.len() > 2)
            || word.starts_with("--chdir=")
    });
    Some(SqlxInvocation {
        args_index,
        no_dotenv,
        has_ambiguous_cwd_option,
    })
}

fn sqlx_driver_from_flag(
    words: &[ShellWord],
    mut index: usize,
    ambient_database_url: Option<&OsStr>,
) -> Option<SqlxDriverResolution> {
    let mut resolution = None;
    while let Some(word) = words.get(index) {
        let value = shell_word_value(word);
        if value == "--" {
            break;
        }
        let database_url = if matches!(value.as_str(), "--database-url" | "-D") {
            index += 1;
            let Some(value) = words.get(index) else {
                return Some(SqlxDriverResolution::Indeterminate(
                    "--database-url is missing its value",
                ));
            };
            Some(value.clone())
        } else if let Some(value) = value.strip_prefix("--database-url=") {
            Some(word.with_value(value))
        } else if let Some(value) = value.strip_prefix("-D=") {
            Some(word.with_value(value))
        } else if let Some(value) = value.strip_prefix("-D") {
            (!value.is_empty()).then(|| word.with_value(value))
        } else {
            None
        };
        if let Some(database_url) = database_url {
            if resolution.is_some() {
                return Some(SqlxDriverResolution::Indeterminate(
                    "the SQLx command contains multiple --database-url options",
                ));
            }
            resolution = Some(sqlx_driver_from_command_value(
                &database_url,
                SqlxDriverSource::CommandFlag,
                ambient_database_url,
            ));
        }
        index += 1;
    }
    resolution
}

fn sqlx_driver_from_command_value(
    value: &ShellWord,
    source: SqlxDriverSource,
    ambient_database_url: Option<&OsStr>,
) -> SqlxDriverResolution {
    let text = value.value.trim();
    if matches!(text, "$DATABASE_URL" | "${DATABASE_URL}") {
        if value.literal_dollar || !value.active_dollar {
            return SqlxDriverResolution::Indeterminate(
                "an explicit DATABASE_URL reference is quoted or escaped literally",
            );
        }
        if source == SqlxDriverSource::CommandFlag && value.syntactically_plain {
            return SqlxDriverResolution::Indeterminate(
                "an unquoted DATABASE_URL command option can split or expand into multiple arguments",
            );
        }
        let Some(database_url) = ambient_database_url else {
            return SqlxDriverResolution::Indeterminate(
                "an explicit DATABASE_URL reference is unset",
            );
        };
        let Some(database_url) = database_url.to_str() else {
            return SqlxDriverResolution::Indeterminate(
                "an explicit DATABASE_URL reference is not valid UTF-8",
            );
        };
        return sqlx_driver_from_literal(database_url, source);
    }
    if text.is_empty()
        || value.active_dollar
        || value.literal_dollar
        || value.dynamic
        || text.contains('`')
        || text.contains("$(")
    {
        return SqlxDriverResolution::Indeterminate(
            "an explicit database URL is empty or dynamically expanded",
        );
    }
    sqlx_driver_from_literal(text, source)
}

fn sqlx_driver_from_literal(value: &str, source: SqlxDriverSource) -> SqlxDriverResolution {
    SqlxDriver::from_database_url(value)
        .map(|driver| SqlxDriverResolution::Known(SqlxDriverRequirement { driver, source }))
        .unwrap_or(SqlxDriverResolution::Indeterminate(
            "DATABASE_URL does not identify a supported SQLx driver",
        ))
}
