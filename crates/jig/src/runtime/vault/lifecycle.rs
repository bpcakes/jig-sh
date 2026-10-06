use std::ffi::OsString;
use std::io::IsTerminal;
#[cfg(unix)]
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use anyhow::{Context, Result, anyhow, bail};
use jig_vault::{
    BrokeredRun, SecretBytes, VAULT_NEW_PASSPHRASE_ENV as NEW_PASSPHRASE_ENV,
    VAULT_PASSPHRASE_ENV as PASSPHRASE_ENV, Vault, VaultError, validate_new_vault_passphrase,
};
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};
use zeroize::Zeroizing;

use crate::command::{
    VaultBackupCommand, VaultBackupCreateRequest, VaultBackupRestoreRequest, VaultCommand,
    VaultImportCommand, VaultPassphraseChangeRequest, VaultPassphraseCommand,
};
use crate::runtime::{VAULT_PASSPHRASE_OPERATOR_GUIDANCE, vault_passphrase_operator_guidance};

use super::{
    ResolvedVaultRuntime, add_vault_scope_fields, parse_env_mappings, parse_file_mappings,
    resolve_vault_runtime, vault, vault_base_home,
};

struct CapturedPassphrases {
    current: Option<SecretString>,
    new: Option<SecretString>,
}

static CAPTURED_PASSPHRASES: Mutex<CapturedPassphrases> = Mutex::new(CapturedPassphrases {
    current: None,
    new: None,
});

pub(crate) fn preflight_scoped_command(command: &mut VaultCommand) -> Result<()> {
    match command {
        VaultCommand::Backup(VaultBackupCommand::Create(request)) => {
            if request.output == Path::new("-") {
                bail!("vault backup create rejects --out -; choose a private backup file");
            }
            let resolved = resolve_vault_runtime(&request.vault)?;
            let source_home = concrete_vault_home(&resolved)?;
            request.prepared = Some(Vault::preflight_backup_create(
                source_home,
                &request.output,
                request.overwrite,
            )?);
            Ok(())
        }
        VaultCommand::Backup(VaultBackupCommand::Restore(request)) => {
            if request.input == Path::new("-") {
                bail!("vault backup restore rejects --in -; choose an encrypted backup file");
            }
            let resolved = resolve_vault_runtime(&request.vault)?;
            let target_home = concrete_vault_home(&resolved)?;
            request.prepared = Some(Vault::preflight_backup_restore(
                &request.input,
                target_home,
            )?);
            Ok(())
        }
        VaultCommand::Passphrase(VaultPassphraseCommand::Change(request)) => {
            let resolved = resolve_vault_runtime(&request.vault)?;
            let home = concrete_vault_home(&resolved)?;
            Vault::preflight_passphrase_change(home)?;
            Ok(())
        }
        VaultCommand::Import(VaultImportCommand::OnePassword(request)) => {
            let resolved = resolve_vault_runtime(&request.vault)?;
            let selected = vault(&resolved)?;
            request.destination = Some(selected.preview_private_output(&request.out_env)?);
            Ok(())
        }
        VaultCommand::Inject(request) => {
            let Some(output) = &request.out_file else {
                return Ok(());
            };
            let resolved = resolve_vault_runtime(&request.vault)?;
            vault(&resolved)?.preflight_private_output(output, request.overwrite)?;
            Ok(())
        }
        VaultCommand::Read(request) => {
            let Some(output) = &request.out_file else {
                return Ok(());
            };
            let resolved = resolve_vault_runtime(&request.vault)?;
            vault(&resolved)?.preflight_private_output(output, request.overwrite)?;
            Ok(())
        }
        VaultCommand::Run(request) => {
            // Reject malformed mappings before passphrase capture so they never
            // cost an operator terminal round trip; `run` parses them again.
            BrokeredRun::with_files(
                request.command.clone(),
                parse_env_mappings(&request.env)?,
                parse_file_mappings(&request.files)?,
            )?;
            Ok(())
        }
        _ => Ok(()),
    }
}

pub(super) fn change_passphrase(request: VaultPassphraseChangeRequest) -> Result<Value> {
    let resolved = resolve_vault_runtime(&request.vault)?;
    let vault = vault(&resolved)?;
    let (current, new) = passphrase_pair()?;
    vault.change_passphrase(&current, &new)?;
    let mut output = json!({
        "ok": true,
        "command": "vault passphrase change",
        "vault_home": vault.root().display().to_string(),
        "changed": true,
    });
    add_vault_scope_fields(&mut output, &resolved);
    Ok(output)
}

