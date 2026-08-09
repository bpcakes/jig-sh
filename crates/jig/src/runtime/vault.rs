use std::ffi::OsString;
use std::io::{ErrorKind, IsTerminal, Read};
#[cfg(unix)]
use std::os::unix::ffi::{OsStrExt, OsStringExt};
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use anyhow::{Context, Result, anyhow, bail};
use jig_vault::{
    BrokeredEnv, BrokeredFile, BrokeredRun, FieldKind, InjectionTemplate, MAX_SECRET_VALUE_LEN,
    SecretBytes, Vault, validate_new_vault_passphrase,
};
use secrecy::SecretString;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::command::{
    VaultAuditCommand, VaultCommand, VaultFieldCommand, VaultFieldListRequest,
    VaultFieldRemoveRequest, VaultFieldSetRequest, VaultInitRequest, VaultInjectRequest,
    VaultMigrateRequest, VaultReadRequest, VaultRepoScope, VaultRunRequest, VaultRuntimeOptions,
    VaultScopeSelection, VaultSecretCommand, VaultSecretListRequest, VaultSecretRemoveRequest,
    VaultSecretSetRequest, VaultSecretValueSource, VaultStatusRequest,
};

const PASSPHRASE_ENV: &str = "JIG_VAULT_PASSPHRASE";
const VAULT_HOME_ENV: &str = "JIG_VAULT_HOME";
const VAULT_FILE_NAME: &str = "vault.json";
static CAPTURED_PASSPHRASE: Mutex<Option<SecretString>> = Mutex::new(None);

pub(crate) fn dispatch(command: VaultCommand) -> Result<Value> {
    match command {
        VaultCommand::Audit(command) => match command {
            VaultAuditCommand::Verify(request) => verify_audit(request),
        },
        VaultCommand::Init(request) => init(request),
        VaultCommand::Status(request) => status(request),
        VaultCommand::Migrate(request) => migrate(request),
        VaultCommand::Field(command) => match command {
            VaultFieldCommand::List(request) => list_fields(request),
            VaultFieldCommand::Set(request) => set_field(request),
            VaultFieldCommand::Remove(request) => remove_field(request),
        },
        VaultCommand::Inject(_) | VaultCommand::Read(_) => {
            bail!("internal error: raw vault output reached the structured dispatcher")
        }
        VaultCommand::Secret(command) => match command {
            VaultSecretCommand::List(request) => list(request),
            VaultSecretCommand::Set(request) => set(request),
            VaultSecretCommand::Remove(request) => remove(request),
        },
        VaultCommand::Run(request) => run(request),
    }
}

pub(crate) fn dispatch_raw(command: VaultCommand) -> Result<()> {
    match command {
        VaultCommand::Inject(request) => inject(request),
        VaultCommand::Read(request) => read_field(request),
        _ => bail!("internal error: structured vault command reached the raw dispatcher"),
    }
}

pub(crate) fn prepare_raw_input(command: &mut VaultCommand) -> Result<()> {
    match command {
        VaultCommand::Inject(request) => {
            let bytes = read_template_input(&request.input)?;
            request.template = Some(InjectionTemplate::parse(bytes)?);
            Ok(())
        }
        VaultCommand::Read(_) => Ok(()),
        _ => bail!("internal error: structured vault command reached raw input preparation"),
    }
}

fn read_field(request: VaultReadRequest) -> Result<()> {
    let resolved = resolve_vault_runtime(&request.vault)?;
    let vault = vault(&resolved)?;
    let passphrase = passphrase()?;
    if let Some(destination) = request.out_file {
        vault.read_field_to_file(
            &passphrase,
            request.reference,
            &destination,
            request.overwrite,
        )?;
    } else {
        let stdout = std::io::stdout();
        vault.read_field_to(&passphrase, request.reference, &mut stdout.lock())?;
    }
    Ok(())
}

fn inject(request: VaultInjectRequest) -> Result<()> {
    let template = request
        .template
        .ok_or_else(|| anyhow!("internal error: vault injection input was not prepared"))?;
    let resolved = resolve_vault_runtime(&request.vault)?;
    let vault = vault(&resolved)?;
    let passphrase = passphrase()?;
    if let Some(destination) = request.out_file {
        vault.inject_template_to_file(&passphrase, template, &destination, request.overwrite)?;
    } else {
        let stdout = std::io::stdout();
        vault.inject_template_to(&passphrase, template, &mut stdout.lock())?;
    }
    Ok(())
}

