use std::ffi::OsString;
use std::path::PathBuf;

use clap::{ArgAction, ArgGroup, Args, Subcommand};
use jig_vault::{VaultItem, VaultReference};

use crate::tool_defs;

const VAULT_RUN_AFTER_HELP: &str = "\
The brokered command must come after --. Secrets are resolved from the local
vault and injected only into the child process environment. File delivery is
Unix-only because Jig requires 0600 secret-file permissions. Output is buffered,
then redacted. Stdout and stderr are capped at 1 MiB each; brokered runs have a
30-minute timeout.

Examples:
  jig vault run --env TOKEN=api_token -- sh -c 'printf \"%s\" \"$TOKEN\"'
  jig vault run --file TOKEN_FILE=api_token -- sh -c 'cat \"$TOKEN_FILE\"'
  jig vault run --json --env TOKEN=api_token -- sh -c 'printf \"%s\" \"$TOKEN\"'";

const VAULT_INIT_AFTER_HELP: &str = "\
Jig prompts twice for a new vault passphrase when run from a terminal. Scripts
can set JIG_VAULT_PASSPHRASE instead. Command-line passphrases are not accepted.

Examples:
  export JIG_VAULT_PASSPHRASE='choose-a-long-local-passphrase'
  jig vault init";

const VAULT_SECRET_SET_AFTER_HELP: &str = "\
Terminal use defaults to hidden input. Pass --value-prompt explicitly for the
same behavior, or --value-stdin for automation. Stdin must be piped or
redirected and is read byte-for-byte; use printf instead of echo when a
trailing newline is not part of the secret.

Examples:
  jig vault secret set api_token
  jig vault secret set api_token --value-prompt
  printf '%s' 'secret-value' | jig vault secret set api_token --value-stdin";

const VAULT_FIELD_SET_AFTER_HELP: &str = "\
Terminal use defaults to hidden input. Pass --value-prompt explicitly for the
same behavior, or --value-stdin for automation. Stdin must be piped or
redirected and is read byte-for-byte; use printf instead of echo when a
trailing newline is not part of the field value. Fields are encrypted whether
they are concealed or text; --text only prevents a contextual value from being
used as an output-redaction needle.

Examples:
  jig vault field set jig://Production/RESTIC_PASSWORD --value-prompt
  printf '%s' 'false' | jig vault field set jig://Production/RESTIC_COMPRESSION --text --value-stdin";

const VAULT_MIGRATE_AFTER_HELP: &str = "\
Upgrade an existing version 1 vault explicitly before changing fields with the
field-oriented commands. The migration is one-way and retains existing values
as concealed fields.

Example:
  jig vault migrate --to 2";

const VAULT_READ_AFTER_HELP: &str = "\
Read one encrypted field through a controlled byte-oriented output path. When
stdout is a terminal, --reveal is required. Piped or redirected stdout receives
the exact field bytes without an added newline. --out-file writes a private file
and refuses an existing destination unless --overwrite is explicit.

Examples:
  jig vault read jig://Production/RESTIC_PASSWORD | command
  jig vault read jig://Production/RESTIC_PASSWORD --reveal
  jig vault read jig://Production/RESTIC_PASSWORD --out-file ./password.txt";

const VAULT_INJECT_AFTER_HELP: &str = "\
Replace only {{ jig://ITEM/FIELD }} placeholders in a bounded template. Pass
--in - explicitly to read the template from stdin. When stdout is a terminal,
--reveal is required. Piped or redirected stdout receives the exact rendered
bytes without an added newline. --out-file uses the same private-file and
explicit-overwrite rules as vault read.

Examples:
  jig vault inject --in config.template > config
  jig vault inject --in - < config.template > config
  jig vault inject --in config.template --out-file config --overwrite";

const VAULT_EXEC_AFTER_HELP: &str = "\
Exec is the transparent developer-process wrapper. It inherits stdin and the
ordinary parent environment, applies the exact assignments from --env-file,
streams redacted stdout and stderr without a Jig timeout or output cap, and
mirrors the child exit status. It invokes COMMAND directly, never through a
shell. The command and all arguments must follow --.

The dotenv grammar is intentionally small: blank lines and full-line comments
(with optional leading spaces or tabs) are accepted; assignments must be exact
NAME=VALUE lines with no whitespace around NAME or '='. Unquoted values accept
only \\\\, \\ , \\#, \\', \\\", \\n, \\r, and \\t escapes; raw whitespace, #, and quotes
are rejected. Single quotes preserve their contents literally. Double quotes
accept only \\\\, \\\", \\n, \\r, and \\t escapes. Within env-file values, dollar
signs and backticks are always rejected, so interpolation and command
substitution never run. Use jig://ITEM/FIELD as the entire decoded value to
bind a vault field.

