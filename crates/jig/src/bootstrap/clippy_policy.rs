pub(crate) const DEFAULT_RUST_CLIPPY_COMMAND: &str = "cargo clippy --workspace --all-targets --all-features --locked -- -D warnings -D clippy::mod_module_files";

pub(super) const NESTED_MANIFEST_RUST_CLIPPY_COMMAND: &str = "cargo clippy --manifest-path \"$jig_manifest\" --all-targets --all-features -- -D warnings -D clippy::mod_module_files";

pub(super) const ALL_FEATURES_ADOPTION_WARNING: &str = "The generated Clippy command checks all Cargo features. If this repository has mutually exclusive features, remove only `--all-features` from the effective Clippy command in .jig.toml; later updates preserve that customization.";

const LEGACY_RUST_CLIPPY_COMMAND: &str =
    "cargo clippy --workspace --all-targets --locked -- -D warnings";
const LEGACY_NESTED_MANIFEST_RUST_CLIPPY_COMMAND: &str =
    "cargo clippy --manifest-path \"$jig_manifest\" --all-targets -- -D warnings";

const NESTED_COMMAND_PREFIX: &str = "( found=0; rc=0";
const NESTED_ENTRY_PREFIX: &str = "; jig_manifest=";
const NESTED_COMMAND_SEPARATOR: &str = "; if [ -f \"$jig_manifest\" ]; then found=1; ";
const NESTED_ENTRY_SUFFIX: &str = " || rc=$?; fi";

enum GeneratedRustClippyCommandShape {
    Root,
    OptionalRoot,
    Nested(String),
}

pub(super) struct GeneratedRustClippyCommand {
    shape: GeneratedRustClippyCommandShape,
    adds_all_features: bool,
}

impl GeneratedRustClippyCommand {
    pub(super) fn upgraded_command(&self) -> String {
        match &self.shape {
            GeneratedRustClippyCommandShape::Root => DEFAULT_RUST_CLIPPY_COMMAND.into(),
            GeneratedRustClippyCommandShape::OptionalRoot => optional_root_command(),
            GeneratedRustClippyCommandShape::Nested(command) => command.clone(),
        }
    }

    pub(super) fn upgraded_nested_command(&self) -> Option<String> {
        match &self.shape {
            GeneratedRustClippyCommandShape::Nested(command) => Some(command.clone()),
            GeneratedRustClippyCommandShape::Root
            | GeneratedRustClippyCommandShape::OptionalRoot => None,
        }
    }

    pub(super) fn adds_all_features(&self) -> bool {
        self.adds_all_features
    }
}

pub(super) fn classify_generated_rust_clippy_command(
    command: &str,
) -> Option<GeneratedRustClippyCommand> {
    if let Some(adds_all_features) = root_command_adds_all_features(command) {
        return Some(GeneratedRustClippyCommand {
            shape: GeneratedRustClippyCommandShape::Root,
            adds_all_features,
        });
    }

    if let Some((cargo_command, fallback)) = crate::shell::optional_cargo_command_branches(command)
        && fallback == expected_fallback()
        && let Some(adds_all_features) = root_command_adds_all_features(cargo_command)
    {
        return Some(GeneratedRustClippyCommand {
            shape: GeneratedRustClippyCommandShape::OptionalRoot,
            adds_all_features,
        });
    }

    upgrade_nested_command(command).map(|(command, adds_all_features)| GeneratedRustClippyCommand {
        shape: GeneratedRustClippyCommandShape::Nested(command),
        adds_all_features,
    })
}

fn root_command_adds_all_features(command: &str) -> Option<bool> {
    match command {
        LEGACY_RUST_CLIPPY_COMMAND => Some(true),
        DEFAULT_RUST_CLIPPY_COMMAND => Some(false),
        _ => None,
    }
}

pub(super) fn is_generated_rust_clippy_command(command: &str) -> bool {
    classify_generated_rust_clippy_command(command).is_some()
}

pub(super) fn clippy_command_enforces_mod_module_files(command: &str) -> bool {
    if let Some((cargo_command, _)) = crate::shell::optional_cargo_command_branches(command) {
        return direct_clippy_command_enforces_mod_module_files(cargo_command);
    }
    if let Some(entries) = nested_command_entries(command) {
        return entries.iter().all(|(_, cargo_command)| {
            direct_clippy_command_enforces_mod_module_files(cargo_command)
        });
    }
    direct_clippy_command_enforces_mod_module_files(command)
}

fn optional_root_command() -> String {
    format!(
        "{}{}{}{}{}",
        crate::shell::OPTIONAL_CARGO_COMMAND_PREFIX,
        DEFAULT_RUST_CLIPPY_COMMAND,
        crate::shell::OPTIONAL_CARGO_COMMAND_ELSE,
        expected_fallback(),
        crate::shell::OPTIONAL_CARGO_COMMAND_SUFFIX,
    )
}