fn read_template_input(path: &Path) -> Result<SecretBytes> {
    let mut input: Box<dyn Read> = if path == Path::new("-") {
        Box::new(std::io::stdin().lock())
    } else {
        Box::new(std::fs::File::open(path).with_context(|| {
            format!("failed to open vault injection template {}", path.display())
        })?)
    };
    let capacity = jig_vault::MAX_TEMPLATE_INPUT_LEN
        .checked_add(1)
        .expect("template input limit leaves room for an overflow byte");
    let mut bytes = SecretBytes::with_capacity(capacity);
    let mut chunk = Zeroizing::new([0_u8; 8 * 1024]);
    loop {
        let remaining = capacity - bytes.len();
        let chunk_len = remaining.min(chunk.len());
        let read = match input.read(&mut chunk[..chunk_len]) {
            Ok(read) => read,
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(error) => {
                return Err(error).context("failed to read vault injection template");
            }
        };
        if read == 0 {
            break;
        }
        bytes
            .extend_from_slice(&chunk[..read])
            .expect("the bounded template buffer was preallocated exactly");
        if bytes.len() > jig_vault::MAX_TEMPLATE_INPUT_LEN {
            bail!(
                "vault injection template exceeds the {} byte limit",
                jig_vault::MAX_TEMPLATE_INPUT_LEN
            );
        }
    }
    Ok(bytes)
}

fn init(request: VaultInitRequest) -> Result<Value> {
    let resolved = resolve_vault_runtime(&request.vault)?;
    let vault = vault(&resolved)?;
    let passphrase = passphrase()?;
    vault.init(&passphrase)?;
    let mut output = json!({
        "ok": true,
        "command": "vault init",
        "vault_home": vault.root().display().to_string(),
        "created": true,
    });
    add_vault_scope_fields(&mut output, &resolved);
    Ok(output)
}

fn status(request: VaultStatusRequest) -> Result<Value> {
    let resolved = resolve_vault_runtime(&request.vault)?;
    let status = Vault::status(resolved.home.clone())?;
    let mut output = json!({
        "ok": true,
        "command": "vault status",
        "vault_home": status.root.display().to_string(),
        "exists": status.exists,
        "vault_file_exists": status.exists,
    });
    add_vault_scope_fields(&mut output, &resolved);
    Ok(output)
}

fn migrate(request: VaultMigrateRequest) -> Result<Value> {
    let resolved = resolve_vault_runtime(&request.vault)?;
    let vault = vault(&resolved)?;
    let passphrase = passphrase()?;
    let migration = vault.migrate(&passphrase, request.target_version)?;
    let mut output = json!({
        "ok": true,
        "command": "vault migrate",
        "vault_home": vault.root().display().to_string(),
        "from_version": migration.from_version,
        "to_version": migration.to_version,
        "changed": migration.changed,
    });
    add_vault_scope_fields(&mut output, &resolved);
    Ok(output)
}

fn verify_audit(request: crate::command::VaultAuditVerifyRequest) -> Result<Value> {
    let resolved = resolve_vault_runtime(&request.vault)?;
    let vault = vault(&resolved)?;
    let passphrase = passphrase()?;
    let verification = vault.verify_audit(&passphrase)?;
    let mut output = json!({
        "ok": true,
        "command": "vault audit verify",
        "vault_home": vault.root().display().to_string(),
        "event_count": verification.event_count,
        "latest_mac": verification.latest_mac,
        "torn_tail_bytes": verification.torn_tail_bytes,
    });
    add_vault_scope_fields(&mut output, &resolved);
    Ok(output)
}

fn list(request: VaultSecretListRequest) -> Result<Value> {
    let resolved = resolve_vault_runtime(&request.vault)?;
    let vault = vault(&resolved)?;
    let passphrase = passphrase()?;
    let secrets: Vec<Value> = vault
        .list(&passphrase)?
        .into_iter()
        .map(|record| {
            json!({
                "name": record.name,
                "created_at_ms": record.created_at_ms,
                "updated_at_ms": record.updated_at_ms,
                "value_len": record.value_len,
            })
        })
        .collect();
    let mut output = json!({
        "ok": true,
        "command": "vault secret list",
        "vault_home": vault.root().display().to_string(),
        "secrets": secrets,
    });
    add_vault_scope_fields(&mut output, &resolved);
    Ok(output)
}

fn list_fields(request: VaultFieldListRequest) -> Result<Value> {
    let item = request.item;
    let resolved = resolve_vault_runtime(&request.vault)?;
    let vault = vault(&resolved)?;
    let passphrase = passphrase()?;
    let fields: Vec<Value> = vault
        .list_fields(&passphrase)?
        .into_iter()
        .filter(|record| {
            item.as_ref()
                .is_none_or(|item| record.reference.item() == item.as_str())
        })
        .map(|record| {
            json!({
                "reference": record.reference.to_string(),
                "kind": field_kind_label(&record.kind),
                "created_at_ms": record.created_at_ms,
                "updated_at_ms": record.updated_at_ms,
                "value_len": record.value_len,
            })
        })
        .collect();
    let mut output = json!({
        "ok": true,
        "command": "vault field list",
        "vault_home": vault.root().display().to_string(),
        "item": item.as_ref().map(ToString::to_string),
        "fields": fields,
    });
    add_vault_scope_fields(&mut output, &resolved);
    Ok(output)
}

