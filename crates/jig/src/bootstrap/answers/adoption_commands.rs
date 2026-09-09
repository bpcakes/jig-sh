use super::*;
use jig_contract::ActionRunner;

/// Bind explicit legacy command flags through the authored alias owner, which
/// may have a project-defined target ID or command key.
pub(super) fn overrides(
    opts: &AnswerOpts,
    model: &AuthoredRepositoryModel,
) -> Result<BTreeMap<String, String>> {
    let mut commands = BTreeMap::new();
    for (flag, alias, value) in [
        (
            "bootstrap-command",
            tool::BOOTSTRAP,
            &opts.bootstrap_command,
        ),
        (
            "rust-fmt-check-command",
            tool::FMT_CHECK,
            &opts.rust_fmt_check_command,
        ),
        (
            "rust-clippy-command",
            tool::CLIPPY,
            &opts.rust_clippy_command,
        ),
        ("rust-test-command", tool::TEST, &opts.rust_test_command),
        (
            "rust-test-locked-command",
            tool::TEST_LOCKED,
            &opts.rust_test_locked_command,
        ),
        (
            "sqlx-check-command",
            tool::SQLX_CHECK,
            &opts.sqlx_check_command,
        ),
        (
            "schema-dump-command",
            tool::SCHEMA_DUMP,
            &opts.schema_dump_command,
        ),
        (
            "contract-check-command",
            tool::CONTRACT_CHECK,
            &opts.contract_check_command,
        ),
        (
            "schema-check-command",
            tool::SCHEMA_CHECK,
            &opts.schema_check_command,
        ),
        (
            "migration-add-command",
            tool::MIGRATION_ADD,
            &opts.migration_add_command,
        ),
    ] {
        let Some(value) = value else { continue };
        let owners = model
            .actions
            .iter()
            .filter(|action| action.legacy_aliases.iter().any(|name| name == alias))
            .collect::<Vec<_>>();
        let [owner] = owners.as_slice() else {
            bail!(
                "--{flag} requires exactly one authored command action for {alias}; edit [repository.actions] and [commands] in .jig.toml"
            );
        };
        let (ActionRunner::Command { command, .. } | ActionRunner::Shell { command, .. }) =
            &owner.runner
        else {
            bail!(
                "--{flag} cannot override the native action for {alias}; edit [repository.actions] in .jig.toml"
            );
        };
        if value.trim().is_empty() {
            bail!("--{flag} must not be empty");
        }
        if let Some(previous) = commands.insert(command.clone(), value.clone())
            && previous != *value
        {
            bail!("explicit command flags conflict for authored command '{command}'");
        }
    }
    Ok(commands)
}