pub(super) fn create_backup(mut request: VaultBackupCreateRequest) -> Result<Value> {
    let prepared = request
        .prepared
        .take()
        .ok_or_else(|| anyhow!("internal error: vault backup creation was not preflighted"))?;
    let resolved = resolve_vault_runtime(&request.vault)?;
    let source_home = concrete_vault_home(&resolved)?;
    let passphrase = passphrase()?;
    let result = Vault::create_backup(&passphrase, prepared)?;
    let mut output = json!({
        "ok": true,
        "command": "vault backup create",
        "vault_home": source_home.display().to_string(),
        "backup": request.output.display().to_string(),
        "bytes_written": result.bytes_written,
        "backup_version": result.backup_version,
        "created_at_ms": result.created_at_ms,
    });
    add_vault_scope_fields(&mut output, &resolved);
    Ok(output)
}

pub(super) fn restore_backup(mut request: VaultBackupRestoreRequest) -> Result<Value> {
    let prepared = request
        .prepared
        .take()
        .ok_or_else(|| anyhow!("internal error: vault backup restore was not preflighted"))?;
    let resolved = resolve_vault_runtime(&request.vault)?;
    let passphrase = passphrase()?;
    // Restore is intentionally a static operation. Calling `vault(&resolved)`
    // or `Vault::resolve` here would create the target before the core can
    // enforce its absent-home no-replace contract.
    let result = Vault::restore_backup(&passphrase, prepared)?;
    let mut output = json!({
        "ok": true,
        "command": "vault backup restore",
        "vault_home": result.root.display().to_string(),
        "backup": request.input.display().to_string(),
        "restored": true,
        "vault_id": result.vault_id,
        "format_version": result.format_version,
    });
    add_vault_scope_fields(&mut output, &resolved);
    Ok(output)
}

fn concrete_vault_home(resolved: &ResolvedVaultRuntime) -> Result<PathBuf> {
    resolved.home.clone().map_or_else(vault_base_home, Ok)
}

pub(crate) fn capture_passphrase() -> Result<()> {
    capture_passphrase_with_prompt(PromptKind::Unlock)
}

pub(crate) fn take_optional_tui_passphrase() -> Result<Option<SecretBytes>> {
    let value = std::env::var_os(PASSPHRASE_ENV);
    // Unlike retryable non-interactive capture, the TUI immediately owns its
    // optional credential and must never leave malformed process copies for
    // later workers or child processes to inherit.
    strip_passphrase_environment();
    let Some(value) = value else {
        return Ok(None);
    };
    let passphrase = passphrase_from_os(value, PASSPHRASE_ENV)?;
    Ok(Some(SecretBytes::new(
        passphrase.expose_secret().as_bytes().to_vec(),
    )))
}

pub(crate) fn capture_new_passphrase() -> Result<()> {
    capture_new_passphrase_candidate()?;
    validate_captured_new_passphrase()
}

/// Captures a new-vault passphrase (prompting with confirmation when no
/// environment value exists) without applying the new-passphrase policy.
/// Bootstrap uses this before rendering, when it cannot yet know whether an
/// existing vault will be reused instead of initialized.
pub(crate) fn capture_new_passphrase_candidate() -> Result<()> {
    capture_passphrase_with_prompt(PromptKind::NewVault)
}

/// Applies the new-passphrase policy to the captured passphrase immediately
/// before it becomes a new credential, clearing the capture on rejection.
pub(crate) fn validate_captured_new_passphrase() -> Result<()> {
    let validation = {
        let captured = captured_passphrase_lock()?;
        validate_chosen_passphrase(captured.current.as_ref().ok_or_else(|| {
            anyhow!("vault passphrase capture unexpectedly produced no passphrase")
        })?)
    };
    if let Err(error) = validation {
        clear_captured_passphrase()?;
        return Err(error);
    }
    Ok(())
}

/// Applies the core new-passphrase policy to an operator-chosen candidate.
/// A rejection keeps the core's single value-free message and routes the
/// retry to the operator, who alone may choose a replacement.
fn validate_chosen_passphrase(passphrase: &SecretString) -> Result<()> {
    validate_new_vault_passphrase(passphrase).map_err(|error| {
        let guidance = vault_passphrase_operator_guidance();
        VaultError::new(error.kind(), format!("{} {guidance}", error.message())).into()
    })
}