fn set(request: VaultSecretSetRequest) -> Result<Value> {
    let resolved = resolve_vault_runtime(&request.vault)?;
    let vault = vault(&resolved)?;
    let passphrase = passphrase()?;
    let value = match request.value_source {
        VaultSecretValueSource::Auto => {
            if std::io::stdin().is_terminal() {
                read_secret_value_from_prompt()?
            } else {
                bail!(
                    "vault secret set NAME defaults to hidden prompt only in an interactive terminal; use --value-stdin for piped or redirected input"
                );
            }
        }
        VaultSecretValueSource::Stdin => {
            let stdin = std::io::stdin();
            if stdin.is_terminal() {
                bail!(
                    "--value-stdin requires piped or redirected stdin; use --value-prompt for hidden terminal input"
                );
            }
            read_secret_value(stdin.lock())?
        }
        VaultSecretValueSource::Prompt => read_secret_value_from_prompt()?,
    };
    vault.set_secret(&passphrase, &request.name, value)?;
    let mut output = json!({
        "ok": true,
        "command": "vault secret set",
        "vault_home": vault.root().display().to_string(),
        "name": request.name,
    });
    add_vault_scope_fields(&mut output, &resolved);
    Ok(output)
}

fn set_field(request: VaultFieldSetRequest) -> Result<Value> {
    let reference = request.reference;
    let reference_text = reference.to_string();
    let kind = if request.text {
        FieldKind::Text
    } else {
        FieldKind::Concealed
    };
    let kind_label = field_kind_label(&kind);
    let resolved = resolve_vault_runtime(&request.vault)?;
    let vault = vault(&resolved)?;
    let passphrase = passphrase()?;
    let value = match request.value_source {
        VaultSecretValueSource::Auto => {
            if std::io::stdin().is_terminal() {
                read_field_value_from_prompt()?
            } else {
                bail!(
                    "vault field set REF defaults to hidden prompt only in an interactive terminal; use --value-stdin for piped or redirected input"
                );
            }
        }
        VaultSecretValueSource::Stdin => {
            let stdin = std::io::stdin();
            if stdin.is_terminal() {
                bail!(
                    "--value-stdin requires piped or redirected stdin; use --value-prompt for hidden terminal input"
                );
            }
            read_secret_value(stdin.lock())?
        }
        VaultSecretValueSource::Prompt => read_field_value_from_prompt()?,
    };
    let changed = !vault
        .set_field(&passphrase, reference, kind, value)?
        .changed
        .is_empty();
    let mut output = json!({
        "ok": true,
        "command": "vault field set",
        "vault_home": vault.root().display().to_string(),
        "reference": reference_text,
        "kind": kind_label,
        "changed": changed,
    });
    add_vault_scope_fields(&mut output, &resolved);
    Ok(output)
}

fn remove(request: VaultSecretRemoveRequest) -> Result<Value> {
    let resolved = resolve_vault_runtime(&request.vault)?;
    let vault = vault(&resolved)?;
    let passphrase = passphrase()?;
    let removed = vault.remove_secret(&passphrase, &request.name)?;
    let mut output = json!({
        "ok": true,
        "command": "vault secret remove",
        "vault_home": vault.root().display().to_string(),
        "name": request.name,
        "removed": removed,
    });
    add_vault_scope_fields(&mut output, &resolved);
    Ok(output)
}

fn remove_field(request: VaultFieldRemoveRequest) -> Result<Value> {
    let reference = request.reference;
    let reference_text = reference.to_string();
    let resolved = resolve_vault_runtime(&request.vault)?;
    let vault = vault(&resolved)?;
    let passphrase = passphrase()?;
    let removed = !vault
        .remove_field(&passphrase, reference)?
        .removed
        .is_empty();
    let mut output = json!({
        "ok": true,
        "command": "vault field remove",
        "vault_home": vault.root().display().to_string(),
        "reference": reference_text,
        "removed": removed,
    });
    add_vault_scope_fields(&mut output, &resolved);
    Ok(output)
}

