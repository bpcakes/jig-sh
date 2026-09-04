use super::*;

use crate::bootstrap::clippy_policy::{
    classify_generated_rust_clippy_command, clippy_command_enforces_mod_module_files,
};

#[derive(Debug, Default)]
pub(in crate::bootstrap::answers) struct ClippyDefaultDiagnostics {
    clippy_all_features_commands: BTreeSet<String>,
    unverified_clippy_policy_commands: BTreeSet<String>,
}

impl ClippyDefaultDiagnostics {
    pub(in crate::bootstrap::answers) fn warnings(self) -> Vec<String> {
        let mut warnings = Vec::new();
        if !self.clippy_all_features_commands.is_empty() {
            warnings.push(format!(
                "Updated exact generated Clippy input(s) to check all Cargo features: {}. The rewritten .jig.toml stores effective runners under [commands]; review every command referenced by a repository action exposing `jig.clippy`. If this repository has mutually exclusive features, remove `--all-features` from those effective command values; this explicit feature-coverage customization is preserved by later updates.",
                self.clippy_all_features_commands
                    .into_iter()
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if !self.unverified_clippy_policy_commands.is_empty() {
            warnings.push(format!(
                "Could not verify `clippy::mod_module_files` enforcement for custom Clippy input(s): {}. These values were preserved. Review each effective command or wrapper and enforce the lint there or through inherited workspace lints.",
                self.unverified_clippy_policy_commands
                    .into_iter()
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        warnings
    }
}

pub(in crate::bootstrap::answers) fn normalize_generated_clippy_defaults(
    raw: &mut RawAnswers,
    commands: &mut Option<BTreeMap<String, String>>,
) -> ClippyDefaultDiagnostics {
    let mut migrations = ClippyDefaultDiagnostics::default();
    let effective_command_keys = effective_clippy_command_keys(raw, commands.as_ref());
    inspect_raw_command(raw, effective_command_keys.is_empty(), &mut migrations);
    raw.normalize_generated_clippy_default();
    let Some(commands) = commands.as_mut() else {
        return migrations;
    };
    for key in effective_command_keys {
        normalize_command(
            commands.get_mut(&key),
            &format!("commands.{key}"),
            &mut migrations,
        );
    }
    migrations
}

fn inspect_raw_command(raw: &RawAnswers, report: bool, migrations: &mut ClippyDefaultDiagnostics) {
    let Some(command) = raw.rust_clippy_command.as_deref() else {
        return;
    };
    if let Some(generated) = classify_generated_rust_clippy_command(command) {
        if report && generated.adds_all_features() {
            migrations
                .clippy_all_features_commands
                .insert("rust_clippy_command".into());
        }
    } else if report && !clippy_command_enforces_mod_module_files(command) {
        migrations
            .unverified_clippy_policy_commands
            .insert("rust_clippy_command".into());
    }
}

fn effective_clippy_command_keys(
    raw: &RawAnswers,
    commands: Option<&BTreeMap<String, String>>,
) -> BTreeSet<String> {
    let mut keys = raw
        .repository
        .iter()
        .flat_map(|repository| &repository.actions)
        .filter(|action| {
            action.target.action.as_str() == "clippy"
                || action
                    .legacy_aliases
                    .iter()
                    .any(|alias| alias == jig_contract::tool::CLIPPY)
        })
        .filter_map(|action| match &action.runner {
            jig_contract::ActionRunner::Command { command, .. } => Some(command.clone()),
            jig_contract::ActionRunner::Native { .. } => None,
        })
        .collect::<BTreeSet<_>>();
    if keys.is_empty()
        && commands.is_some_and(|commands| commands.contains_key("api_clippy_command"))
    {
        keys.insert("api_clippy_command".into());
    }
    keys
}

fn normalize_command(
    command: Option<&mut String>,
    source: &str,
    migrations: &mut ClippyDefaultDiagnostics,
) {
    let Some(command) = command else {
        return;
    };
    if let Some(generated) = classify_generated_rust_clippy_command(command) {
        if generated.adds_all_features() {
            migrations
                .clippy_all_features_commands
                .insert(source.into());
        }
        *command = generated.upgraded_command();
    } else if !clippy_command_enforces_mod_module_files(command) {
        migrations
            .unverified_clippy_policy_commands
            .insert(source.into());
    }
}