fn expected_fallback() -> String {
    format!(
        "printf '%s\\n' {}",
        crate::shell::quote(&format!("{}clippy.", crate::CARGO_SKIP_OUTPUT_PREFIX))
    )
}

fn nested_command_suffix() -> String {
    format!(
        "; if [ \"$found\" -eq 0 ]; then {}; fi; exit \"$rc\" )",
        expected_fallback()
    )
}

fn upgrade_nested_command(command: &str) -> Option<(String, bool)> {
    [
        (LEGACY_NESTED_MANIFEST_RUST_CLIPPY_COMMAND, true),
        (NESTED_MANIFEST_RUST_CLIPPY_COMMAND, false),
    ]
    .into_iter()
    .find_map(|(generated, adds_all_features)| {
        upgrade_nested_command_version(command, generated)
            .map(|command| (command, adds_all_features))
    })
}

fn upgrade_nested_command_version(command: &str, generated: &str) -> Option<String> {
    let entries = nested_command_entries(command)?;
    if entries
        .iter()
        .any(|(_, cargo_command)| *cargo_command != generated)
    {
        return None;
    }
    let suffix = nested_command_suffix();
    let mut upgraded = NESTED_COMMAND_PREFIX.to_string();
    for (manifest_assignment, _) in entries {
        upgraded.push_str(NESTED_ENTRY_PREFIX);
        upgraded.push_str(manifest_assignment);
        upgraded.push_str(NESTED_COMMAND_SEPARATOR);
        upgraded.push_str(NESTED_MANIFEST_RUST_CLIPPY_COMMAND);
        upgraded.push_str(NESTED_ENTRY_SUFFIX);
    }
    upgraded.push_str(&suffix);
    Some(upgraded)
}

fn nested_command_entries(command: &str) -> Option<Vec<(&str, &str)>> {
    let mut remaining = command.strip_prefix(NESTED_COMMAND_PREFIX)?;
    let suffix = nested_command_suffix();
    let mut entries = Vec::new();
    loop {
        if remaining == suffix {
            return (!entries.is_empty()).then_some(entries);
        }
        remaining = remaining.strip_prefix(NESTED_ENTRY_PREFIX)?;
        let separator = remaining.find(NESTED_COMMAND_SEPARATOR)?;
        let manifest_assignment = &remaining[..separator];
        if manifest_assignment.is_empty()
            || manifest_assignment.contains(NESTED_ENTRY_PREFIX)
            || manifest_assignment.contains(NESTED_COMMAND_SEPARATOR)
        {
            return None;
        }
        remaining = &remaining[separator + NESTED_COMMAND_SEPARATOR.len()..];
        let entry_end = remaining.find(NESTED_ENTRY_SUFFIX)?;
        let cargo_command = &remaining[..entry_end];
        if cargo_command.is_empty() {
            return None;
        }
        entries.push((manifest_assignment, cargo_command));
        remaining = &remaining[entry_end + NESTED_ENTRY_SUFFIX.len()..];
    }
}

fn direct_clippy_command_enforces_mod_module_files(command: &str) -> bool {
    if command.trim().is_empty()
        || command.chars().any(|character| {
            matches!(
                character,
                ';' | '|' | '&' | '<' | '>' | '`' | '#' | '\n' | '\r'
            )
        })
        || command.contains("$(")
    {
        return false;
    }
    let tokens = command.split_ascii_whitespace().collect::<Vec<_>>();
    if tokens.get(..2) != Some(["cargo", "clippy"].as_slice()) {
        return false;
    }
    let Some(separator) = tokens.iter().position(|token| *token == "--") else {
        return false;
    };
    let arguments = &tokens[separator + 1..];
    if arguments.iter().any(|argument| argument.contains('$'))
        || arguments
            .iter()
            .any(|argument| *argument == "--cap-lints" || argument.starts_with("--cap-lints="))
    {
        return false;
    }

    let mut enforced = false;
    let mut forbidden = false;
    let mut index = 0;
    while index < arguments.len() {
        let Some((level, lint, consumed)) = lint_level_argument(arguments, index) else {
            index += 1;
            continue;
        };
        if is_mod_module_files_lint(lint) {
            match level {
                LintLevel::Forbid => {
                    enforced = true;
                    forbidden = true;
                }
                LintLevel::Deny if !forbidden => enforced = true,
                LintLevel::AllowOrWarn if !forbidden => enforced = false,
                LintLevel::Deny | LintLevel::AllowOrWarn => {}
            }
        }
        index += consumed;
    }
    enforced
}

#[derive(Clone, Copy)]
enum LintLevel {
    AllowOrWarn,
    Deny,
    Forbid,
}

