//! Vault check.

use std::fmt::Write as _;

use jig_context::RepoContext;
use serde_json::Value;

use super::check::{DoctorCheck, check};
use crate::command::{VaultCommand, VaultStatusRequest};

pub(super) fn vault_check(ctx: std::result::Result<&RepoContext, String>) -> DoctorCheck {
    let ctx = match ctx {
        Ok(ctx) => ctx,
        Err(error) => {
            return check(
                "vault",
                "Vault",
                false,
                false,
                "blocked",
                format!("Skipped until repo context loads successfully: {error}"),
            )
            .with_fix("Fix the reported repo context issue, then run `scripts/jig vault status`.");
        }
    };
    // Vault status is intentionally a cheap metadata probe and must not prompt
    // for a passphrase; doctor relies on that non-authenticated boundary.
    match crate::runtime::dispatch_vault(VaultCommand::Status(VaultStatusRequest {
        vault: crate::runtime::vault_options_for_context(Some(ctx)),
    })) {
        Ok(output) => {
            let initialized = output["exists"].as_bool().unwrap_or(false);
            let check = check(
                "vault",
                "Vault",
                false,
                initialized,
                if initialized {
                    "initialized"
                } else {
                    "not initialized"
                },
                vault_detail(&output),
            )
            .with_optional_fix((!initialized).then_some(VAULT_INIT_OPERATOR_FIX))
            .with_data(output);
            // `vault init` needs a human-chosen passphrase, so it must never
            // become an agent-facing next step.
            if initialized {
                check
            } else {
                check.operator_only()
            }
        }
        Err(error) => check("vault", "Vault", false, false, "error", error.to_string())
            .with_fix("Run `scripts/jig vault status` for vault diagnostics."),
    }
}

pub(super) fn vault_detail(output: &Value) -> String {
    let mut detail = format!(
        "vault_home={}",
        output["vault_home"].as_str().unwrap_or("<unknown>")
    );
    if let Some(scope) = output["vault_scope"].as_str() {
        let _ = write!(detail, " scope={scope}");
    }
    if let Some(scope_id) = output["vault_scope_id"].as_str() {
        let _ = write!(detail, " scope_id={scope_id}");
    }
    if let Some(main_checkout_root) = output["vault_main_checkout_root"].as_str() {
        let _ = write!(detail, " main_checkout_root={main_checkout_root}");
    }
    if output["vault_worktree_local"].as_bool() == Some(true) {
        detail.push_str(" worktree_local=true");
    }
    // Read from the unauthenticated public header; not an integrity check.
    if let Some(version) = output["format_version"].as_u64() {
        let _ = write!(detail, " format_version={version}");
    }
    detail
}

/// Remediation for an uninitialized vault. It is an operator step because the
/// command prompts for a new passphrase that agents must never choose.
pub(super) const VAULT_INIT_OPERATOR_FIX: &str = "Operator step: run `scripts/jig vault init` in a terminal; it prompts for a new vault passphrase. Agents should ask the operator and never choose or handle the passphrase.";
