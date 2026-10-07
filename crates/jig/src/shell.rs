use std::{
    io,
    path::{Path, PathBuf},
};

pub(crate) const OPTIONAL_CARGO_COMMAND_PREFIX: &str = "if [ -f Cargo.toml ]; then ";
pub(crate) const OPTIONAL_CARGO_COMMAND_ELSE: &str = "; else ";
pub(crate) const OPTIONAL_CARGO_COMMAND_SUFFIX: &str = "; fi";

pub(crate) fn git_env_path(path: &Path) -> io::Result<PathBuf> {
    Ok(path.to_path_buf())
}

pub(crate) fn optional_cargo_command(command: &str, label: &str) -> String {
    let skip_prefix = crate::CARGO_SKIP_OUTPUT_PREFIX;
    let skip_message = quote(&format!("{skip_prefix}{label}."));
    // Runtime command dispatch sets CWD to the repo root, so this guard checks
    // for a root Cargo workspace without blocking harness-only repos.
    format!(
        "{}{command}{}printf '%s\\n' {skip_message}{}",
        crate::shell::OPTIONAL_CARGO_COMMAND_PREFIX,
        crate::shell::OPTIONAL_CARGO_COMMAND_ELSE,
        crate::shell::OPTIONAL_CARGO_COMMAND_SUFFIX,
    )
}

pub(crate) fn optional_cargo_command_branches(command: &str) -> Option<(&str, &str)> {
    let body = command.strip_prefix(OPTIONAL_CARGO_COMMAND_PREFIX)?;
    let body = body.strip_suffix(OPTIONAL_CARGO_COMMAND_SUFFIX)?;
    body.split_once(OPTIONAL_CARGO_COMMAND_ELSE)
}

pub(crate) fn quote(value: &str) -> String {
    if !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '_' | '-' | ':'))
    {
        return value.to_string();
    }
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quote_handles_shell_special_characters() {
        assert_eq!(quote("scripts/jig"), "scripts/jig");
        assert_eq!(
            quote("https://example.test/path"),
            "https://example.test/path"
        );
        assert_eq!(quote("path+suffix"), "'path+suffix'");
        assert_eq!(quote(""), "''");
        assert_eq!(quote("path with space"), "'path with space'");
        assert_eq!(quote("team's path"), "'team'\\''s path'");
    }

    #[test]
    fn optional_cargo_command_branches_requires_full_wrapper() {
        let command = format!(
            "{OPTIONAL_CARGO_COMMAND_PREFIX}cargo test{OPTIONAL_CARGO_COMMAND_ELSE}printf skipped{OPTIONAL_CARGO_COMMAND_SUFFIX}"
        );
        assert_eq!(
            optional_cargo_command_branches(&command),
            Some(("cargo test", "printf skipped"))
        );
        assert!(optional_cargo_command_branches(&(command + " trailing")).is_none());
    }
}