pub(crate) fn capture_passphrase_change() -> Result<()> {
    let current_from_env = std::env::var_os(PASSPHRASE_ENV).is_some();
    let new_from_env = std::env::var_os(NEW_PASSPHRASE_ENV).is_some();
    if current_from_env || new_from_env {
        return capture_passphrase_pair_from_env();
    }
    if hidden_terminal_input_available() {
        clear_captured_passphrase()?;
        let current = prompt_passphrase(PromptKind::Unlock)?;
        let new = prompt_passphrase(PromptKind::NewVault)?;
        validate_chosen_passphrase(&new)?;
        return set_captured_passphrase_pair(current, new);
    }
    Err(passphrase_change_prompt_unavailable())
}

fn passphrase_change_prompt_unavailable() -> anyhow::Error {
    let guidance = vault_passphrase_operator_guidance();
    anyhow!(
        "`jig vault passphrase change` cannot prompt for the current and new vault passphrases because {NO_PROMPT_TERMINAL}. Without a terminal, operator-managed automation must provide both {PASSPHRASE_ENV} and {NEW_PASSPHRASE_ENV}. {guidance}"
    )
}

/// Shared value-free reason for the no-terminal capture diagnostics; the
/// prompt requires both stdin and stderr to be interactive terminals.
const NO_PROMPT_TERMINAL: &str =
    "stdin and stderr are not both attached to an interactive terminal";

fn require_captured_passphrase(kind: PromptKind) -> Result<()> {
    let passphrase_is_captured = {
        let captured = captured_passphrase_lock()?;
        captured.current.is_some()
    };
    if passphrase_is_captured {
        return Ok(());
    }
    let guidance = vault_passphrase_operator_guidance();
    Err(match kind {
        PromptKind::Unlock => anyhow!(
            "`jig vault` cannot prompt for the vault passphrase because {NO_PROMPT_TERMINAL}, and no operator-provided {PASSPHRASE_ENV} is present. {guidance}"
        ),
        PromptKind::NewVault => anyhow!(
            "The operator must choose and enter a new vault passphrase, but Jig cannot prompt for it because {NO_PROMPT_TERMINAL}. {guidance}"
        ),
    })
}

pub(crate) fn passphrase_prompt_available() -> bool {
    hidden_terminal_input_available()
}

pub(crate) fn passphrase_env_present() -> bool {
    std::env::var_os(PASSPHRASE_ENV).is_some()
}

