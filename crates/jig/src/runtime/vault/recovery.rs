use jig_vault::{VaultError, VaultErrorKind, VaultRecovery};

pub(super) fn message(error: &VaultError) -> String {
    match error.recovery() {
        Some(VaultRecovery::Integrity) => format!(
            "{} Operator step: preserve the vault, audit log, and recovery data for investigation. Never delete or edit the rollback witness or its journals to bypass this refusal. Agents must ask the operator to resolve the integrity failure.",
            error.message(),
        ),
        Some(VaultRecovery::StorageConflict) => format!(
            "{} {} Resolve the conflict without deleting the vault rollback witness or its journals, then rerun the same restore or an authenticated vault command for the affected home.",
            error.message(),
            super::scope::VAULT_STORAGE_OPERATOR_STEP,
        ),
        Some(VaultRecovery::Credential) => format!(
            "{} {}",
            error.message(),
            crate::runtime::VAULT_PASSPHRASE_OPERATOR_GUIDANCE,
        ),
        _ if error.kind() == VaultErrorKind::InvalidInput
            && error.message() == jig_vault::NEW_VAULT_PASSPHRASE_POLICY =>
        {
            format!(
                "{} {}",
                error.message(),
                crate::runtime::vault_passphrase_operator_guidance(),
            )
        }
        _ => error.message().to_owned(),
    }
}

pub(super) fn operator_guidance(error: anyhow::Error) -> anyhow::Error {
    let Some(vault_error) = error.downcast_ref::<VaultError>() else {
        return error;
    };
    let message = message(vault_error);
    if message == vault_error.message() {
        error
    } else {
        // Keep the original typed error available to callers and preserve its kind.
        error.context(message)
    }
}

pub(super) fn vault_operator_guidance(error: VaultError) -> anyhow::Error {
    operator_guidance(error.into())
}

#[cfg(test)]
mod tests;
