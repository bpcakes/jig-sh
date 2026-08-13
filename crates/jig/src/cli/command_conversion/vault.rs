use crate::command;

use super::super::{
    VaultAuditCommand, VaultAuditVerifyOpts, VaultBackupCommand, VaultBackupCreateOpts,
    VaultBackupRestoreOpts, VaultCommand, VaultExecOpts, VaultFieldCommand, VaultFieldListOpts,
    VaultFieldRemoveOpts, VaultFieldSetOpts, VaultImportCommand, VaultImportOnePasswordOpts,
    VaultInitOpts, VaultInjectOpts, VaultMigrateOpts, VaultPassphraseChangeOpts,
    VaultPassphraseCommand, VaultReadOpts, VaultRunOpts, VaultRuntimeOpts, VaultSecretCommand,
    VaultSecretListOpts, VaultSecretRemoveOpts, VaultSecretSetOpts, VaultStatusOpts, VaultTuiOpts,
};

impl From<VaultCommand> for command::VaultCommand {
    fn from(command: VaultCommand) -> Self {
        match command {
            VaultCommand::Audit(command) => Self::Audit(command.into()),
            VaultCommand::Backup(command) => Self::Backup(command.into()),
            VaultCommand::Init(opts) => Self::Init(opts.into()),
            VaultCommand::Status(opts) => Self::Status(opts.into()),
            VaultCommand::Tui(opts) => Self::Tui(opts.into()),
            VaultCommand::Migrate(opts) => Self::Migrate(opts.into()),
            VaultCommand::Passphrase(command) => Self::Passphrase(command.into()),
            VaultCommand::Exec(opts) => Self::Exec(opts.into()),
            VaultCommand::Import(command) => Self::Import(command.into()),
            VaultCommand::Field(command) => Self::Field(command.into()),
            VaultCommand::Inject(opts) => Self::Inject(opts.into()),
            VaultCommand::Read(opts) => Self::Read(opts.into()),
            VaultCommand::Secret(command) => Self::Secret(command.into()),
            VaultCommand::Run(opts) => Self::Run(opts.into()),
        }
    }
}

impl From<VaultBackupCommand> for command::VaultBackupCommand {
    fn from(command: VaultBackupCommand) -> Self {
        match command {
            VaultBackupCommand::Create(opts) => Self::Create(Box::new(opts.into())),
            VaultBackupCommand::Restore(opts) => Self::Restore(Box::new(opts.into())),
        }
    }
}

impl From<VaultBackupCreateOpts> for command::VaultBackupCreateRequest {
    fn from(opts: VaultBackupCreateOpts) -> Self {
        Self {
            output: opts.out,
            overwrite: opts.overwrite,
            prepared: None,
            vault: opts.vault.into(),
        }
    }
}

impl From<VaultBackupRestoreOpts> for command::VaultBackupRestoreRequest {
    fn from(opts: VaultBackupRestoreOpts) -> Self {
        Self {
            input: opts.input,
            prepared: None,
            vault: opts.vault.into(),
        }
    }
}

impl From<VaultPassphraseCommand> for command::VaultPassphraseCommand {
    fn from(command: VaultPassphraseCommand) -> Self {
        match command {
            VaultPassphraseCommand::Change(opts) => Self::Change(opts.into()),
        }
    }
}

impl From<VaultPassphraseChangeOpts> for command::VaultPassphraseChangeRequest {
    fn from(opts: VaultPassphraseChangeOpts) -> Self {
        Self {
            vault: opts.vault.into(),
        }
    }
}

impl From<VaultImportCommand> for command::VaultImportCommand {
    fn from(command: VaultImportCommand) -> Self {
        match command {
            VaultImportCommand::OnePassword(opts) => Self::OnePassword(opts.into()),
        }
    }
}

impl From<VaultImportOnePasswordOpts> for command::VaultImportOnePasswordRequest {
    fn from(opts: VaultImportOnePasswordOpts) -> Self {
        Self {
            env_file: opts.env_file,
            environment: None,
            destination_exists: None,
            item: opts.item,
            out_env: opts.out_env,
            replace: opts.replace,
            overwrite: opts.overwrite,
            dry_run: opts.dry_run,
            vault: opts.vault.into(),
        }
    }
}

impl From<VaultAuditCommand> for command::VaultAuditCommand {
    fn from(command: VaultAuditCommand) -> Self {
        match command {
            VaultAuditCommand::Verify(opts) => Self::Verify(opts.into()),
        }
    }
}

impl From<VaultSecretCommand> for command::VaultSecretCommand {
    fn from(command: VaultSecretCommand) -> Self {
        match command {
            VaultSecretCommand::List(opts) => Self::List(opts.into()),
            VaultSecretCommand::Set(opts) => Self::Set(opts.into()),
            VaultSecretCommand::Remove(opts) => Self::Remove(opts.into()),
        }
    }
}

