//! Usage hints for `jig init` and `jig adopt` argument errors. Each reads
//! Clap's structured error context, never its rendered text.

use clap::error::{ContextKind, ContextValue, ErrorKind};

use jig_commands::root_commands;

pub(super) const TEMPLATE_ERROR_HINT: &str = "\
Templates:
  Omit --template to use the default jig-sh harness template.
  Release builds use the official template:
  https://github.com/bpcakes/jig-sh.git
  Unreleased local builds use templates embedded in the jig binary.

If you passed --template without a value, either omit it to use the default
or provide a path/URL.

Use one of:
  jig adopt .
  jig adopt . --write
  jig init /path/to/new-repo --preset harness-only --repo-name new-repo --sqlx-enabled false --no-input --no-vault
  jig adopt . --write --template /path/to/jig-sh

Pass --template only for a local checkout, fork, or private template.";

const MISSING_INIT_PATH_HINT: &str = "\
`jig init` creates a new Jig-managed repository.
Use `jig adopt .` for an existing repository.

Use one of:
  jig init /path/to/new-repo --preset harness-only --repo-name new-repo --sqlx-enabled false --no-input --no-vault
  jig init /path/to/new-repo --preset rust-react
  jig init /path/to/new-repo --preset rust-react --db postgres --frontends web,landing,admin
  jig adopt .              # preview Jig adoption for this existing repo
  jig adopt . --write      # apply Jig adoption to this existing repo
  jig presets              # list available project scaffolds";

/// The destination argument of `jig init`, as Clap names it.
const INIT_PATH_ARG: &str = "<PATH>";

/// A hint when `--template` was given without a usable value.
pub(super) fn template_hint(error: &clap::Error) -> Option<String> {
    if !matches!(
        error.kind(),
        ErrorKind::InvalidValue | ErrorKind::TooFewValues
    ) {
        return None;
    }
    invalid_args(error)
        .any(is_template_arg)
        .then(|| TEMPLATE_ERROR_HINT.to_owned())
}

/// A hint when `jig init` is missing only its destination path.
pub(super) fn missing_init_path_hint(error: &clap::Error, root_command: &str) -> Option<String> {
    if error.kind() != ErrorKind::MissingRequiredArgument
        || root_command != root_commands::INIT.name
    {
        return None;
    }
    let mut missing = invalid_args(error);
    (missing.next() == Some(INIT_PATH_ARG) && missing.next().is_none())
        .then(|| MISSING_INIT_PATH_HINT.to_owned())
}

/// The arguments Clap names as invalid or missing for this error.
fn invalid_args(error: &clap::Error) -> impl Iterator<Item = &str> {
    let values: &[String] = match error.get(ContextKind::InvalidArg) {
        Some(ContextValue::String(value)) => std::slice::from_ref(value),
        Some(ContextValue::Strings(values)) => values,
        _ => &[],
    };
    values.iter().map(String::as_str)
}

fn is_template_arg(value: &str) -> bool {
    value
        .split_whitespace()
        .next()
        .is_some_and(|arg| arg == "--template")
}