Unlike exec, the older vault run command injects selected legacy secret names
into a cleaned, closed-stdin child with buffered/capped output, a timeout, and
owned process-tree cleanup.

Example:
  jig vault exec --env-file .env.jig -- sh -c 'printf \"%s\" \"$TOKEN\"'";

#[derive(Debug, Subcommand)]
pub(crate) enum VaultCommand {
    /// Inspect or verify the local vault audit log.
    #[command(name = tool_defs::cli_command::VAULT_AUDIT, subcommand)]
    Audit(VaultAuditCommand),
    /// Create a local encrypted vault.
    #[command(
        name = tool_defs::cli_command::VAULT_INIT,
        after_help = VAULT_INIT_AFTER_HELP
    )]
    Init(VaultInitOpts),
    /// Inspect local vault presence without decrypting values.
    #[command(name = tool_defs::cli_command::VAULT_STATUS)]
    Status(VaultStatusOpts),
    /// Explicitly upgrade an existing vault format.
    #[command(
        name = tool_defs::cli_command::VAULT_MIGRATE,
        after_help = VAULT_MIGRATE_AFTER_HELP
    )]
    Migrate(VaultMigrateOpts),
    /// Add, list, or remove encrypted vault fields.
    #[command(name = tool_defs::cli_command::VAULT_FIELD, subcommand)]
    Field(VaultFieldCommand),
    /// Transparently execute a command using a restricted reference-aware dotenv file.
    #[command(
        name = tool_defs::cli_command::VAULT_EXEC,
        after_help = VAULT_EXEC_AFTER_HELP
    )]
    Exec(VaultExecOpts),
    /// Render a template containing canonical vault field references.
    #[command(
        name = tool_defs::cli_command::VAULT_INJECT,
        after_help = VAULT_INJECT_AFTER_HELP
    )]
    Inject(VaultInjectOpts),
    /// Read one encrypted field through a controlled output sink.
    #[command(
        name = tool_defs::cli_command::VAULT_READ,
        after_help = VAULT_READ_AFTER_HELP
    )]
    Read(VaultReadOpts),
    /// Add, list, or remove vault secrets.
    #[command(name = tool_defs::cli_command::VAULT_SECRET, subcommand)]
    Secret(VaultSecretCommand),
    /// Run a command with selected secrets injected and output redacted.
    #[command(name = tool_defs::cli_command::VAULT_RUN, after_help = VAULT_RUN_AFTER_HELP)]
    Run(VaultRunOpts),
}

#[derive(Debug, Subcommand)]
pub(crate) enum VaultAuditCommand {
    /// Verify the local tamper-evident audit chain.
    #[command(name = tool_defs::cli_command::VAULT_AUDIT_VERIFY)]
    Verify(VaultAuditVerifyOpts),
}

#[derive(Debug, Subcommand)]
pub(crate) enum VaultSecretCommand {
    /// List secret metadata without values.
    #[command(name = tool_defs::cli_command::VAULT_SECRET_LIST)]
    List(VaultSecretListOpts),
    /// Set a secret value.
    #[command(
        name = tool_defs::cli_command::VAULT_SECRET_SET,
        after_help = VAULT_SECRET_SET_AFTER_HELP
    )]
    Set(VaultSecretSetOpts),
    /// Remove a secret from the vault.
    #[command(name = tool_defs::cli_command::VAULT_SECRET_REMOVE)]
    Remove(VaultSecretRemoveOpts),
}

#[derive(Debug, Subcommand)]
pub(crate) enum VaultFieldCommand {
    /// List field metadata without values, optionally limited to one item.
    #[command(name = tool_defs::cli_command::VAULT_FIELD_LIST)]
    List(VaultFieldListOpts),
    /// Set an encrypted field value.
    #[command(
        name = tool_defs::cli_command::VAULT_FIELD_SET,
        after_help = VAULT_FIELD_SET_AFTER_HELP
    )]
    Set(VaultFieldSetOpts),
    /// Remove an encrypted field from the vault.
    #[command(name = tool_defs::cli_command::VAULT_FIELD_REMOVE)]
    Remove(VaultFieldRemoveOpts),
}