fn lint_level_argument<'a>(
    arguments: &'a [&'a str],
    index: usize,
) -> Option<(LintLevel, &'a str, usize)> {
    let argument = arguments[index];
    let separate_level = match argument {
        "-A" | "--allow" | "-W" | "--warn" | "--expect" | "--force-warn" => {
            Some(LintLevel::AllowOrWarn)
        }
        "-D" | "--deny" => Some(LintLevel::Deny),
        "-F" | "--forbid" => Some(LintLevel::Forbid),
        _ => None,
    };
    if let Some(level) = separate_level {
        return arguments.get(index + 1).map(|lint| (level, *lint, 2));
    }
    for (prefix, level) in [
        ("--allow=", LintLevel::AllowOrWarn),
        ("--warn=", LintLevel::AllowOrWarn),
        ("--expect=", LintLevel::AllowOrWarn),
        ("--force-warn=", LintLevel::AllowOrWarn),
        ("--deny=", LintLevel::Deny),
        ("--forbid=", LintLevel::Forbid),
        ("-A", LintLevel::AllowOrWarn),
        ("-W", LintLevel::AllowOrWarn),
        ("-D", LintLevel::Deny),
        ("-F", LintLevel::Forbid),
    ] {
        if let Some(lint) = argument
            .strip_prefix(prefix)
            .filter(|lint| !lint.is_empty())
        {
            return Some((level, lint, 1));
        }
    }
    None
}