fn capture_passphrase_with_prompt(kind: PromptKind) -> Result<()> {
    if std::env::var_os(PASSPHRASE_ENV).is_some() {
        return capture_passphrase_from_env();
    }
    if hidden_terminal_input_available() {
        clear_captured_passphrase()?;
        let passphrase = prompt_passphrase(kind)?;
        strip_passphrase_environment();
        set_captured_passphrase(passphrase)?;
        return Ok(());
    }
    capture_passphrase_from_env()?;
    require_captured_passphrase(kind)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PromptKind {
    Unlock,
    NewVault,
}

pub(crate) fn capture_passphrase_from_env() -> Result<()> {
    let Some(value) = std::env::var_os(PASSPHRASE_ENV) else {
        {
            let mut captured = captured_passphrase_lock()?;
            captured.current = None;
            captured.new = None;
        }
        strip_passphrase_environment();
        return Ok(());
    };
    // Keep a malformed current value (and the untouched new value) intact so
    // the operator can inspect or retry them. After successful capture, both
    // reserved process copies are cleared below.
    let passphrase = passphrase_from_os(value, PASSPHRASE_ENV)?;
    strip_passphrase_environment();
    {
        let mut captured = captured_passphrase_lock()?;
        captured.current = Some(passphrase);
        captured.new = None;
    }
    Ok(())
}

pub(crate) fn capture_passphrase_pair_from_env() -> Result<()> {
    clear_captured_passphrase()?;
    let current_value = std::env::var_os(PASSPHRASE_ENV).ok_or_else(incomplete_passphrase_pair)?;
    let new_value = std::env::var_os(NEW_PASSPHRASE_ENV).ok_or_else(incomplete_passphrase_pair)?;
    let current = passphrase_from_os(current_value, PASSPHRASE_ENV)?;
    let new = passphrase_from_os(new_value, NEW_PASSPHRASE_ENV)?;
    // Match new-vault capture: once both values are valid UTF-8, consume the
    // process copies before policy validation. The parent shell is unaffected.
    strip_passphrase_environment();
    validate_chosen_passphrase(&new)?;
    set_captured_passphrase_pair(current, new)
}

fn incomplete_passphrase_pair() -> anyhow::Error {
    anyhow!(
        "{PASSPHRASE_ENV} and {NEW_PASSPHRASE_ENV} must both be provided, or both be absent so the operator can be prompted, for `jig vault passphrase change`. {VAULT_PASSPHRASE_OPERATOR_GUIDANCE}"
    )
}

fn prompt_passphrase(kind: PromptKind) -> Result<SecretString> {
    match kind {
        PromptKind::Unlock => {
            let passphrase = prompt_zeroizing("Jig Vault passphrase: ")
                .context("failed to read vault passphrase from terminal")?;
            Ok(secret_string_from_zeroizing(passphrase))
        }
        PromptKind::NewVault => {
            let passphrase = prompt_zeroizing("New Jig Vault passphrase: ")
                .context("failed to read new vault passphrase from terminal")?;
            let confirmation = prompt_zeroizing("Confirm Jig Vault passphrase: ")
                .context("failed to read vault passphrase confirmation from terminal")?;
            if *passphrase != *confirmation {
                bail!("vault passphrase confirmation did not match");
            }
            Ok(secret_string_from_zeroizing(passphrase))
        }
    }
}

pub(super) fn prompt_zeroizing(prompt: &str) -> Result<Zeroizing<String>> {
    Ok(Zeroizing::new(rpassword::prompt_password(prompt)?))
}

fn secret_string_from_zeroizing(mut value: Zeroizing<String>) -> SecretString {
    SecretString::from(std::mem::take(&mut *value))
}

pub(super) fn set_captured_passphrase(passphrase: SecretString) -> Result<()> {
    {
        let mut captured = captured_passphrase_lock()?;
        captured.current = Some(passphrase);
        captured.new = None;
    }
    Ok(())
}

fn set_captured_passphrase_pair(current: SecretString, new: SecretString) -> Result<()> {
    let mut captured = captured_passphrase_lock()?;
    captured.current = Some(current);
    captured.new = Some(new);
    Ok(())
}

fn clear_captured_passphrase() -> Result<()> {
    {
        let mut captured = captured_passphrase_lock()?;
        captured.current = None;
        captured.new = None;
    }
    Ok(())
}

pub(crate) fn strip_passphrase_environment() {
    // SAFETY: every caller runs before Jig starts background threads: the
    // vault CLI capture boundary (`cli/vault_run.rs`) and the capture paths it
    // invokes, and bootstrap vault preparation before rendering. Non-vault
    // commands already withheld both variables at CLI startup through
    // `runtime::withhold_vault_passphrase_environment`. Removing both reserved
    // variables cannot affect the parent shell and prevents unrelated vault
    // operations or their child processes from inheriting stale rotation
    // material.
    unsafe {
        std::env::remove_var(PASSPHRASE_ENV);
        std::env::remove_var(NEW_PASSPHRASE_ENV);
    }
}

pub(super) fn passphrase() -> Result<SecretString> {
    let passphrase = {
        let mut captured = captured_passphrase_lock()?;
        captured.new = None;
        captured.current.take()
    };
    // Each CLI invocation dispatches exactly one vault operation after capture,
    // so consume the passphrase instead of keeping process-global key material.
    if let Some(passphrase) = passphrase {
        return Ok(passphrase);
    }
    // Reached when capture was skipped, consumed, or cleared after rejection;
    // that does not prove a missing terminal, so keep the wording neutral.
    let guidance = vault_passphrase_operator_guidance();
    Err(anyhow!(
        "no captured vault passphrase is available for this command. {guidance}"
    ))
}

fn passphrase_pair() -> Result<(SecretString, SecretString)> {
    let pair = {
        let mut captured = captured_passphrase_lock()?;
        let current = captured.current.take();
        let new = captured.new.take();
        current.zip(new)
    };
    pair.ok_or_else(|| {
        let guidance = vault_passphrase_operator_guidance();
        anyhow!(
            "no captured current and new vault passphrases are available for `jig vault passphrase change`. {guidance}"
        )
    })
}

fn captured_passphrase_lock() -> Result<MutexGuard<'static, CapturedPassphrases>> {
    CAPTURED_PASSPHRASES
        .lock()
        .map_err(|error| anyhow!("vault passphrase capture lock is poisoned: {error}"))
}