fn run(request: VaultRunRequest) -> Result<Value> {
    let resolved = resolve_vault_runtime(&request.vault)?;
    let vault = vault(&resolved)?;
    let passphrase = passphrase()?;
    let env = parse_env_mappings(&request.env)?;
    let files = parse_file_mappings(&request.files)?;
    let env_mappings = request.env.len();
    let file_mappings = request.files.len();
    let output = vault.run_brokered(
        &passphrase,
        BrokeredRun::with_files(request.command, env, files)?,
    )?;
    let mut output = json!({
        "ok": output.exit_status == 0,
        "command": "vault run",
        "vault_home": vault.root().display().to_string(),
        "env_mappings": env_mappings,
        "file_mappings": file_mappings,
        "result": {
            "exit_status": output.exit_status,
            "exit_signal": output.exit_signal,
            "stdout": output.stdout,
            "stderr": output.stderr,
        },
    });
    add_vault_scope_fields(&mut output, &resolved);
    Ok(output)
}

fn vault(resolved: &ResolvedVaultRuntime) -> Result<Vault> {
    Ok(Vault::resolve(resolved.home.clone())?)
}

#[derive(Clone, Debug)]
struct ResolvedVaultRuntime {
    home: Option<PathBuf>,
    scope: &'static str,
    scope_id: Option<String>,
    repo_name: Option<String>,
}

fn resolve_vault_runtime(options: &VaultRuntimeOptions) -> Result<ResolvedVaultRuntime> {
    if let Some(home) = &options.home {
        return Ok(ResolvedVaultRuntime {
            home: Some(home.clone()),
            scope: "explicit-home",
            scope_id: None,
            repo_name: None,
        });
    }

    match &options.scope {
        VaultScopeSelection::Repo(scope) => Ok(ResolvedVaultRuntime {
            home: Some(scoped_vault_home(scope)?),
            scope: "repo",
            scope_id: Some(scope.scope_id.clone()),
            repo_name: Some(scope.repo_name.clone()),
        }),
        VaultScopeSelection::Global => Ok(ResolvedVaultRuntime {
            home: None,
            scope: "global",
            scope_id: None,
            repo_name: None,
        }),
        VaultScopeSelection::Auto => Ok(ResolvedVaultRuntime {
            home: None,
            scope: "legacy",
            scope_id: None,
            repo_name: None,
        }),
    }
}

fn scoped_vault_home(scope: &VaultRepoScope) -> Result<PathBuf> {
    if !crate::command::is_valid_vault_scope_id(&scope.scope_id) {
        bail!("invalid repo vault scope id '{}'", scope.scope_id);
    }
    let scopes_home = vault_base_home()?.join("scopes");
    let trusted_home = scopes_home.join(trusted_repo_scope_dir(scope)?);
    let legacy_home = scopes_home.join(&scope.scope_id);
    reject_legacy_repo_scope_cutover(scope, &trusted_home, &legacy_home)?;
    Ok(trusted_home)
}

fn reject_legacy_repo_scope_cutover(
    scope: &VaultRepoScope,
    trusted_home: &Path,
    legacy_home: &Path,
) -> Result<()> {
    if vault_file_exists(trusted_home)? || !vault_file_exists(legacy_home)? {
        return Ok(());
    }

    bail!(
        "legacy repo-scoped vault data exists at {}, but this Jig version now stores repo-scoped vaults in the trusted repo-local vault namespace at {} for '{}'. Refusing to treat the new namespace as empty. Move the legacy vault directory after confirming this checkout should own those secrets, or pass --home {} to inspect it explicitly",
        legacy_home.display(),
        trusted_home.display(),
        scope.repo_name,
        legacy_home.display()
    );
}

fn vault_file_exists(home: &Path) -> Result<bool> {
    let vault_file = home.join(VAULT_FILE_NAME);
    match std::fs::symlink_metadata(&vault_file) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error)
            .with_context(|| format!("failed to inspect vault file {}", vault_file.display())),
    }
}

fn trusted_repo_scope_dir(scope: &VaultRepoScope) -> Result<String> {
    let repo_root = std::fs::canonicalize(&scope.repo_root).with_context(|| {
        format!(
            "failed to canonicalize repo root for vault scope: {}",
            scope.repo_root.display()
        )
    })?;
    let mut digest = Sha256::new();
    digest.update(b"jig-vault-repo-scope-v2\0");
    #[cfg(unix)]
    digest.update(repo_root.as_os_str().as_bytes());
    #[cfg(windows)]
    for unit in repo_root.as_os_str().encode_wide() {
        digest.update(unit.to_le_bytes());
    }
    #[cfg(all(not(unix), not(windows)))]
    digest.update(repo_root.to_string_lossy().as_bytes());
    digest.update(b"\0");
    digest.update(scope.scope_id.as_bytes());
    Ok(format!("repo-{}", lower_hex(&digest.finalize())))
}

fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn vault_base_home() -> Result<PathBuf> {
    match std::env::var_os(VAULT_HOME_ENV) {
        Some(value) if value.is_empty() => bail!("{VAULT_HOME_ENV} must not be empty"),
        Some(value) => Ok(PathBuf::from(value)),
        None => Ok(dirs::home_dir()
            .context("could not resolve home directory for Jig vault")?
            .join(".jig/vault")),
    }
}

