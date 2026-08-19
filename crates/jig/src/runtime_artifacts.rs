const GENERATED_RUNTIME_LAUNCHER_MARKER: &str = "# jig-generated-runtime-launcher:v1";
const GENERATED_RUNTIME_INSTALLER_MARKER: &str = "# jig-generated-runtime-installer:v1";
const RUNTIME_REPOSITORY_SCOPE_MARKER: &str = "# jig-runtime-repository-scope:v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ParsedField<T> {
    Missing,
    Malformed,
    Value(T),
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct LauncherInspection {
    legacy_version: Option<String>,
    declared_contract_version: ParsedField<u32>,
    readable_contract_version: Option<u32>,
    generated: bool,
    repository_scoped: bool,
}

impl LauncherInspection {
    pub(crate) fn legacy_version(&self) -> Option<&str> {
        self.legacy_version.as_deref()
    }

    pub(crate) fn declared_contract_version(&self) -> ParsedField<u32> {
        self.declared_contract_version
    }

    pub(crate) fn readable_contract_version(&self) -> Option<u32> {
        self.readable_contract_version
    }

    pub(crate) fn is_generated(&self) -> bool {
        self.generated
    }

    pub(crate) fn uses_repository_scope_protocol(&self) -> bool {
        self.repository_scoped
    }
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct InstallerInspection {
    generated: bool,
    repository_scoped: bool,
}

impl InstallerInspection {
    pub(crate) fn is_generated(&self) -> bool {
        self.generated
    }

    pub(crate) fn uses_repository_scope_protocol(&self) -> bool {
        self.repository_scoped
    }
}

pub(crate) fn inspect_launcher(text: &str) -> LauncherInspection {
    let mut declared_contract_version = ParsedField::Missing;
    let mut readable_contract_version = None;
    let mut saw_contract_declaration = false;

    for line in text.lines() {
        let Some(value) = line.trim().strip_prefix("CONTRACT_VERSION=").map(str::trim) else {
            continue;
        };
        let parsed = unquote_shell_value(value).parse::<u32>();
        if !saw_contract_declaration {
            declared_contract_version = parsed
                .as_ref()
                .map_or(ParsedField::Malformed, |value| ParsedField::Value(*value));
            saw_contract_declaration = true;
        }
        if readable_contract_version.is_none() {
            readable_contract_version = parsed.ok();
        }
    }

    let legacy_version = text.lines().find_map(|line| {
        line.trim()
            .strip_prefix("JIG_VERSION=")
            .map(str::trim)
            .map(unquote_shell_value)
            .map(str::to_string)
    });
    let generated = recognizable_generated_launcher(text);
    let repository_scoped = text.contains(GENERATED_RUNTIME_LAUNCHER_MARKER)
        && text.contains(RUNTIME_REPOSITORY_SCOPE_MARKER);

    LauncherInspection {
        legacy_version,
        declared_contract_version,
        readable_contract_version,
        generated,
        repository_scoped,
    }
}

pub(crate) fn inspect_installer(text: &str) -> InstallerInspection {
    InstallerInspection {
        generated: recognizable_generated_installer(text),
        repository_scoped: text.contains(GENERATED_RUNTIME_INSTALLER_MARKER)
            && text.contains(RUNTIME_REPOSITORY_SCOPE_MARKER),
    }
}

fn unquote_shell_value(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            value
                .strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
        .unwrap_or(value)
}

fn contains_all(text: &str, markers: &[&str]) -> bool {
    markers.iter().all(|marker| text.contains(marker))
}

fn recognizable_generated_launcher(text: &str) -> bool {
    text.contains(GENERATED_RUNTIME_LAUNCHER_MARKER)
        || (recognizable_posix_launcher_core(text) && recognizable_legacy_launcher_body(text))
        || (recognizable_beta_bash_launcher_core(text) && recognizable_legacy_launcher_body(text))
}

fn recognizable_posix_launcher_core(text: &str) -> bool {
    contains_all(
        text,
        &[
            "#!/bin/sh\nset -eu\n",
            "ROOT_DIR=\"$(CDPATH= cd \"$SCRIPT_DIR/..\" && pwd -P)\"",
            "INSTALLER=\"$ROOT_DIR/scripts/install-jig.sh\"",
            "exec \"$bin_path\" \"$@\"",
        ],
    )
}

fn recognizable_beta_bash_launcher_core(text: &str) -> bool {
    contains_all(
        text,
        &[
            "#!/usr/bin/env bash\nset -euo pipefail\n",
            "ROOT_DIR=\"$(cd \"$(dirname \"${BASH_SOURCE[0]}\")/..\" && pwd -P)\"",
            "INSTALLER=\"$ROOT_DIR/scripts/install-jig.sh\"",
            "exec \"$bin_path\" \"$@\"",
        ],
    )
}

fn recognizable_legacy_launcher_body(text: &str) -> bool {
    contains_all(
        text,
        &[
            "JIG_VERSION=",
            "binary_version() {",
            "use_matching_binary() {",
            "actual_version=\"$(binary_version \"$bin_path\" || true)\"",
        ],
    )
}

fn recognizable_generated_installer(text: &str) -> bool {
    text.contains(GENERATED_RUNTIME_INSTALLER_MARKER)
        || (recognizable_generated_installer_core(text) && recognizable_legacy_installer_body(text))
}

fn recognizable_generated_installer_core(text: &str) -> bool {
    contains_all(
        text,
        &[
            "#!/usr/bin/env bash\nset -euo pipefail\n",
            "ROOT_DIR=\"$(cd \"$(dirname \"${BASH_SOURCE[0]}\")/..\" && pwd -P)\"",
            "ANSWERS_FILE=\"$ROOT_DIR/.jig.toml\"",
            "acquire_install_lock() {",
            "install_from_local_source() {",
            "install_from_git_source() {",
            "printf '%s\\n' \"$BIN_PATH\"",
        ],
    )
}

fn recognizable_legacy_installer_body(text: &str) -> bool {
    contains_all(text, &["JIG_VERSION=", "assert_exact_version() {"])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launcher_inspection_preserves_strict_and_best_effort_contract_views() {
        let inspection = inspect_launcher(
            "JIG_VERSION='0.2.0-beta.1'\nCONTRACT_VERSION=invalid\nCONTRACT_VERSION=4\n",
        );

        assert_eq!(inspection.legacy_version(), Some("0.2.0-beta.1"));
        assert_eq!(
            inspection.declared_contract_version(),
            ParsedField::Malformed
        );
        assert_eq!(inspection.readable_contract_version(), Some(4));
    }

    #[test]
    fn current_protocol_recognition_requires_both_markers() {
        let launcher = inspect_launcher(&format!(
            "{GENERATED_RUNTIME_LAUNCHER_MARKER}\n{RUNTIME_REPOSITORY_SCOPE_MARKER}\n"
        ));
        let installer = inspect_installer(&format!(
            "{GENERATED_RUNTIME_INSTALLER_MARKER}\n{RUNTIME_REPOSITORY_SCOPE_MARKER}\n"
        ));

        assert!(launcher.is_generated());
        assert!(launcher.uses_repository_scope_protocol());
        assert!(installer.is_generated());
        assert!(installer.uses_repository_scope_protocol());
    }
}
