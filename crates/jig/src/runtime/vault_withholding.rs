//! Withholds the reserved vault passphrase variables from Jig commands that
//! never capture them, and explains that boundary in nested vault diagnostics.

use std::borrow::Cow;
use std::process::Command;

use jig_vault::{VAULT_NEW_PASSPHRASE_ENV, VAULT_PASSPHRASE_ENV};

use super::VAULT_PASSPHRASE_OPERATOR_GUIDANCE;

/// Non-secret marker an outer Jig command sets when it removed a reserved
/// passphrase variable, so nested vault commands can explain the missing value.
pub(crate) const VAULT_PASSPHRASE_WITHHELD_ENV: &str = "JIG_VAULT_PASSPHRASE_WITHHELD";

/// Agents read this note. Keep the task outside recording runners, which can
/// persist secret-bearing output before an outer vault wrapper redacts it.
const WITHHELD_PASSPHRASE_NOTE: &str = "An outer Jig command withheld the vault passphrase from its child processes (JIG_VAULT_PASSPHRASE_WITHHELD=1), so this nested command cannot use it. Run vault commands directly rather than from Jig checks, run actions, dev apps, or Jig-launched agents. Ask the operator to run the task directly with `scripts/jig vault exec --env-file FILE -- TASK_COMMAND` outside the recording runner. Do not wrap `scripts/jig check` or `scripts/jig run` in vault exec: those commands can persist secret-bearing output in run history before the outer wrapper redacts it. Do not wrap `scripts/jig dev` or an agent launch either; a Jig-launched agent must instead ask the operator to run the vault command in a terminal.";

/// Removes both reserved variables from Jig's own environment, and therefore
/// from every child it starts, then sets the non-secret withheld marker. The
/// environment is left untouched when neither variable is present.
pub(crate) fn withhold_vault_passphrase_environment() {
    if !reserved_passphrase_present() {
        return;
    }
    // SAFETY: the CLI calls this once at startup, right after argument parsing
    // and before launcher validation, repository loading, worker threads, or
    // child processes exist, so no other thread can access the environment.
    unsafe {
        std::env::remove_var(VAULT_PASSPHRASE_ENV);
        std::env::remove_var(VAULT_NEW_PASSPHRASE_ENV);
        std::env::set_var(VAULT_PASSPHRASE_WITHHELD_ENV, "1");
    }
}

/// Operator guidance for passphrase-unavailable diagnostics. When an outer Jig
/// command withheld the passphrase, a value-free note explaining why precedes
/// the shared guidance; the marker is ignored while a passphrase is present.
pub(crate) fn vault_passphrase_operator_guidance() -> Cow<'static, str> {
    if passphrase_withheld_by_outer_command() {
        Cow::Owned(format!(
            "{WITHHELD_PASSPHRASE_NOTE} {VAULT_PASSPHRASE_OPERATOR_GUIDANCE}"
        ))
    } else {
        Cow::Borrowed(VAULT_PASSPHRASE_OPERATOR_GUIDANCE)
    }
}

/// Keeps both reserved variables out of a Jig-owned helper process even when
/// it runs before a vault command has captured and cleared them.
pub(crate) fn withhold_vault_passphrase(command: &mut Command) -> &mut Command {
    command
        .env_remove(VAULT_PASSPHRASE_ENV)
        .env_remove(VAULT_NEW_PASSPHRASE_ENV)
}

fn passphrase_withheld_by_outer_command() -> bool {
    !reserved_passphrase_present()
        && std::env::var_os(VAULT_PASSPHRASE_WITHHELD_ENV).is_some_and(|value| value == "1")
}