fn add_vault_scope_fields(output: &mut Value, resolved: &ResolvedVaultRuntime) {
    output["vault_scope"] = json!(resolved.scope);
    output["vault_scope_id"] = json!(resolved.scope_id.as_deref());
    output["vault_repo_name"] = json!(resolved.repo_name.as_deref());
}

pub(crate) fn capture_passphrase() -> Result<()> {
    capture_passphrase_with_prompt(PromptKind::Unlock)
}

pub(crate) fn capture_new_passphrase() -> Result<()> {
    capture_passphrase_with_prompt(PromptKind::NewVault)?;
    let validation = {
        let captured = captured_passphrase_lock()?;
        validate_new_vault_passphrase(captured.as_ref().ok_or_else(|| {
            anyhow!("vault passphrase capture unexpectedly produced no passphrase")
        })?)
    };
    if let Err(error) = validation {
        clear_captured_passphrase()?;
        return Err(error.into());
    }
    Ok(())
}

fn require_captured_passphrase() -> Result<()> {
    let passphrase_is_captured = {
        let captured = captured_passphrase_lock()?;
        captured.is_some()
    };
    if passphrase_is_captured {
        return Ok(());
    }
    Err(anyhow!(
        "{PASSPHRASE_ENV} is required for non-interactive `jig vault` commands; run from a terminal to be prompted, or export {PASSPHRASE_ENV}. Command-line passphrases are intentionally unsupported"
    ))
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
        set_captured_passphrase(passphrase)?;
        return Ok(());
    }
    capture_passphrase_from_env()?;
    require_captured_passphrase()
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
            *captured = None;
        }
        return Ok(());
    };
    // Keep a malformed environment value intact so the operator can inspect or
    // retry it. Only the successfully captured process copy is cleared below.
    let passphrase = passphrase_from_os(value)?;
    // SAFETY: `cli::run` invokes this immediately before entering
    // `runtime::dispatch_vault`, and before `Vault::run_brokered`, the only
    // vault path that spawns threads. Clearing the child process environment
    // after successful capture does not affect the parent shell's environment
    // for later invocations.
    unsafe {
        std::env::remove_var(PASSPHRASE_ENV);
    }
    {
        let mut captured = captured_passphrase_lock()?;
        *captured = Some(passphrase);
    }
    Ok(())
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

fn prompt_zeroizing(prompt: &str) -> Result<Zeroizing<String>> {
    Ok(Zeroizing::new(rpassword::prompt_password(prompt)?))
}

fn secret_string_from_zeroizing(mut value: Zeroizing<String>) -> SecretString {
    SecretString::from(std::mem::take(&mut *value))
}

fn set_captured_passphrase(passphrase: SecretString) -> Result<()> {
    {
        let mut captured = captured_passphrase_lock()?;
        *captured = Some(passphrase);
    }
    Ok(())
}

fn clear_captured_passphrase() -> Result<()> {
    {
        let mut captured = captured_passphrase_lock()?;
        *captured = None;
    }
    Ok(())
}

fn passphrase() -> Result<SecretString> {
    let passphrase = {
        let mut captured = captured_passphrase_lock()?;
        captured.take()
    };
    // Each CLI invocation dispatches exactly one vault operation after capture,
    // so consume the passphrase instead of keeping process-global key material.
    if let Some(passphrase) = passphrase {
        return Ok(passphrase);
    }
    Err(anyhow!(
        "{PASSPHRASE_ENV} is required for non-interactive `jig vault` commands; run from a terminal to be prompted, or export {PASSPHRASE_ENV}. Command-line passphrases are intentionally unsupported"
    ))
}

fn captured_passphrase_lock() -> Result<MutexGuard<'static, Option<SecretString>>> {
    CAPTURED_PASSPHRASE
        .lock()
        .map_err(|error| anyhow!("vault passphrase capture lock is poisoned: {error}"))
}

#[cfg(unix)]
fn passphrase_from_os(value: OsString) -> Result<SecretString> {
    SecretBytes::new(value.into_vec())
        .into_secret_string()
        .map_err(|_bytes| {
            // The rejected bytes are passphrase material; discard them instead
            // of preserving the conversion payload in diagnostics.
            anyhow!(
                "{PASSPHRASE_ENV} must be valid UTF-8 for `jig vault`; run from a terminal to be prompted, or export valid UTF-8. Command-line passphrases are intentionally unsupported"
            )
        })
}