#[derive(Args, Clone, Debug, Default)]
pub(crate) struct VaultRuntimeOpts {
    #[arg(
        long,
        help = "Vault home directory; explicit physical override that bypasses repo scoping and allow_global checks"
    )]
    pub(crate) home: Option<PathBuf>,
    #[arg(
        long,
        conflicts_with = "home",
        help = "Use the user-level global vault instead of the current repo vault scope"
    )]
    pub(crate) global: bool,
}

#[derive(Args, Debug, Default)]
pub(crate) struct VaultInitOpts {
    #[command(flatten)]
    pub(crate) vault: VaultRuntimeOpts,
}

#[derive(Args, Debug, Default)]
pub(crate) struct VaultStatusOpts {
    #[command(flatten)]
    pub(crate) vault: VaultRuntimeOpts,
}

#[derive(Args, Debug)]
pub(crate) struct VaultMigrateOpts {
    #[arg(
        long,
        value_parser = parse_vault_migration_target,
        help = "Vault format version to migrate to; currently only 2 is supported"
    )]
    pub(crate) to: u32,
    #[command(flatten)]
    pub(crate) vault: VaultRuntimeOpts,
}

#[derive(Args, Debug, Default)]
pub(crate) struct VaultAuditVerifyOpts {
    #[command(flatten)]
    pub(crate) vault: VaultRuntimeOpts,
}

#[derive(Args, Debug, Default)]
pub(crate) struct VaultSecretListOpts {
    #[command(flatten)]
    pub(crate) vault: VaultRuntimeOpts,
}

#[derive(Args, Debug, Default)]
pub(crate) struct VaultFieldListOpts {
    #[arg(
        value_parser = parse_vault_item,
        value_name = "jig://ITEM",
        help = "Optional canonical item selector; list all fields when omitted"
    )]
    pub(crate) item: Option<VaultItem>,
    #[command(flatten)]
    pub(crate) vault: VaultRuntimeOpts,
}

#[derive(Args, Debug)]
#[command(group(
    ArgGroup::new("value_source")
        .args(["value_stdin", "value_prompt"])
))]
pub(crate) struct VaultSecretSetOpts {
    #[arg(help = "Secret name to set; names appear in local audit metadata")]
    pub(crate) name: String,
    #[arg(
        long = "value-stdin",
        action = ArgAction::SetTrue,
        help = "Read a 4 byte to 1 MiB secret value from stdin and store the bytes exactly as provided; the 4 byte minimum keeps redaction matchable"
    )]
    pub(crate) value_stdin: bool,
    #[arg(
        long = "value-prompt",
        action = ArgAction::SetTrue,
        help = "Prompt for a UTF-8 secret value with hidden terminal input; no trailing newline is stored"
    )]
    pub(crate) value_prompt: bool,
    #[command(flatten)]
    pub(crate) vault: VaultRuntimeOpts,
}

#[derive(Args, Debug)]
#[command(group(
    ArgGroup::new("value_source")
        .args(["value_stdin", "value_prompt"])
))]
pub(crate) struct VaultFieldSetOpts {
    #[arg(
        value_parser = parse_vault_reference,
        help = "Canonical field reference jig://ITEM/FIELD; names appear in local audit metadata"
    )]
    pub(crate) reference: VaultReference,
    #[arg(
        long,
        action = ArgAction::SetTrue,
        help = "Store the encrypted field as contextual text rather than a concealed redaction needle"
    )]
    pub(crate) text: bool,
    #[arg(
        long = "value-stdin",
        action = ArgAction::SetTrue,
        help = "Read an exact field value from stdin; concealed fields require at least 4 bytes, while text fields may be empty"
    )]
    pub(crate) value_stdin: bool,
    #[arg(
        long = "value-prompt",
        action = ArgAction::SetTrue,
        help = "Prompt for a UTF-8 field value with hidden terminal input; no trailing newline is stored"
    )]
    pub(crate) value_prompt: bool,
    #[command(flatten)]
    pub(crate) vault: VaultRuntimeOpts,
}

#[derive(Args, Debug)]
pub(crate) struct VaultSecretRemoveOpts {
    #[arg(help = "Secret name to remove; names appear in local audit metadata")]
    pub(crate) name: String,
    #[command(flatten)]
    pub(crate) vault: VaultRuntimeOpts,
}

#[derive(Args, Debug)]
pub(crate) struct VaultFieldRemoveOpts {
    #[arg(
        value_parser = parse_vault_reference,
        help = "Canonical field reference jig://ITEM/FIELD; names appear in local audit metadata"
    )]
    pub(crate) reference: VaultReference,
    #[command(flatten)]
    pub(crate) vault: VaultRuntimeOpts,
}