fn is_mod_module_files_lint(lint: &str) -> bool {
    lint == "clippy::mod_module_files" || lint == "clippy::mod-module-files"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_clippy_classification_covers_root_migration_versions() {
        for (command, adds_all_features) in [
            (LEGACY_RUST_CLIPPY_COMMAND, true),
            (DEFAULT_RUST_CLIPPY_COMMAND, false),
        ] {
            let generated = classify_generated_rust_clippy_command(command).unwrap();
            assert_eq!(generated.upgraded_command(), DEFAULT_RUST_CLIPPY_COMMAND);
            assert_eq!(generated.adds_all_features(), adds_all_features);

            let wrapped = format!(
                "{}{}{}{}{}",
                crate::shell::OPTIONAL_CARGO_COMMAND_PREFIX,
                command,
                crate::shell::OPTIONAL_CARGO_COMMAND_ELSE,
                expected_fallback(),
                crate::shell::OPTIONAL_CARGO_COMMAND_SUFFIX,
            );
            let generated = classify_generated_rust_clippy_command(&wrapped).unwrap();
            assert_eq!(generated.upgraded_command(), optional_root_command());
            assert_eq!(generated.adds_all_features(), adds_all_features);
        }

        let without_all_features = "cargo clippy --workspace --all-targets --locked -- -D warnings -D clippy::mod_module_files";
        assert!(classify_generated_rust_clippy_command(without_all_features).is_none());
        assert!(
            classify_generated_rust_clippy_command(&format!(
                "{}{}{}{}{}",
                crate::shell::OPTIONAL_CARGO_COMMAND_PREFIX,
                without_all_features,
                crate::shell::OPTIONAL_CARGO_COMMAND_ELSE,
                expected_fallback(),
                crate::shell::OPTIONAL_CARGO_COMMAND_SUFFIX,
            ))
            .is_none()
        );
    }

    #[test]
    fn generated_clippy_classification_upgrades_nested_manifest_versions() {
        for (command, adds_all_features) in [
            (LEGACY_NESTED_MANIFEST_RUST_CLIPPY_COMMAND, true),
            (NESTED_MANIFEST_RUST_CLIPPY_COMMAND, false),
        ] {
            let nested =
                generated_nested_command(&["api/Cargo.toml", "worker/Cargo.toml"], command);
            let generated = classify_generated_rust_clippy_command(&nested).unwrap();
            assert_eq!(generated.adds_all_features(), adds_all_features);
            assert_eq!(
                generated.upgraded_command(),
                generated_nested_command(
                    &["api/Cargo.toml", "worker/Cargo.toml"],
                    NESTED_MANIFEST_RUST_CLIPPY_COMMAND,
                )
            );
        }

        let without_all_features = generated_nested_command(
            &["api/Cargo.toml"],
            "cargo clippy --manifest-path \"$jig_manifest\" --all-targets -- -D warnings -D clippy::mod_module_files",
        );
        assert!(classify_generated_rust_clippy_command(&without_all_features).is_none());

        let quoted_paths = ["api with spaces/Cargo.toml", "worker's/Cargo.toml"];
        let quoted =
            generated_nested_command(&quoted_paths, LEGACY_NESTED_MANIFEST_RUST_CLIPPY_COMMAND);
        assert_eq!(
            classify_generated_rust_clippy_command(&quoted)
                .unwrap()
                .upgraded_command(),
            generated_nested_command(&quoted_paths, NESTED_MANIFEST_RUST_CLIPPY_COMMAND)
        );
    }

    #[test]
    fn generated_clippy_classification_preserves_custom_commands_and_wrappers() {
        assert!(
            classify_generated_rust_clippy_command(&format!(
                "{DEFAULT_RUST_CLIPPY_COMMAND} --custom"
            ))
            .is_none()
        );

        let custom_fallback = format!(
            "{}{}{}printf custom{}",
            crate::shell::OPTIONAL_CARGO_COMMAND_PREFIX,
            DEFAULT_RUST_CLIPPY_COMMAND,
            crate::shell::OPTIONAL_CARGO_COMMAND_ELSE,
            crate::shell::OPTIONAL_CARGO_COMMAND_SUFFIX,
        );
        assert!(classify_generated_rust_clippy_command(&custom_fallback).is_none());

        let nested = generated_nested_command(
            &["api/Cargo.toml", "worker/Cargo.toml"],
            LEGACY_NESTED_MANIFEST_RUST_CLIPPY_COMMAND,
        );
        assert!(
            classify_generated_rust_clippy_command(
                &nested.replace(" || rc=$?", " --custom || rc=$?")
            )
            .is_none()
        );
        assert!(
            classify_generated_rust_clippy_command(
                &nested.replace("skipping cargo clippy.", "custom fallback")
            )
            .is_none()
        );

        let mixed = nested.replacen(
            &format!(
                "jig_manifest=api/Cargo.toml{NESTED_COMMAND_SEPARATOR}{LEGACY_NESTED_MANIFEST_RUST_CLIPPY_COMMAND}"
            ),
            &format!(
                "jig_manifest=api/Cargo.toml{NESTED_COMMAND_SEPARATOR}{NESTED_MANIFEST_RUST_CLIPPY_COMMAND}"
            ),
            1,
        );
        assert!(classify_generated_rust_clippy_command(&mixed).is_none());
    }

    #[test]
    fn clippy_policy_verification_requires_effective_denial_in_every_invocation() {
        for command in [
            DEFAULT_RUST_CLIPPY_COMMAND.to_string(),
            "cargo clippy --workspace -- -Dclippy::mod_module_files".into(),
            "cargo clippy --workspace -- --forbid=clippy::mod-module-files".into(),
            optional_root_command(),
            generated_nested_command(
                &["api/Cargo.toml", "worker/Cargo.toml"],
                NESTED_MANIFEST_RUST_CLIPPY_COMMAND,
            ),
        ] {
            assert!(
                clippy_command_enforces_mod_module_files(&command),
                "{command}"
            );
        }

        let denied = "cargo clippy --workspace -- -D clippy::mod_module_files";
        let allowed = "cargo clippy --workspace -- -A clippy::mod_module_files";
        for command in [
            allowed.into(),
            format!("{denied} -A clippy::mod_module_files"),
            format!("{denied} --cap-lints allow"),
            format!("{denied} $EXTRA_FLAGS"),
            format!("{denied} ${{EXTRA_FLAGS}}"),
            "printf '%s' clippy::mod_module_files".to_string(),
            format!("{denied} # clippy::mod_module_files"),
            generated_nested_command(
                &["api/Cargo.toml", "worker/Cargo.toml"],
                LEGACY_NESTED_MANIFEST_RUST_CLIPPY_COMMAND,
            ),
            generated_nested_command(&["api/Cargo.toml"], denied).replace(
                &format!("jig_manifest=api/Cargo.toml{NESTED_COMMAND_SEPARATOR}{denied}"),
                &format!("jig_manifest=api/Cargo.toml{NESTED_COMMAND_SEPARATOR}{allowed}"),
            ),
        ] {
            assert!(
                !clippy_command_enforces_mod_module_files(&command),
                "{command}"
            );
        }

        let mixed = generated_nested_command(
            &["api/Cargo.toml", "worker/Cargo.toml"],
            NESTED_MANIFEST_RUST_CLIPPY_COMMAND,
        )
        .replacen(
            NESTED_MANIFEST_RUST_CLIPPY_COMMAND,
            LEGACY_NESTED_MANIFEST_RUST_CLIPPY_COMMAND,
            1,
        );
        assert!(!clippy_command_enforces_mod_module_files(&mixed));
    }

    fn generated_nested_command(manifests: &[&str], cargo_command: &str) -> String {
        let mut command = NESTED_COMMAND_PREFIX.to_string();
        for manifest in manifests {
            command.push_str(NESTED_ENTRY_PREFIX);
            command.push_str(&crate::shell::quote(manifest));
            command.push_str(NESTED_COMMAND_SEPARATOR);
            command.push_str(cargo_command);
            command.push_str(NESTED_ENTRY_SUFFIX);
        }
        command.push_str(&nested_command_suffix());
        command
    }
}