#[cfg(not(unix))]
fn passphrase_from_os(value: OsString) -> Result<SecretString> {
    value.into_string().map(SecretString::from).map_err(|_value| {
        // The rejected value is passphrase material; discard it instead of
        // preserving the conversion payload in diagnostics.
        anyhow!(
            "{PASSPHRASE_ENV} must be valid UTF-8 for `jig vault`; run from a terminal to be prompted, or export valid UTF-8. Command-line passphrases are intentionally unsupported"
        )
    })
}

fn hidden_terminal_input_available() -> bool {
    std::io::stdin().is_terminal() && std::io::stderr().is_terminal()
}

fn parse_env_mappings(values: &[String]) -> Result<Vec<BrokeredEnv>> {
    values
        .iter()
        .map(|value| Ok(BrokeredEnv::parse(value)?))
        .collect()
}

fn parse_file_mappings(values: &[String]) -> Result<Vec<BrokeredFile>> {
    #[cfg(not(unix))]
    if !values.is_empty() {
        bail!(
            "vault run --file requires Unix-style owner-only temporary files; use --env on this platform"
        );
    }

    values
        .iter()
        .map(|value| BrokeredFile::parse(value).map_err(anyhow::Error::from))
        .collect()
}

fn field_kind_label(kind: &FieldKind) -> &'static str {
    match kind {
        FieldKind::Concealed => "concealed",
        FieldKind::Text => "text",
    }
}

fn read_secret_value(mut input: impl Read) -> Result<SecretBytes> {
    // Allocate the full cap up front so secret bytes from stdin do not pass
    // through discarded intermediate Vec buffers during growth.
    let mut value = SecretBytes::with_capacity(MAX_SECRET_VALUE_LEN);
    let mut buffer = Zeroizing::new([0_u8; 8192]);
    loop {
        let read = input
            .read(&mut buffer[..])
            .context("failed to read secret value from stdin")?;
        if read == 0 {
            return Ok(value);
        }
        if value.len() + read > MAX_SECRET_VALUE_LEN {
            bail!("secret value is larger than the {MAX_SECRET_VALUE_LEN} byte limit");
        }
        value.extend_from_slice(&buffer[..read])?;
    }
}

fn read_secret_value_from_prompt() -> Result<SecretBytes> {
    if !hidden_terminal_input_available() {
        bail!("--value-prompt requires an interactive terminal; use --value-stdin for automation");
    }
    let mut value =
        prompt_zeroizing("Secret value: ").context("failed to read secret value from terminal")?;
    Ok(SecretBytes::new(std::mem::take(&mut *value).into_bytes()))
}

fn read_field_value_from_prompt() -> Result<SecretBytes> {
    if !hidden_terminal_input_available() {
        bail!("--value-prompt requires an interactive terminal; use --value-stdin for automation");
    }
    let mut value =
        prompt_zeroizing("Field value: ").context("failed to read field value from terminal")?;
    Ok(SecretBytes::new(std::mem::take(&mut *value).into_bytes()))
}

#[cfg(test)]
mod tests {
    use secrecy::SecretString;
    use tempfile::tempdir;

    use crate::test_env::{EnvVarGuard, lock_env};

    use super::*;

    #[test]
    fn parses_env_mappings() {
        let parsed = parse_env_mappings(&["TOKEN=api_token".into()]).unwrap();
        assert_eq!(parsed[0].var().as_str(), "TOKEN");
        assert_eq!(parsed[0].secret_name().as_str(), "api_token");
    }

    #[test]
    fn rejects_invalid_env_mapping_shape() {
        let error = parse_env_mappings(&["TOKEN".into()])
            .unwrap_err()
            .to_string();
        assert!(error.contains("VAR=SECRET_NAME"));
    }

    #[test]
    fn rejects_invalid_env_mapping_secret_name_before_unlock() {
        let error = parse_env_mappings(&["TOKEN=bad secret".into()])
            .unwrap_err()
            .to_string();
        assert!(error.contains("unsupported characters"));
    }

    #[cfg(unix)]
    #[test]
    fn parses_file_mappings() {
        let parsed = parse_file_mappings(&["TOKEN_FILE=api_token".into()]).unwrap();
        assert_eq!(parsed[0].var().as_str(), "TOKEN_FILE");
        assert_eq!(parsed[0].secret_name().as_str(), "api_token");
    }

