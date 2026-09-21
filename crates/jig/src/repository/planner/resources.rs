//! Declaration admission and exact invocation authority; never process execution.
use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};
use jig_contract::{
    ActionEffect, ActionIntent, ActionRunner, ActionSpec, CargoImpactContextV1, ExecutionResourceV1,
};
use sha2::{Digest, Sha256};

use crate::repository_path::normalize_portable_repo_path;

pub(in crate::repository) fn validate_declarations(action: &ActionSpec) -> Result<()> {
    if action.resources.is_empty() {
        return Ok(());
    }
    ensure!(
        action.resources.len() <= 8,
        "target '{}' allows at most 8 resource declarations",
        action.target
    );
    ensure!(
        action.intent == ActionIntent::Check
            && action.effects.contains(&ActionEffect::ReadOnly)
            && action.effects.contains(&ActionEffect::Process)
            && action
                .effects
                .iter()
                .all(|effect| matches!(effect, ActionEffect::ReadOnly | ActionEffect::Process)),
        "target '{}' resource coordination requires a read-only process check",
        action.target
    );
    if matches!(action.runner, ActionRunner::Native { .. }) {
        anyhow::bail!(
            "native target '{}' cannot declare execution resources",
            action.target
        );
    }
    let mut seen = BTreeSet::new();
    let mut browser_seen = false;
    for resource in &action.resources {
        match resource {
            ExecutionResourceV1::CargoV1 {
                workspace_manifest,
                working_directory,
                context,
            } => {
                let identity = cargo_identity(
                    action,
                    workspace_manifest,
                    working_directory.as_deref(),
                    context,
                )?;
                ensure!(
                    seen.insert(identity),
                    "target '{}' has duplicate Cargo resource declarations",
                    action.target
                );
            }
            ExecutionResourceV1::PlaywrightServersV1 {} => {
                ensure!(
                    !matches!(action.runner, ActionRunner::RustNextestV1 { .. }),
                    "target '{}' Playwright server resources require a generic process runner",
                    action.target
                );
                ensure!(
                    !browser_seen,
                    "target '{}' has duplicate Playwright server resource declarations",
                    action.target
                );
                browser_seen = true;
            }
        }
    }
    Ok(())
}

fn cargo_identity(
    action: &ActionSpec,
    workspace_manifest: &str,
    working_directory: Option<&str>,
    context: &CargoImpactContextV1,
) -> Result<Vec<u8>> {
    ensure!(
        workspace_manifest.len() <= 4096,
        "Cargo resource workspace_manifest is too long"
    );
    let manifest =
        normalize_portable_repo_path(workspace_manifest, "Cargo resource workspace_manifest")?;
    ensure!(
        manifest.rsplit('/').next() == Some("Cargo.toml"),
        "Cargo resource workspace_manifest must name Cargo.toml"
    );
    let cwd = working_directory.unwrap_or(".");
    ensure!(
        cwd.len() <= 4096,
        "Cargo resource working_directory is too long"
    );
    let cwd = normalize_portable_repo_path(cwd, "Cargo resource working_directory")?;
    // Wrappers may explicitly declare the directory in which they invoke
    // Cargo after changing directory; never infer it from shell text.
    jig_rust::rust_focus::validate_context(context).map_err(anyhow::Error::msg)?;
    if let ActionRunner::RustNextestV1 { configuration } = &action.runner {
        ensure!(
            manifest == configuration.workspace_manifest
                && context == &configuration.context
                && cwd == ".",
            "Cargo resource must match the typed Rust runner manifest, context and repository-root working directory"
        );
    }
    let mut context = context.clone();
    context.features.sort();
    context.features.dedup();
    Ok(serde_json::to_vec(&(manifest, cwd, context))?)
}

/// Bind declared inputs and repository-wide source, retaining legacy digests
/// byte-for-byte when the opt-in scheduling field is absent.
pub(super) fn conservative_action_input_digest(
    contract_version: u32,
    action: &ActionSpec,
    worktree_fingerprint: &str,
) -> Result<String> {
    let mut hasher = Sha256::new();
    if contract_version < super::super::FILE_BUDGET_CONTRACT_VERSION {
        hasher.update(b"jig-target-input-v1\0");
    } else {
        hasher.update(b"jig-target-input-v2\0");
    }
    hasher.update(action.target.to_string().as_bytes());
    hasher.update([0]);
    hasher.update(worktree_fingerprint.as_bytes());
    for input in &action.inputs {
        hasher.update([0]);
        hasher.update(input.as_bytes());
    }
    if contract_version >= super::super::FILE_BUDGET_CONTRACT_VERSION {
        let runner = serde_json::to_vec(&action.runner)
            .context("Failed to canonicalize native target input authority")?;
        hasher.update([0]);
        hasher.update((runner.len() as u64).to_be_bytes());
        hasher.update(runner);
    }
    if contract_version >= super::super::ACTION_EXECUTION_CONTRACT_VERSION {
        let declarations = serde_json::to_vec(&action.arguments)?;
        hasher.update([0]);
        hasher.update((declarations.len() as u64).to_be_bytes());
        hasher.update(declarations);
    }
    if !action.resources.is_empty() {
        let resources = serde_json::to_vec(&action.resources)?;
        hasher.update(b"\0jig-execution-resources-v1\0");
        hasher.update((resources.len() as u64).to_be_bytes());
        hasher.update(resources);
    }
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests;