#[derive(Args, Debug)]
pub(crate) struct VaultReadOpts {
    #[arg(
        value_parser = parse_vault_reference,
        help = "Canonical field reference jig://ITEM/FIELD; names appear in local audit metadata"
    )]
    pub(crate) reference: VaultReference,
    #[arg(
        long,
        action = ArgAction::SetTrue,
        conflicts_with = "out_file",
        help = "Allow exact field bytes to be written directly to an interactive terminal"
    )]
    pub(crate) reveal: bool,
    #[arg(
        long,
        value_name = "PATH",
        help = "Write exact field bytes to a private file instead of stdout"
    )]
    pub(crate) out_file: Option<PathBuf>,
    #[arg(
        long,
        action = ArgAction::SetTrue,
        requires = "out_file",
        help = "Atomically replace an existing --out-file destination"
    )]
    pub(crate) overwrite: bool,
    #[command(flatten)]
    pub(crate) vault: VaultRuntimeOpts,
}

#[derive(Args, Debug)]
pub(crate) struct VaultInjectOpts {
    #[arg(
        long = "in",
        value_name = "PATH|-",
        required = true,
        help = "Bounded template path, or - to read the template explicitly from stdin"
    )]
    pub(crate) input: PathBuf,
    #[arg(
        long,
        action = ArgAction::SetTrue,
        conflicts_with = "out_file",
        help = "Allow exact rendered bytes to be written directly to an interactive terminal"
    )]
    pub(crate) reveal: bool,
    #[arg(
        long,
        value_name = "PATH",
        help = "Write exact rendered bytes to a private file instead of stdout"
    )]
    pub(crate) out_file: Option<PathBuf>,
    #[arg(
        long,
        action = ArgAction::SetTrue,
        requires = "out_file",
        help = "Atomically replace an existing --out-file destination"
    )]
    pub(crate) overwrite: bool,
    #[command(flatten)]
    pub(crate) vault: VaultRuntimeOpts,
}

#[derive(Args)]
pub(crate) struct VaultExecOpts {
    #[arg(
        long,
        value_name = "FILE",
        help = "Restricted UTF-8 dotenv file; - is rejected so the child can inherit stdin"
    )]
    pub(crate) env_file: PathBuf,
    #[command(flatten)]
    pub(crate) vault: VaultRuntimeOpts,
    #[arg(
        last = true,
        allow_hyphen_values = true,
        required = true,
        help = "Command and arguments to execute directly after --"
    )]
    pub(crate) command: Vec<OsString>,
}

impl std::fmt::Debug for VaultExecOpts {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VaultExecOpts")
            .field("env_file", &self.env_file)
            .field("vault", &self.vault)
            .field("command", &"[REDACTED]")
            .field("command_len", &self.command.len())
            .finish()
    }
}

#[derive(Args, Debug)]
#[command(group(
    ArgGroup::new("secret_source")
        .args(["env", "files"])
        .required(true)
        .multiple(true)
))]
pub(crate) struct VaultRunOpts {
    #[arg(
        long = "env",
        help = "Environment mapping VAR=SECRET_NAME; VAR must match [A-Za-z_][A-Za-z0-9_]* and must not be a preserved process variable such as PATH or HOME; may be repeated"
    )]
    pub(crate) env: Vec<String>,
    #[arg(
        long = "file",
        help = "File mapping VAR=SECRET_NAME; writes the secret to a private temp file (0600 on Unix) and injects its path as VAR; may be repeated"
    )]
    pub(crate) files: Vec<String>,
    #[command(flatten)]
    pub(crate) vault: VaultRuntimeOpts,
    #[arg(
        last = true,
        allow_hyphen_values = true,
        required = true,
        help = "Command to run after --"
    )]
    pub(crate) command: Vec<String>,
}

fn parse_vault_reference(value: &str) -> Result<VaultReference, String> {
    value
        .parse::<VaultReference>()
        .map_err(|error| error.to_string())
}

fn parse_vault_item(value: &str) -> Result<VaultItem, String> {
    value
        .parse::<VaultItem>()
        .map_err(|error| error.to_string())
}

fn parse_vault_migration_target(value: &str) -> Result<u32, String> {
    let target = value
        .parse::<u32>()
        .map_err(|_| "vault migration target must be the integer 2".to_owned())?;
    if target == 2 {
        Ok(target)
    } else {
        Err("only vault migration target 2 is supported".to_owned())
    }
}