fn reserved_passphrase_present() -> bool {
    std::env::var_os(VAULT_PASSPHRASE_ENV).is_some()
        || std::env::var_os(VAULT_NEW_PASSPHRASE_ENV).is_some()
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use super::*;
    use crate::test_env::{EnvVarGuard, lock_env};

    #[test]
    fn withholding_removes_present_vault_passphrases_and_sets_the_marker() {
        let _env = lock_env();
        let _marker = EnvVarGuard::remove(VAULT_PASSPHRASE_WITHHELD_ENV);
        let _current = EnvVarGuard::remove(VAULT_PASSPHRASE_ENV);
        let _new = EnvVarGuard::remove(VAULT_NEW_PASSPHRASE_ENV);

        withhold_vault_passphrase_environment();
        assert!(std::env::var_os(VAULT_PASSPHRASE_WITHHELD_ENV).is_none());

        for (current, new) in [(true, true), (true, false), (false, true)] {
            let _marker = EnvVarGuard::remove(VAULT_PASSPHRASE_WITHHELD_ENV);
            let _current = current
                .then(|| EnvVarGuard::set(VAULT_PASSPHRASE_ENV, "test-only-reserved-current"));
            let _new =
                new.then(|| EnvVarGuard::set(VAULT_NEW_PASSPHRASE_ENV, "test-only-reserved-new"));

            withhold_vault_passphrase_environment();

            assert!(std::env::var_os(VAULT_PASSPHRASE_ENV).is_none());
            assert!(std::env::var_os(VAULT_NEW_PASSPHRASE_ENV).is_none());
            assert_eq!(
                std::env::var(VAULT_PASSPHRASE_WITHHELD_ENV).as_deref(),
                Ok("1")
            );
        }
    }

    #[test]
    fn withheld_vault_passphrase_note_requires_marker_and_absent_passphrases() {
        let _env = lock_env();
        let _current = EnvVarGuard::remove(VAULT_PASSPHRASE_ENV);
        let _new = EnvVarGuard::remove(VAULT_NEW_PASSPHRASE_ENV);
        let _marker = EnvVarGuard::remove(VAULT_PASSPHRASE_WITHHELD_ENV);
        assert_eq!(
            vault_passphrase_operator_guidance(),
            VAULT_PASSPHRASE_OPERATOR_GUIDANCE
        );

        let _marker = EnvVarGuard::set(VAULT_PASSPHRASE_WITHHELD_ENV, "1");
        let guidance = vault_passphrase_operator_guidance();
        assert!(guidance.starts_with(WITHHELD_PASSPHRASE_NOTE), "{guidance}");
        assert!(guidance.ends_with(VAULT_PASSPHRASE_OPERATOR_GUIDANCE));
        for expected in [
            "outer Jig command withheld the vault passphrase",
            "Run vault commands directly",
            "run the task directly with `scripts/jig vault exec --env-file FILE -- TASK_COMMAND` outside the recording runner",
            "Do not wrap `scripts/jig check` or `scripts/jig run` in vault exec",
            "persist secret-bearing output in run history before the outer wrapper redacts it",
            "Do not wrap `scripts/jig dev` or an agent launch either",
            "a Jig-launched agent must instead ask the operator to run the vault command in a terminal",
        ] {
            assert!(guidance.contains(expected), "{expected}: {guidance}");
        }
        assert!(!guidance.contains("wrap that invocation"), "{guidance}");
        assert!(!guidance.contains("wrap the outer command"), "{guidance}");
        assert!(!guidance.contains("export "), "{guidance}");

        for (name, value) in [
            (VAULT_PASSPHRASE_ENV, "test-only-reserved-current"),
            (VAULT_NEW_PASSPHRASE_ENV, "test-only-reserved-new"),
        ] {
            let _present = EnvVarGuard::set(name, value);
            assert_eq!(
                vault_passphrase_operator_guidance(),
                VAULT_PASSPHRASE_OPERATOR_GUIDANCE,
                "{name}"
            );
        }

        let _marker = EnvVarGuard::set(VAULT_PASSPHRASE_WITHHELD_ENV, "0");
        assert_eq!(
            vault_passphrase_operator_guidance(),
            VAULT_PASSPHRASE_OPERATOR_GUIDANCE
        );
    }

    #[test]
    fn jig_owned_helpers_never_forward_reserved_vault_passphrases() {
        let mut command = Command::new("git");
        command.env(VAULT_PASSPHRASE_ENV, "test-only-reserved-current");

        withhold_vault_passphrase(&mut command);

        let removed = command
            .get_envs()
            .filter(|(_, value)| value.is_none())
            .map(|(name, _)| name)
            .collect::<Vec<_>>();
        assert!(removed.contains(&OsStr::new(VAULT_PASSPHRASE_ENV)));
        assert!(removed.contains(&OsStr::new(VAULT_NEW_PASSPHRASE_ENV)));
    }
}
