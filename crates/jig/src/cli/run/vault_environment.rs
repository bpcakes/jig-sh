//! CLI-startup boundary for the reserved vault passphrase environment.

use crate::cli::CommandKind;
use crate::cli::bootstrap_run::{adopt_requests_vault_setup, init_requests_vault_setup};
use crate::runtime;

impl CommandKind {
    /// Whether this invocation may capture `JIG_VAULT_PASSPHRASE` or
    /// `JIG_VAULT_NEW_PASSPHRASE`: every `vault` subcommand, and `init` or
    /// `adopt --write` when they set up the vault. Every other command
    /// withholds both variables from itself and its children at startup.
    pub(super) const fn may_capture_vault_passphrase(&self) -> bool {
        match self {
            Self::Vault(_) => true,
            Self::Init(opts) => init_requests_vault_setup(opts),
            Self::Adopt(opts) => adopt_requests_vault_setup(opts),
            // Keep this match exhaustive so every new top-level command makes
            // this choice explicitly; commands that never unlock the vault
            // belong here.
            Self::RuntimeCompatible(_)
            | Self::Presets
            | Self::Update(_)
            | Self::Bootstrap
            | Self::Setup
            | Self::Doctor
            | Self::Info(_)
            | Self::Dev(_)
            | Self::Check(_)
            | Self::Run(_)
            | Self::FileBudget(_)
            | Self::Status(_)
            | Self::Ui(_)
            | Self::Loop(_)
            | Self::Migration(_)
            | Self::Sqlx(_)
            | Self::MigrationAdd(_)
            | Self::SchemaDump
            | Self::GenerateSqlxUncheckedQueriesTodo(_)
            | Self::Proxy(_)
            | Self::Agent(_)
            | Self::Codex(_)
            | Self::Claude(_)
            | Self::AgentMap(_)
            | Self::State(_) => false,
        }
    }
}

/// Withholds both reserved passphrase variables, and records the non-secret
/// withheld marker, unless the parsed command may capture the passphrase.
/// Invariant: call this before launcher validation, repository loading,
/// worker threads, or any child process.
pub(super) fn enforce_vault_passphrase_startup_boundary(command: &CommandKind) {
    if !command.may_capture_vault_passphrase() {
        runtime::withhold_vault_passphrase_environment();
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;
    use jig_vault::{VAULT_NEW_PASSPHRASE_ENV, VAULT_PASSPHRASE_ENV};

    use super::*;
    use crate::cli::Cli;
    use crate::runtime::VAULT_PASSPHRASE_WITHHELD_ENV;
    use crate::test_env::{EnvVarGuard, lock_env};

    const CAPTURING: &[&[&str]] = &[
        &["vault", "status"],
        &["vault", "exec", "--env-file", ".env.jig", "--", "true"],
        &["vault", "passphrase", "change"],
        &["vault", "tui"],
        &["init", "ExampleProject"],
        &["adopt", ".", "--write"],
    ];

    const WITHHOLDING: &[&[&str]] = &[
        &["check", "test"],
        &["run", "api:test"],
        &["dev"],
        &["doctor"],
        &["bootstrap"],
        &["setup"],
        &["claude", "launch", "work"],
        &["update", "."],
        &["init", "ExampleProject", "--no-vault"],
        &["adopt", "."],
        &["adopt", ".", "--write", "--no-vault"],
    ];

    fn command(args: &[&str]) -> CommandKind {
        Cli::try_parse_from(std::iter::once("jig").chain(args.iter().copied()))
            .unwrap_or_else(|error| panic!("{args:?} did not parse: {error}"))
            .command
    }

    #[test]
    fn only_vault_and_vault_initializing_bootstrap_commands_may_capture_the_passphrase() {
        for args in CAPTURING {
            assert!(command(args).may_capture_vault_passphrase(), "{args:?}");
        }
        for args in WITHHOLDING {
            assert!(!command(args).may_capture_vault_passphrase(), "{args:?}");
        }
    }

    #[test]
    fn startup_boundary_withholds_reserved_vault_variables_from_non_capturing_commands() {
        let _env = lock_env();
        let cases = CAPTURING
            .iter()
            .map(|args| (args, true))
            .chain(WITHHOLDING.iter().map(|args| (args, false)));
        for (args, captures) in cases {
            let _current = EnvVarGuard::set(VAULT_PASSPHRASE_ENV, "test-only-reserved-current");
            let _new = EnvVarGuard::set(VAULT_NEW_PASSPHRASE_ENV, "test-only-reserved-new");
            let _marker = EnvVarGuard::remove(VAULT_PASSPHRASE_WITHHELD_ENV);

            enforce_vault_passphrase_startup_boundary(&command(args));

            assert_eq!(
                std::env::var_os(VAULT_PASSPHRASE_ENV).is_some(),
                captures,
                "{args:?}"
            );
            assert_eq!(
                std::env::var_os(VAULT_NEW_PASSPHRASE_ENV).is_some(),
                captures,
                "{args:?}"
            );
            assert_eq!(
                std::env::var(VAULT_PASSPHRASE_WITHHELD_ENV).ok().as_deref(),
                (!captures).then_some("1"),
                "{args:?}"
            );
        }
    }

    #[test]
    fn startup_boundary_marks_only_an_actual_vault_passphrase_removal() {
        let _env = lock_env();
        let _current = EnvVarGuard::remove(VAULT_PASSPHRASE_ENV);
        let _new = EnvVarGuard::remove(VAULT_NEW_PASSPHRASE_ENV);
        let _marker = EnvVarGuard::remove(VAULT_PASSPHRASE_WITHHELD_ENV);

        enforce_vault_passphrase_startup_boundary(&command(&["check", "test"]));
        assert!(std::env::var_os(VAULT_PASSPHRASE_WITHHELD_ENV).is_none());

        // A stale rotation value alone is still withheld and marked.
        let _new = EnvVarGuard::set(VAULT_NEW_PASSPHRASE_ENV, "test-only-reserved-new");
        enforce_vault_passphrase_startup_boundary(&command(&["check", "test"]));
        assert!(std::env::var_os(VAULT_NEW_PASSPHRASE_ENV).is_none());
        assert_eq!(
            std::env::var(VAULT_PASSPHRASE_WITHHELD_ENV).as_deref(),
            Ok("1")
        );
    }
}