    #[cfg(not(unix))]
    #[test]
    fn rejects_file_mappings_on_non_unix() {
        let error = parse_file_mappings(&["TOKEN_FILE=api_token".into()])
            .unwrap_err()
            .to_string();
        assert!(error.contains("requires Unix-style owner-only temporary files"));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_invalid_file_mapping_shape() {
        let error = parse_file_mappings(&["TOKEN_FILE".into()])
            .unwrap_err()
            .to_string();
        assert!(error.contains("VAR=SECRET_NAME"));
    }

    #[test]
    fn read_secret_value_rejects_oversized_input() {
        let value = vec![b'x'; MAX_SECRET_VALUE_LEN + 1];
        let error = read_secret_value(std::io::Cursor::new(value))
            .unwrap_err()
            .to_string();
        assert!(error.contains("larger than"));
    }

    #[test]
    fn passphrase_clears_environment_after_reading() {
        let _env = lock_env();
        let _passphrase = EnvVarGuard::set(PASSPHRASE_ENV, "correct horse battery staple");
        capture_passphrase_from_env().unwrap();
        let _captured = passphrase().unwrap();
        assert!(std::env::var_os(PASSPHRASE_ENV).is_none());
    }

    #[test]
    fn rejected_new_passphrase_clears_captured_value() {
        let _env = lock_env();
        let _passphrase = EnvVarGuard::set(PASSPHRASE_ENV, "short");

        let error = capture_new_passphrase().unwrap_err().to_string();

        assert!(error.contains("at least 12 bytes"));
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
        use std::os::unix::ffi::OsStringExt;

        let _env = lock_env();
        let invalid = OsString::from_vec(vec![0xff, 0xfe, 0xfd]);
        let _passphrase = EnvVarGuard::set(PASSPHRASE_ENV, invalid);

        let error = capture_passphrase_from_env().unwrap_err().to_string();

        assert!(error.contains("valid UTF-8"));
        assert!(std::env::var_os(PASSPHRASE_ENV).is_some());
    }

    #[test]
    fn status_does_not_require_passphrase() {
        let temp = tempdir().unwrap();
        let home = temp.path().join("vault");
        let output = status(VaultStatusRequest {
            vault: VaultRuntimeOptions {
                home: Some(home.clone()),
                ..Default::default()
            },
        })
        .unwrap();
        assert_eq!(output["exists"], false);
        assert_eq!(output["vault_file_exists"], false);
        assert!(!home.exists());
    }

    #[test]
    fn status_reports_existing_vault() {
        let temp = tempdir().unwrap();
        let home = temp.path().join("vault");
        let vault = Vault::resolve(Some(home.clone())).unwrap();
        vault
            .init(&SecretString::from(
                "correct horse battery staple".to_string(),
            ))
            .unwrap();
        let output = status(VaultStatusRequest {
            vault: VaultRuntimeOptions {
                home: Some(home),
                ..Default::default()
            },
        })
        .unwrap();
        assert_eq!(output["exists"], true);
        assert_eq!(output["vault_file_exists"], true);
    }

    #[test]
    fn field_list_reports_only_metadata_and_filters_by_item() {
        let _env = lock_env();
        let temp = tempdir().unwrap();
        let home = temp.path().join("vault");
        let passphrase = "correct horse battery staple";
        let vault = Vault::resolve(Some(home.clone())).unwrap();
        let passphrase = SecretString::from(passphrase.to_owned());
        vault.init(&passphrase).unwrap();
        vault
            .set_field(
                &passphrase,
                "jig://Production/RESTIC_COMPRESSION".parse().unwrap(),
                FieldKind::Text,
                SecretBytes::new(b"false".to_vec()),
            )
            .unwrap();
        vault
            .set_field(
                &passphrase,
                "jig://Staging/ARRAY_APP_KEY".parse().unwrap(),
                FieldKind::Concealed,
                SecretBytes::new(b"test-key".to_vec()),
            )
            .unwrap();

        set_captured_passphrase(SecretString::from(
            "correct horse battery staple".to_owned(),
        ))
        .unwrap();
        let output = list_fields(VaultFieldListRequest {
            item: Some("jig://Production".parse().unwrap()),
            vault: VaultRuntimeOptions {
                home: Some(home),
                ..Default::default()
            },
        })
        .unwrap();

        assert_eq!(output["command"], "vault field list");
        assert_eq!(output["item"], "jig://Production");
        assert_eq!(output["fields"].as_array().unwrap().len(), 1);
        assert_eq!(
            output["fields"][0]["reference"],
            "jig://Production/RESTIC_COMPRESSION"
        );
        assert_eq!(output["fields"][0]["kind"], "text");
        assert_eq!(output["fields"][0]["value_len"], 5);
        assert!(output["fields"][0].get("value").is_none());
    }

    #[test]
    fn field_remove_reports_whether_a_field_existed() {
        let _env = lock_env();
        let temp = tempdir().unwrap();
        let home = temp.path().join("vault");
        let passphrase = SecretString::from("correct horse battery staple".to_owned());
        let vault = Vault::resolve(Some(home.clone())).unwrap();
        vault.init(&passphrase).unwrap();
        vault
            .set_field(
                &passphrase,
                "jig://Production/RESTIC_PASSWORD".parse().unwrap(),
                FieldKind::Concealed,
                SecretBytes::new(b"test-password".to_vec()),
            )
            .unwrap();

        set_captured_passphrase(SecretString::from(
            "correct horse battery staple".to_owned(),
        ))
        .unwrap();
        let output = remove_field(VaultFieldRemoveRequest {
            reference: "jig://Production/RESTIC_PASSWORD".parse().unwrap(),
            vault: VaultRuntimeOptions {
                home: Some(home.clone()),
                ..Default::default()
            },
        })
        .unwrap();
        assert_eq!(output["removed"], true);

        set_captured_passphrase(SecretString::from(
            "correct horse battery staple".to_owned(),
        ))
        .unwrap();
        let output = remove_field(VaultFieldRemoveRequest {
            reference: "jig://Production/RESTIC_PASSWORD".parse().unwrap(),
            vault: VaultRuntimeOptions {
                home: Some(home),
                ..Default::default()
            },
        })
        .unwrap();
        assert_eq!(output["removed"], false);
    }

    #[test]
    fn migrate_reports_when_an_already_v2_vault_is_unchanged() {
        let _env = lock_env();
        let temp = tempdir().unwrap();
        let home = temp.path().join("vault");
        let passphrase = SecretString::from("correct horse battery staple".to_owned());
        let vault = Vault::resolve(Some(home.clone())).unwrap();
        vault.init(&passphrase).unwrap();

        set_captured_passphrase(SecretString::from(
            "correct horse battery staple".to_owned(),
        ))
        .unwrap();
        let output = migrate(VaultMigrateRequest {
            target_version: 2,
            vault: VaultRuntimeOptions {
                home: Some(home),
                ..Default::default()
            },
        })
        .unwrap();

        assert_eq!(output["command"], "vault migrate");
        assert_eq!(output["from_version"], 2);
        assert_eq!(output["to_version"], 2);
        assert_eq!(output["changed"], false);
    }

    #[test]
    fn repo_scope_resolves_under_vault_base_home() {
        let _env = lock_env();
        let temp = tempdir().unwrap();
        let base = temp.path().join("vault-base");
        let repo = temp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let _home = EnvVarGuard::set(VAULT_HOME_ENV, &base);

        let output = status(VaultStatusRequest {
            vault: VaultRuntimeOptions::repo("scope_123", "demo", &repo),
        })
        .unwrap();

        assert_eq!(output["vault_scope"], "repo");
        assert_eq!(output["vault_scope_id"], "scope_123");
        assert_eq!(output["vault_repo_name"], "demo");
        let vault_home = output["vault_home"].as_str().unwrap();
        assert!(vault_home.starts_with(&base.join("scopes/repo-").display().to_string()));
        assert!(!vault_home.ends_with("scope_123"));
        assert!(!base.exists());
    }

    #[test]
    fn legacy_repo_scope_vault_blocks_trusted_namespace_cutover_until_migrated() {
        let _env = lock_env();
        let temp = tempdir().unwrap();
        let base = temp.path().join("vault-base");
        let repo = temp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let _home = EnvVarGuard::set(VAULT_HOME_ENV, &base);
        let legacy_home = base.join("scopes").join("legacy_scope");
        let legacy_vault = Vault::resolve(Some(legacy_home.clone())).unwrap();
        legacy_vault
            .init(&SecretString::from(
                "correct horse battery staple".to_string(),
            ))
            .unwrap();

        let error = status(VaultStatusRequest {
            vault: VaultRuntimeOptions::repo("legacy_scope", "demo", &repo),
        })
        .unwrap_err()
        .to_string();

        assert!(error.contains("legacy repo-scoped vault data exists"));
        assert!(error.contains("trusted repo-local vault namespace"));
        assert!(error.contains(&legacy_home.display().to_string()));
    }

    #[test]
    fn copied_scope_id_does_not_reuse_another_repo_physical_vault_home() {
        let _env = lock_env();
        let temp = tempdir().unwrap();
        let base = temp.path().join("vault-base");
        let repo_a = temp.path().join("repo-a");
        let repo_b = temp.path().join("repo-b");
        std::fs::create_dir_all(&repo_a).unwrap();
        std::fs::create_dir_all(&repo_b).unwrap();
        let _home = EnvVarGuard::set(VAULT_HOME_ENV, &base);

        let first = status(VaultStatusRequest {
            vault: VaultRuntimeOptions::repo("copied_scope", "demo", &repo_a),
        })
        .unwrap();
        let second = status(VaultStatusRequest {
            vault: VaultRuntimeOptions::repo("copied_scope", "demo", &repo_b),
        })
        .unwrap();

        assert_ne!(first["vault_home"], second["vault_home"]);
        assert_eq!(first["vault_scope_id"], "copied_scope");
        assert_eq!(second["vault_scope_id"], "copied_scope");
    }
}