#[cfg(unix)]
fn passphrase_from_os(value: OsString, variable: &str) -> Result<SecretString> {
    SecretBytes::new(value.into_vec())
        .into_secret_string()
        .map_err(|_bytes| {
            // The rejected bytes are passphrase material; discard them instead
            // of preserving the conversion payload in diagnostics.
            anyhow!(
                "{variable} must contain valid UTF-8 for `jig vault`. {VAULT_PASSPHRASE_OPERATOR_GUIDANCE}"
            )
        })
}

#[cfg(not(unix))]
fn passphrase_from_os(value: OsString, variable: &str) -> Result<SecretString> {
    value.into_string().map(SecretString::from).map_err(|_value| {
        // The rejected value is passphrase material; discard it instead of
        // preserving the conversion payload in diagnostics.
        anyhow!(
            "{variable} must contain valid UTF-8 for `jig vault`. {VAULT_PASSPHRASE_OPERATOR_GUIDANCE}"
        )
    })
}

pub(super) fn hidden_terminal_input_available() -> bool {
    std::io::stdin().is_terminal() && std::io::stderr().is_terminal()
}

#[cfg(test)]
mod tests {
    use secrecy::ExposeSecret;

    use crate::runtime::VAULT_PASSPHRASE_WITHHELD_ENV;
    use crate::test_env::{EnvVarGuard, lock_env};

    use super::*;

    const WITHHELD_NOTE: &str = "outer Jig command withheld the vault passphrase";

    fn assert_operator_guidance(error: &str) {
        assert!(
            error.contains(VAULT_PASSPHRASE_OPERATOR_GUIDANCE),
            "{error}"
        );
        assert!(!error.contains("export "), "{error}");
    }

    #[test]
    fn operator_guidance_forbids_agent_passphrase_handling() {
        for expected in [
            PASSPHRASE_ENV,
            NEW_PASSPHRASE_ENV,
            "run the exact command in a terminal",
            "stdin and stderr attached to an interactive terminal",
            "automation outside the agent session",
            "must never request, print, store, or choose a vault passphrase",
            "must not set JIG_VAULT_PASSPHRASE or JIG_VAULT_NEW_PASSPHRASE themselves",
            "no inline VAR=value prefixes, exports, or .env files",
            "Command-line passphrases are not supported",
        ] {
            assert!(
                VAULT_PASSPHRASE_OPERATOR_GUIDANCE.contains(expected),
                "{expected}"
            );
        }
    }

    #[test]
    fn missing_captured_passphrases_route_to_the_operator() {
        let _env = lock_env();
        let _marker = EnvVarGuard::remove(VAULT_PASSPHRASE_WITHHELD_ENV);
        clear_captured_passphrase().unwrap();

        let unlock = require_captured_passphrase(PromptKind::Unlock)
            .unwrap_err()
            .to_string();
        assert!(unlock.contains("cannot prompt for the vault passphrase"));
        assert!(unlock.contains(NO_PROMPT_TERMINAL));
        assert!(!unlock.contains(WITHHELD_NOTE), "{unlock}");
        assert_operator_guidance(&unlock);

        let new_vault = require_captured_passphrase(PromptKind::NewVault)
            .unwrap_err()
            .to_string();
        assert!(new_vault.contains("operator must choose and enter a new vault passphrase"));
        assert_operator_guidance(&new_vault);

        let rotation = passphrase_change_prompt_unavailable().to_string();
        assert!(rotation.contains(PASSPHRASE_ENV));
        assert!(rotation.contains(NEW_PASSPHRASE_ENV));
        assert!(rotation.contains(NO_PROMPT_TERMINAL));
        assert_operator_guidance(&rotation);

        // Consumed or cleared captures do not prove a missing terminal.
        let consumed = passphrase().unwrap_err().to_string();
        assert!(consumed.contains("no captured vault passphrase is available"));
        assert!(!consumed.contains(NO_PROMPT_TERMINAL));
        assert_operator_guidance(&consumed);

        let pair = passphrase_pair().unwrap_err().to_string();
        assert!(pair.contains("no captured current and new vault passphrases"));
        assert!(!pair.contains(NO_PROMPT_TERMINAL));
        assert_operator_guidance(&pair);
    }

