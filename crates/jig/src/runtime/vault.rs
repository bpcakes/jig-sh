use std::io::{ErrorKind, IsTerminal, Read};
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use jig_vault::{
    BrokeredEnv, BrokeredFile, BrokeredRun, EnvVarName, ExecEnvBinding, FieldKind,
    InjectionTemplate, MAX_SECRET_VALUE_LEN, PreparedPrivateFile, SecretBytes, Vault, VaultExec,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::command::{
    VaultAuditCommand, VaultBackupCommand, VaultCommand, VaultExecRequest, VaultExecValue,
    VaultFieldCommand, VaultFieldListRequest, VaultFieldRemoveRequest, VaultFieldSetRequest,
    VaultImportCommand, VaultImportOnePasswordRequest, VaultInitRequest, VaultInjectRequest,
    VaultMigrateRequest, VaultPassphraseCommand, VaultReadRequest, VaultRepoScope, VaultRunRequest,
    VaultRuntimeOptions, VaultScopeSelection, VaultSecretCommand, VaultSecretListRequest,
    VaultSecretRemoveRequest, VaultSecretSetRequest, VaultSecretValueSource, VaultStatusRequest,
};

use super::VaultRawOutcome;

const VAULT_HOME_ENV: &str = "JIG_VAULT_HOME";
const VAULT_FILE_NAME: &str = "vault.json";

mod lifecycle;

#[cfg(test)]
use lifecycle::set_captured_passphrase;
pub(crate) use lifecycle::{
    capture_new_passphrase, capture_passphrase, capture_passphrase_change, passphrase_env_present,
    passphrase_prompt_available, preflight_scoped_command, strip_passphrase_environment,
};
use lifecycle::{
    change_passphrase, create_backup, hidden_terminal_input_available, passphrase,
    prompt_zeroizing, restore_backup,
};
type VaultResolver = fn(Option<PathBuf>) -> jig_vault::Result<Vault>;

#[cfg(not(test))]
pub(crate) fn dispatch(command: VaultCommand) -> Result<Value> {
    dispatch_with_resolver(command, Vault::resolve)
}

#[cfg(test)]
pub(crate) fn dispatch_for_test(command: VaultCommand) -> Result<Value> {
    dispatch_with_resolver(command, Vault::resolve_for_test)
}

fn dispatch_with_resolver(command: VaultCommand, resolver: VaultResolver) -> Result<Value> {
    match command {
        VaultCommand::Audit(command) => match command {
            VaultAuditCommand::Verify(request) => verify_audit(request, resolver),
        },
        VaultCommand::Backup(command) => match command {
            VaultBackupCommand::Create(request) => create_backup(*request),
            VaultBackupCommand::Restore(request) => restore_backup(*request),
        },
        VaultCommand::Init(request) => init(request, resolver),
        VaultCommand::Status(request) => status(request),
        VaultCommand::Migrate(request) => migrate(request),
        VaultCommand::Passphrase(VaultPassphraseCommand::Change(request)) => {
            change_passphrase(request)
        }
        VaultCommand::Field(command) => match command {
            VaultFieldCommand::List(request) => list_fields(request),
            VaultFieldCommand::Set(request) => set_field(request),
            VaultFieldCommand::Remove(request) => remove_field(request),
        },
        VaultCommand::Exec(_) | VaultCommand::Inject(_) | VaultCommand::Read(_) => {
            bail!("internal error: raw vault output reached the structured dispatcher")
        }
        VaultCommand::Import(VaultImportCommand::OnePassword(request)) => {
            import_onepassword(request)
        }
        VaultCommand::Secret(command) => match command {
            VaultSecretCommand::List(request) => list(request, resolver),
            VaultSecretCommand::Set(request) => set(request, resolver),
            VaultSecretCommand::Remove(request) => remove(request, resolver),
        },
        VaultCommand::Run(request) => run(request, resolver),
    }
}

pub(crate) fn dispatch_raw(command: VaultCommand) -> Result<VaultRawOutcome> {
    match command {
        VaultCommand::Exec(request) => exec(request),
        VaultCommand::Inject(request) => inject(request).map(|()| VaultRawOutcome::Complete),
        VaultCommand::Read(request) => read_field(request).map(|()| VaultRawOutcome::Complete),
        _ => bail!("internal error: structured vault command reached the raw dispatcher"),
    }
}

pub(crate) fn prepare_raw_input(command: &mut VaultCommand) -> Result<()> {
    match command {
        VaultCommand::Exec(request) => {
            request.environment = Some(super::vault_env::parse_vault_env_file(&request.env_file)?);
            Ok(())
        }
        VaultCommand::Inject(request) => {
            let bytes = read_template_input(&request.input)?;
            request.template = Some(InjectionTemplate::parse(bytes)?);
            Ok(())
        }
        VaultCommand::Import(VaultImportCommand::OnePassword(request)) => {
            request.environment = Some(super::vault_env::parse_onepassword_env_file(
                &request.env_file,
                &request.item,
            )?);
            let destination_exists = super::vault_import::preflight_destination(&request.out_env)?;
            if destination_exists && !request.overwrite && !request.dry_run {
                bail!(
                    "vault import destination {} already exists; pass --overwrite to replace it atomically",
                    request.out_env.display()
                );
            }
            PreparedPrivateFile::preflight(
                &request.out_env,
                request.overwrite || (request.dry_run && destination_exists),
            )?;
            request.destination_exists = Some(destination_exists);
            Ok(())
        }
        VaultCommand::Read(_) => Ok(()),
        _ => bail!("internal error: structured vault command reached raw input preparation"),
    }
}

fn import_onepassword(mut request: VaultImportOnePasswordRequest) -> Result<Value> {
    let environment = request.environment.take().ok_or_else(|| {
        anyhow!("internal error: vault onepassword import input was not prepared")
    })?;
    let destination_exists = request.destination_exists.ok_or_else(|| {
        anyhow!("internal error: vault onepassword import destination was not preflighted")
    })?;
    let entries = super::vault_import::import_entries(&environment);
    let references = entries
        .iter()
        .map(|entry| entry.reference.clone())
        .collect::<Vec<_>>();
    let resolved = resolve_vault_runtime(&request.vault)?;
    let vault = vault(&resolved)?;
    // Compute the exact recovery command before unlock, external resolution,
    // or mutation. This rejects non-UTF-8 path metadata rather than emitting a
    // lossy command only after a post-commit destination failure.
    let recovery_command = onepassword_import_recovery_command(&request, vault.root())?;
    let passphrase = passphrase()?;
    let existing = vault.preview_import_fields(&passphrase, &references)?;

    if request.dry_run {
        let fields = entries
            .iter()
            .zip(&existing)
            .map(|(entry, exists)| {
                json!({
                    "variable": entry.name,
                    "reference": entry.reference.to_string(),
                    "kind": field_kind_label(&entry.kind),
                    "action": if *exists { "replace" } else { "create" },
                })
            })
            .collect::<Vec<_>>();
        let mut output = json!({
            "ok": true,
            "command": "vault import onepassword",
            "dry_run": true,
            "vault_home": vault.root().display().to_string(),
            "source": request.env_file.display().to_string(),
            "destination": request.out_env.display().to_string(),
            "destination_action": if destination_exists { "replace" } else { "create" },
            "requires_overwrite": destination_exists && !request.overwrite,
            "requires_replace": existing.iter().any(|exists| *exists) && !request.replace,
            "fields": fields,
        });
        add_vault_scope_fields(&mut output, &resolved);
        return Ok(output);
    }

    if !request.replace {
        if let Some((entry, _)) = entries.iter().zip(&existing).find(|(_, exists)| **exists) {
            bail!(
                "vault field '{}' already exists; pass --replace to replace existing import fields",
                entry.reference
            );
        }
    }

    let imported = super::vault_import::resolve_import(environment)?;
    let prepared =
        PreparedPrivateFile::prepare(&request.out_env, imported.destination, request.overwrite)?;
    let result = vault.import_fields(&passphrase, imported.mutations, request.replace)?;
    if let Err(error) = prepared.install() {
        bail!(
            "vault import succeeded, but destination installation failed: {error}. Safe rerun: {}",
            recovery_command
        );
    }

    let fields = imported
        .entries
        .iter()
        .map(|entry| {
            json!({
                "variable": entry.name,
                "reference": entry.reference.to_string(),
                "kind": field_kind_label(&entry.kind),
            })
        })
        .collect::<Vec<_>>();
    let mut output = json!({
        "ok": true,
        "command": "vault import onepassword",
        "dry_run": false,
        "vault_home": vault.root().display().to_string(),
        "source": request.env_file.display().to_string(),
        "destination": request.out_env.display().to_string(),
        "changed": result.changed.iter().map(ToString::to_string).collect::<Vec<_>>(),
        "fields": fields,
    });
    add_vault_scope_fields(&mut output, &resolved);
    Ok(output)
}

fn onepassword_import_recovery_command(
    request: &VaultImportOnePasswordRequest,
    vault_home: &Path,
) -> Result<String> {
    let source = exact_recovery_path("source", &request.env_file)?;
    let destination = exact_recovery_path("destination", &request.out_env)?;
    let vault_home = exact_recovery_path("vault home", vault_home)?;
    Ok([
        "jig".to_owned(),
        "vault".to_owned(),
        "import".to_owned(),
        "onepassword".to_owned(),
        "--env-file".to_owned(),
        shell_quote(source),
        "--item".to_owned(),
        shell_quote(request.item.as_str()),
        "--out-env".to_owned(),
        shell_quote(destination),
        "--replace".to_owned(),
        "--overwrite".to_owned(),
        "--home".to_owned(),
        shell_quote(vault_home),
    ]
    .join(" "))
}

fn exact_recovery_path<'a>(label: &str, path: &'a Path) -> Result<&'a str> {
    path.to_str().ok_or_else(|| {
        anyhow!(
            "vault import {label} path is not valid UTF-8; choose a UTF-8 path so a post-commit recovery command can be exact"
        )
    })
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn exec(request: VaultExecRequest) -> Result<VaultRawOutcome> {
    let environment = request
        .environment
        .ok_or_else(|| anyhow!("internal error: vault exec environment was not prepared"))?;
    let mut bindings = Vec::with_capacity(environment.assignments.len());
    for assignment in environment.assignments {
        let variable = EnvVarName::parse(&assignment.name).map_err(|_| {
            anyhow!(
                "internal error: validated vault exec variable '{}' from line {} was rejected",
                assignment.name,
                assignment.line
            )
        })?;
        let binding = match assignment.value {
            VaultExecValue::Literal(value) => ExecEnvBinding::literal(variable, value)?,
            VaultExecValue::Field(reference) => ExecEnvBinding::field(variable, reference),
        };
        bindings.push(binding);
    }
    let execution = VaultExec::new(request.command, bindings)?;
    let resolved = resolve_vault_runtime(&request.vault)?;
    let vault = vault(&resolved)?;
    let passphrase = passphrase()?;
    let outcome = vault.exec(&passphrase, execution)?;
    Ok(VaultRawOutcome::ChildExit(outcome.exit_status))
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

fn init(request: VaultInitRequest, resolver: VaultResolver) -> Result<Value> {
    let resolved = resolve_vault_runtime(&request.vault)?;
    let vault = vault_with_resolver(&resolved, resolver)?;
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

fn verify_audit(
    request: crate::command::VaultAuditVerifyRequest,
    resolver: VaultResolver,
) -> Result<Value> {
    let resolved = resolve_vault_runtime(&request.vault)?;
    let vault = vault_with_resolver(&resolved, resolver)?;
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

fn list(request: VaultSecretListRequest, resolver: VaultResolver) -> Result<Value> {
    let resolved = resolve_vault_runtime(&request.vault)?;
    let vault = vault_with_resolver(&resolved, resolver)?;
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

fn set(request: VaultSecretSetRequest, resolver: VaultResolver) -> Result<Value> {
    let resolved = resolve_vault_runtime(&request.vault)?;
    let vault = vault_with_resolver(&resolved, resolver)?;
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

fn remove(request: VaultSecretRemoveRequest, resolver: VaultResolver) -> Result<Value> {
    let resolved = resolve_vault_runtime(&request.vault)?;
    let vault = vault_with_resolver(&resolved, resolver)?;
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

fn run(request: VaultRunRequest, resolver: VaultResolver) -> Result<Value> {
    let resolved = resolve_vault_runtime(&request.vault)?;
    let vault = vault_with_resolver(&resolved, resolver)?;
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
    vault_with_resolver(resolved, Vault::resolve)
}

fn vault_with_resolver(resolved: &ResolvedVaultRuntime, resolver: VaultResolver) -> Result<Vault> {
    Ok(resolver(resolved.home.clone())?)
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
        let vault = Vault::resolve_for_test(Some(home.clone())).unwrap();
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
        let legacy_vault = Vault::resolve_for_test(Some(legacy_home.clone())).unwrap();
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
