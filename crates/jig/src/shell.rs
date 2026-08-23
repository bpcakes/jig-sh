use std::{
    ffi::OsStr,
    io,
    path::{Path, PathBuf},
    process::Command,
};

pub(crate) const OPTIONAL_CARGO_COMMAND_PREFIX: &str = "if [ -f Cargo.toml ]; then ";
pub(crate) const OPTIONAL_CARGO_COMMAND_ELSE: &str = "; else ";
pub(crate) const OPTIONAL_CARGO_COMMAND_SUFFIX: &str = "; fi";

pub(crate) const BASH_CONTROL_ENVIRONMENT_KEYS: [&str; 7] = [
    "BASH_ENV",
    "ENV",
    "CDPATH",
    "SHELLOPTS",
    "BASHOPTS",
    "PS4",
    "BASH_XTRACEFD",
];

pub(crate) fn sanitize_bash_environment(command: &mut Command) {
    for key in BASH_CONTROL_ENVIRONMENT_KEYS {
        command.env_remove(key);
    }

    let exported_functions = std::env::vars_os()
        .map(|(key, _)| key)
        .chain(
            command
                .get_envs()
                .filter_map(|(key, value)| value.map(|_| key.to_os_string())),
        )
        .filter(|key| is_exported_bash_function_environment_key(key))
        .collect::<Vec<_>>();
    for key in exported_functions {
        command.env_remove(key);
    }
}

pub(crate) fn is_exported_bash_function_environment_key(key: &OsStr) -> bool {
    // `OsStr`'s encoded bytes preserve ASCII boundaries on every supported
    // platform, including around non-Unicode function names on Unix.
    let key = key.as_encoded_bytes();
    key.starts_with(b"BASH_FUNC_") && key.ends_with(b"%%")
}

pub(crate) fn git_env_path(path: &Path) -> io::Result<PathBuf> {
    Ok(path.to_path_buf())
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

    #[cfg(unix)]
    use std::{fs, os::unix::ffi::OsStringExt};

    #[cfg(unix)]
    use tempfile::tempdir;

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

    #[cfg(unix)]
    #[test]
    fn bash_environment_sanitizer_blocks_startup_options_tracing_and_exported_functions() {
        let _env = crate::test_env::lock_env();
        let temp = tempdir().unwrap();
        let startup_marker = temp.path().join("startup-poison-ran");
        let trace_marker = temp.path().join("trace-poison-ran");
        let startup = temp.path().join("startup-poison.sh");
        fs::write(&startup, "printf poison > \"$JIG_STARTUP_MARKER\"\n").unwrap();
        let non_unicode_function =
            std::ffi::OsString::from_vec(b"BASH_FUNC_jig_\xff_poison%%".to_vec());

        let mut command = Command::new("bash");
        command
            .arg("-c")
            .arg(
                r#"[ -z "${BASH_ENV+x}" ] || exit 70
[ -z "${ENV+x}" ] || exit 71
[ -z "${CDPATH+x}" ] || exit 72
[ -z "${BASH_XTRACEFD+x}" ] || exit 73
case "$-" in *x*|*v*) exit 74 ;; esac
shopt -q extglob && exit 75
case "$PS4" in *JIG_PS4_POISON*) exit 76 ;; esac
[ "$JIG_ORDINARY_ENV" = preserved ] || exit 77
env | grep -Fqx 'bash_func_jig_near_miss%%=preserved' || exit 78
declare -F
printf 'clean\n'
"#,
            )
            .env("BASH_ENV", &startup)
            .env("ENV", &startup)
            .env("CDPATH", temp.path())
            .env("SHELLOPTS", "xtrace:verbose")
            .env("BASHOPTS", "extglob")
            .env(
                "PS4",
                "JIG_PS4_POISON$(printf poison > \"$JIG_TRACE_MARKER\")",
            )
            .env("BASH_XTRACEFD", "2")
            .env(
                "BASH_FUNC_jig_ascii_poison%%",
                "() { printf ascii-poison; }",
            )
            .env(&non_unicode_function, "() { printf non-unicode-poison; }")
            .env("bash_func_jig_near_miss%%", "preserved")
            .env("JIG_STARTUP_MARKER", &startup_marker)
            .env("JIG_TRACE_MARKER", &trace_marker)
            .env("JIG_ORDINARY_ENV", "preserved");
        sanitize_bash_environment(&mut command);

        let output = command.output().unwrap();

        assert!(
            output.status.success(),
            "stdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout, b"clean\n");
        assert!(
            output.stderr.is_empty(),
            "Bash trace or source leaked: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!startup_marker.exists(), "Bash startup poison executed");
        assert!(!trace_marker.exists(), "Bash PS4 poison executed");
    }

    #[cfg(unix)]
    #[test]
    fn exported_bash_function_matching_is_byte_exact_for_non_unicode_names() {
        let non_unicode = std::ffi::OsString::from_vec(b"BASH_FUNC_jig_\xff_poison%%".to_vec());

        assert!(is_exported_bash_function_environment_key(OsStr::new(
            "BASH_FUNC_jig_poison%%"
        )));
        assert!(is_exported_bash_function_environment_key(&non_unicode));
        for near_miss in [
            "bash_func_jig_poison%%",
            "XBASH_FUNC_jig_poison%%",
            "BASH_FUNC_jig_poison%",
            "BASH_FUNC_jig_poison%%suffix",
        ] {
            assert!(
                !is_exported_bash_function_environment_key(OsStr::new(near_miss)),
                "matched non-control environment key {near_miss:?}"
            );
        }
    }
}