    #[test]
    fn missing_passphrase_errors_explain_an_outer_withheld_passphrase() {
        let _env = lock_env();
        let _current = EnvVarGuard::remove(PASSPHRASE_ENV);
        let _new = EnvVarGuard::remove(NEW_PASSPHRASE_ENV);
        let _marker = EnvVarGuard::set(VAULT_PASSPHRASE_WITHHELD_ENV, "1");
        clear_captured_passphrase().unwrap();

        for error in [
            require_captured_passphrase(PromptKind::Unlock).unwrap_err(),
            require_captured_passphrase(PromptKind::NewVault).unwrap_err(),
            passphrase_change_prompt_unavailable(),
            passphrase().unwrap_err(),
            passphrase_pair().unwrap_err(),
        ] {
            let error = error.to_string();
            assert!(error.contains(WITHHELD_NOTE), "{error}");
            assert_operator_guidance(&error);
        }
    }

    #[test]
    fn passphrase_clears_both_reserved_environment_values_after_reading() {
        let _env = lock_env();
        let _passphrase = EnvVarGuard::set(PASSPHRASE_ENV, "correct horse battery staple");
        let _new = EnvVarGuard::set(NEW_PASSPHRASE_ENV, "stale rotation passphrase");
        capture_passphrase_from_env().unwrap();
        let _captured = passphrase().unwrap();
        assert!(std::env::var_os(PASSPHRASE_ENV).is_none());
        assert!(std::env::var_os(NEW_PASSPHRASE_ENV).is_none());
    }

    #[test]
    fn missing_current_passphrase_strips_unused_new_environment_value() {
        let _env = lock_env();
        let _passphrase = EnvVarGuard::remove(PASSPHRASE_ENV);
        let _new = EnvVarGuard::set(NEW_PASSPHRASE_ENV, "stale rotation passphrase");

        capture_passphrase_from_env().unwrap();

        assert!(std::env::var_os(PASSPHRASE_ENV).is_none());
        assert!(std::env::var_os(NEW_PASSPHRASE_ENV).is_none());
        assert!(passphrase().is_err());
    }

    #[test]
    fn tui_capture_returns_protected_bytes_and_strips_both_environment_values() {
        let _env = lock_env();
        let _passphrase = EnvVarGuard::set(PASSPHRASE_ENV, "correct horse battery staple");
        let _new = EnvVarGuard::set(NEW_PASSPHRASE_ENV, "stale rotation passphrase");

        let captured = take_optional_tui_passphrase().unwrap().unwrap();

        assert_eq!(captured.as_slice(), b"correct horse battery staple");
        assert!(std::env::var_os(PASSPHRASE_ENV).is_none());
        assert!(std::env::var_os(NEW_PASSPHRASE_ENV).is_none());
        assert!(passphrase().is_err());
    }