impl From<VaultFieldCommand> for command::VaultFieldCommand {
    fn from(command: VaultFieldCommand) -> Self {
        match command {
            VaultFieldCommand::List(opts) => Self::List(opts.into()),
            VaultFieldCommand::Set(opts) => Self::Set(opts.into()),
            VaultFieldCommand::Remove(opts) => Self::Remove(opts.into()),
        }
    }
}

impl From<VaultRuntimeOpts> for command::VaultRuntimeOptions {
    fn from(opts: VaultRuntimeOpts) -> Self {
        Self {
            home: opts.home,
            scope: if opts.global {
                command::VaultScopeSelection::Global
            } else {
                command::VaultScopeSelection::Auto
            },
        }
    }
}

impl From<VaultInitOpts> for command::VaultInitRequest {
    fn from(opts: VaultInitOpts) -> Self {
        Self {
            vault: opts.vault.into(),
        }
    }
}

impl From<VaultStatusOpts> for command::VaultStatusRequest {
    fn from(opts: VaultStatusOpts) -> Self {
        Self {
            vault: opts.vault.into(),
        }
    }
}

impl From<VaultTuiOpts> for command::VaultTuiRequest {
    fn from(opts: VaultTuiOpts) -> Self {
        Self {
            vault: opts.vault.into(),
        }
    }
}

impl From<VaultMigrateOpts> for command::VaultMigrateRequest {
    fn from(opts: VaultMigrateOpts) -> Self {
        Self {
            target_version: opts.to,
            vault: opts.vault.into(),
        }
    }
}

impl From<VaultAuditVerifyOpts> for command::VaultAuditVerifyRequest {
    fn from(opts: VaultAuditVerifyOpts) -> Self {
        Self {
            vault: opts.vault.into(),
        }
    }
}

impl From<VaultSecretListOpts> for command::VaultSecretListRequest {
    fn from(opts: VaultSecretListOpts) -> Self {
        Self {
            vault: opts.vault.into(),
        }
    }
}

impl From<VaultFieldListOpts> for command::VaultFieldListRequest {
    fn from(opts: VaultFieldListOpts) -> Self {
        Self {
            item: opts.item,
            vault: opts.vault.into(),
        }
    }
}

impl From<VaultSecretSetOpts> for command::VaultSecretSetRequest {
    fn from(opts: VaultSecretSetOpts) -> Self {
        let value_source = if opts.value_prompt {
            command::VaultSecretValueSource::Prompt
        } else if opts.value_stdin {
            command::VaultSecretValueSource::Stdin
        } else {
            command::VaultSecretValueSource::Auto
        };
        Self {
            name: opts.name,
            value_source,
            vault: opts.vault.into(),
        }
    }
}

impl From<VaultFieldSetOpts> for command::VaultFieldSetRequest {
    fn from(opts: VaultFieldSetOpts) -> Self {
        let value_source = if opts.value_prompt {
            command::VaultSecretValueSource::Prompt
        } else if opts.value_stdin {
            command::VaultSecretValueSource::Stdin
        } else {
            command::VaultSecretValueSource::Auto
        };
        Self {
            reference: opts.reference,
            text: opts.text,
            value_source,
            vault: opts.vault.into(),
        }
    }
}

impl From<VaultSecretRemoveOpts> for command::VaultSecretRemoveRequest {
    fn from(opts: VaultSecretRemoveOpts) -> Self {
        Self {
            name: opts.name,
            vault: opts.vault.into(),
        }
    }
}

impl From<VaultFieldRemoveOpts> for command::VaultFieldRemoveRequest {
    fn from(opts: VaultFieldRemoveOpts) -> Self {
        Self {
            reference: opts.reference,
            vault: opts.vault.into(),
        }
    }
}

impl From<VaultReadOpts> for command::VaultReadRequest {
    fn from(opts: VaultReadOpts) -> Self {
        Self {
            reference: opts.reference,
            reveal: opts.reveal,
            out_file: opts.out_file,
            overwrite: opts.overwrite,
            vault: opts.vault.into(),
        }
    }
}

impl From<VaultInjectOpts> for command::VaultInjectRequest {
    fn from(opts: VaultInjectOpts) -> Self {
        Self {
            input: opts.input,
            template: None,
            reveal: opts.reveal,
            out_file: opts.out_file,
            overwrite: opts.overwrite,
            vault: opts.vault.into(),
        }
    }
}

impl From<VaultExecOpts> for command::VaultExecRequest {
    fn from(opts: VaultExecOpts) -> Self {
        Self {
            env_file: opts.env_file,
            environment: None,
            command: opts.command,
            vault: opts.vault.into(),
        }
    }
}

impl From<VaultRunOpts> for command::VaultRunRequest {
    fn from(opts: VaultRunOpts) -> Self {
        Self {
            env: opts.env,
            files: opts.files,
            command: opts.command,
            vault: opts.vault.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vault_secret_set_defaults_to_auto_value_source() {
        let request: command::VaultSecretSetRequest = VaultSecretSetOpts {
            name: "api_token".into(),
            value_stdin: false,
            value_prompt: false,
            vault: VaultRuntimeOpts::default(),
        }
        .into();

        assert_eq!(request.value_source, command::VaultSecretValueSource::Auto);
    }
}
