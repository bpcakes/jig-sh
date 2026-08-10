use anyhow::Result as AnyResult;
use secrecy::SecretString;

use super::OpenVault;
use crate::audit::AuditAction;
use crate::broker::BrokeredRun;
use crate::error::{
    ClassifiedVaultError, classified_kind, classify_source, vault_error_from_anyhow,
};
use crate::run::{
    ResolvedBrokeredEnv, ResolvedBrokeredFile, ResolvedBrokeredRun, RunOutput, run_brokered,
};
use crate::store::VaultStore;
use crate::{Result, VaultError, VaultErrorKind};

struct PreparedBrokeredRun {
    handle: BrokeredRunHandle,
    resolved: ResolvedBrokeredRun,
}

struct BrokeredRunHandle {
    // Retain key material until the matching child outcome is audited.
    vault: OpenVault,
    run_id: String,
}

impl BrokeredRunHandle {
    fn record_finish(&self, store: &VaultStore, output: &RunOutput) -> AnyResult<()> {
        self.vault.append_audit(
            store,
            AuditAction::BrokeredRunFinish,
            serde_json::json!({
                "run_id": self.run_id,
                "exit_status": output.exit_status,
                "exit_signal": output.exit_signal,
            }),
        )?;
        Ok(())
    }

    fn failure_error(
        &self,
        store: &VaultStore,
        stage: &'static str,
        kind: VaultErrorKind,
        error: anyhow::Error,
    ) -> VaultError {
        if let Err(audit_error) = self.record_failure(store, stage) {
            return VaultError::from_anyhow(
                kind,
                error.context(format!(
                    "brokered run failed; additionally failed to append failure audit event: {audit_error}"
                )),
            );
        }
        VaultError::from_anyhow(kind, error)
    }

    fn record_failure(&self, store: &VaultStore, stage: &'static str) -> AnyResult<()> {
        self.vault.append_audit(
            store,
            AuditAction::BrokeredRunFailed,
            brokered_run_failure_details(&self.run_id, stage),
        )?;
        Ok(())
    }
}

impl VaultStore {
    fn prepare_brokered_run(
        &self,
        passphrase: &SecretString,
        request: BrokeredRun,
    ) -> AnyResult<PreparedBrokeredRun> {
        let run_id = ulid::Ulid::new().to_string();
        self.with_lock(|| {
            let vault = self.open_unlocked(passphrase)?;
            let start_details = brokered_run_start_details(&request, &run_id);
            vault
                .append_audit_unlocked(self, AuditAction::BrokeredRunStart, start_details)
                .map_err(|error| {
                    classify_source(
                        VaultErrorKind::AuditTampered,
                        "failed to append brokered run start audit event",
                        error,
                    )
                })?;
            let resolved = resolve_brokered_run(&vault, request).map_err(|error| {
                brokered_failure_error_unlocked(self, &vault, &run_id, "resolve", error)
            })?;
            Ok(PreparedBrokeredRun {
                handle: BrokeredRunHandle { vault, run_id },
                resolved,
            })
        })
    }

    pub(crate) fn run_brokered(
        &self,
        passphrase: &SecretString,
        request: BrokeredRun,
    ) -> Result<RunOutput> {
        let prepared = self
            .prepare_brokered_run(passphrase, request)
            .map_err(|error| {
                if error.is::<ClassifiedVaultError>() {
                    vault_error_from_anyhow(VaultErrorKind::Internal, error)
                } else {
                    self.map_open_error(error)
                }
            })?;
        match run_brokered(prepared.resolved) {
            Ok(output) => {
                prepared
                    .handle
                    .record_finish(self, &output)
                    .map_err(|error| {
                        VaultError::from_anyhow(VaultErrorKind::AuditTampered, error)
                    })?;
                Ok(output)
            }
            Err(error) => {
                Err(prepared
                    .handle
                    .failure_error(self, "process", VaultErrorKind::Process, error))
            }
        }
    }
}

fn brokered_run_start_details(request: &BrokeredRun, run_id: &str) -> serde_json::Value {
    serde_json::json!({
        "run_id": run_id,
        "env": request.env().iter().map(|mapping| serde_json::json!({
            "var": mapping.var().as_str(),
            "secret_name": mapping.secret_name().as_str(),
        })).collect::<Vec<_>>(),
        "files": request.files().iter().map(|mapping| serde_json::json!({
            "var": mapping.var().as_str(),
            "secret_name": mapping.secret_name().as_str(),
        })).collect::<Vec<_>>(),
    })
}

fn brokered_run_failure_details(run_id: &str, stage: &'static str) -> serde_json::Value {
    serde_json::json!({
        "run_id": run_id,
        "stage": stage,
        // Audit logs intentionally omit argv, paths, and original error text.
        "error": "brokered run failed",
    })
}

fn brokered_failure_error_unlocked(
    store: &VaultStore,
    vault: &OpenVault,
    run_id: &str,
    stage: &'static str,
    error: anyhow::Error,
) -> anyhow::Error {
    let kind = classified_kind(&error).unwrap_or(VaultErrorKind::Internal);
    if let Err(audit_error) = vault.append_audit_unlocked(
        store,
        AuditAction::BrokeredRunFailed,
        brokered_run_failure_details(run_id, stage),
    ) {
        return classify_source(
            kind,
            "brokered run failed; additionally failed to append failure audit event",
            error.context(format!(
                "additional audit failure while recording brokered run failure: {audit_error}"
            )),
        );
    }
    error
}

fn resolve_brokered_run(vault: &OpenVault, request: BrokeredRun) -> AnyResult<ResolvedBrokeredRun> {
    let (command, env_mappings, file_mappings) = request.into_parts();
    let mut env = Vec::with_capacity(env_mappings.len());
    for mapping in env_mappings {
        let (var, secret_name) = mapping.into_parts();
        env.push(ResolvedBrokeredEnv {
            var,
            value: vault.secret_value(&secret_name)?,
            secret_name,
        });
    }
    let mut files = Vec::with_capacity(file_mappings.len());
    for mapping in file_mappings {
        let (var, secret_name) = mapping.into_parts();
        files.push(ResolvedBrokeredFile {
            var,
            value: vault.secret_value(&secret_name)?,
            secret_name,
        });
    }
    Ok(ResolvedBrokeredRun {
        command,
        env,
        files,
    })
}