    #[test]
    fn missing_tui_passphrase_still_strips_stale_rotation_value() {
        let _env = lock_env();
        let _passphrase = EnvVarGuard::remove(PASSPHRASE_ENV);
        let _new = EnvVarGuard::set(NEW_PASSPHRASE_ENV, "stale rotation passphrase");

        assert!(take_optional_tui_passphrase().unwrap().is_none());
        assert!(std::env::var_os(NEW_PASSPHRASE_ENV).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn malformed_tui_passphrase_still_strips_both_environment_values() {
        let _env = lock_env();
        let invalid = OsString::from_vec(vec![0xff, 0xfe, 0xfd]);
        let _passphrase = EnvVarGuard::set(PASSPHRASE_ENV, invalid);
        let _new = EnvVarGuard::set(NEW_PASSPHRASE_ENV, "stale rotation passphrase");

        let error = take_optional_tui_passphrase().unwrap_err().to_string();

        assert!(error.contains("valid UTF-8"));
        assert!(std::env::var_os(PASSPHRASE_ENV).is_none());
        assert!(std::env::var_os(NEW_PASSPHRASE_ENV).is_none());
    }

    #[test]
    fn passphrase_change_capture_clears_and_consumes_both_values_atomically() {
        let _env = lock_env();
        clear_captured_passphrase().unwrap();
        let _current = EnvVarGuard::set(PASSPHRASE_ENV, "correct horse battery staple");
        let _new = EnvVarGuard::set(NEW_PASSPHRASE_ENV, "new correct horse battery staple");

        capture_passphrase_pair_from_env().unwrap();

        assert!(std::env::var_os(PASSPHRASE_ENV).is_none());
        assert!(std::env::var_os(NEW_PASSPHRASE_ENV).is_none());
        let (current, new) = passphrase_pair().unwrap();
        assert_eq!(current.expose_secret(), "correct horse battery staple");
        assert_eq!(new.expose_secret(), "new correct horse battery staple");
        assert!(passphrase_pair().is_err());
    }

    #[test]
    fn passphrase_change_capture_requires_both_environment_values_without_mixing() {
        let _env = lock_env();
        clear_captured_passphrase().unwrap();
        let _current = EnvVarGuard::set(PASSPHRASE_ENV, "correct horse battery staple");
        let _new = EnvVarGuard::remove(NEW_PASSPHRASE_ENV);

        let error = capture_passphrase_pair_from_env().unwrap_err().to_string();

        assert!(error.contains(PASSPHRASE_ENV));
        assert!(error.contains(NEW_PASSPHRASE_ENV));
        assert_operator_guidance(&error);
        assert_eq!(
            std::env::var(PASSPHRASE_ENV).as_deref(),
            Ok("correct horse battery staple")
        );
        assert!(passphrase_pair().is_err());
    }

    #[cfg(unix)]
    #[test]
    fn passphrase_change_parse_error_keeps_both_environment_values_for_retry() {
        let _env = lock_env();
        clear_captured_passphrase().unwrap();
        let _current = EnvVarGuard::set(PASSPHRASE_ENV, "correct horse battery staple");
        let invalid = OsString::from_vec(vec![0xff, 0xfe, 0xfd]);
        let _new = EnvVarGuard::set(NEW_PASSPHRASE_ENV, invalid);

        let error = capture_passphrase_pair_from_env().unwrap_err().to_string();

        assert!(error.contains(NEW_PASSPHRASE_ENV));
        assert!(error.contains("valid UTF-8"));
        assert_eq!(
            std::env::var(PASSPHRASE_ENV).as_deref(),
            Ok("correct horse battery staple")
        );
        assert!(std::env::var_os(NEW_PASSPHRASE_ENV).is_some());
        assert!(passphrase_pair().is_err());
    }

    #[test]
    fn rejected_new_change_passphrase_clears_both_environment_values_and_capture() {
        let _env = lock_env();
        clear_captured_passphrase().unwrap();
        let _current = EnvVarGuard::set(PASSPHRASE_ENV, "correct horse battery staple");
        let _new = EnvVarGuard::set(NEW_PASSPHRASE_ENV, "short");

        let error = capture_passphrase_pair_from_env().unwrap_err().to_string();

        assert!(error.contains(jig_vault::NEW_VAULT_PASSPHRASE_POLICY));
        assert_operator_guidance(&error);
        assert!(!error.contains("short"));
        assert!(std::env::var_os(PASSPHRASE_ENV).is_none());
        assert!(std::env::var_os(NEW_PASSPHRASE_ENV).is_none());
        assert!(passphrase_pair().is_err());
    }

    #[test]
    fn rejected_new_passphrase_clears_captured_value() {
        let _env = lock_env();
        let _passphrase = EnvVarGuard::set(PASSPHRASE_ENV, "short");

        let error = capture_new_passphrase().unwrap_err().to_string();

        assert!(error.contains(jig_vault::NEW_VAULT_PASSPHRASE_POLICY));
        assert_operator_guidance(&error);
        assert!(std::env::var_os(PASSPHRASE_ENV).is_none());
        assert!(
            passphrase()
                .unwrap_err()
                .to_string()
                .contains(PASSPHRASE_ENV)
        );
    }

    #[cfg(unix)]
    #[test]
    fn passphrase_parse_error_keeps_environment_for_retry() {
        let _env = lock_env();
        let invalid = OsString::from_vec(vec![0xff, 0xfe, 0xfd]);
        let _passphrase = EnvVarGuard::set(PASSPHRASE_ENV, invalid);
        let _new = EnvVarGuard::set(NEW_PASSPHRASE_ENV, "preserved for retry");

        let error = capture_passphrase_from_env().unwrap_err().to_string();

        assert!(error.contains("valid UTF-8"));
        assert_operator_guidance(&error);
        assert!(std::env::var_os(PASSPHRASE_ENV).is_some());
        assert_eq!(
            std::env::var(NEW_PASSPHRASE_ENV).as_deref(),
            Ok("preserved for retry")
        );
    }
}
