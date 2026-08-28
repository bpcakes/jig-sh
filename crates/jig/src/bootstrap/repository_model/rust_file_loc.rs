use std::collections::BTreeMap;

use anyhow::Result;
use jig_contract::{ActionEffect, ActionIntent, ActionRunner, ActionSpec, FieldProvenance};

use super::{CommandScope, ModelBuilder, REPO_COMPONENT, provenance, target_id};

pub(super) const RUST_FILE_LOC_ACTION: &str = "rust-file-loc";
pub(in crate::bootstrap) const RUST_FILE_LOC_COMMAND_KEY: &str = "repo_rust_file_loc_command";
const RUST_FILE_LOC_SCRIPT: &str = "scripts/check-rust-file-loc.sh";

pub(in crate::bootstrap) fn is_rust_file_loc_action(action: &ActionSpec) -> bool {
    action.target.component.as_str() == REPO_COMPONENT
        && action.target.action.as_str() == RUST_FILE_LOC_ACTION
}

pub(in crate::bootstrap) fn action_uses_managed_rust_file_loc_checker(
    action: &ActionSpec,
    commands: &BTreeMap<String, String>,
) -> bool {
    if !is_rust_file_loc_action(action)
        || action.runner != ActionRunner::command(RUST_FILE_LOC_COMMAND_KEY)
    {
        return false;
    }
    commands
        .get(RUST_FILE_LOC_COMMAND_KEY)
        .is_some_and(|command| {
            command
                .strip_prefix(RUST_FILE_LOC_SCRIPT)
                .is_some_and(|suffix| {
                    suffix.is_empty()
                        || suffix
                            .as_bytes()
                            .first()
                            .is_some_and(u8::is_ascii_whitespace)
                })
        })
}

pub(in crate::bootstrap) fn is_generated_rust_file_loc_command(command: &str) -> bool {
    command
        .strip_prefix(RUST_FILE_LOC_SCRIPT)
        .and_then(|suffix| suffix.strip_prefix(' '))
        .is_some_and(|argument| {
            !argument.is_empty()
                && !argument.starts_with('-')
                && !argument.bytes().any(|byte| byte.is_ascii_whitespace())
        })
}

pub(super) fn refresh_managed_rust_file_loc_command(
    actions: &[ActionSpec],
    commands: &mut BTreeMap<String, String>,
    default_branch: &str,
) {
    if actions.iter().any(|action| {
        action_uses_managed_rust_file_loc_checker(action, commands)
            && commands
                .get(RUST_FILE_LOC_COMMAND_KEY)
                .is_some_and(|command| is_generated_rust_file_loc_command(command))
    }) {
        commands.insert(
            RUST_FILE_LOC_COMMAND_KEY.into(),
            rust_file_loc_command(default_branch),
        );
    }
}

fn rust_file_loc_command(default_branch: &str) -> String {
    format!(
        "{RUST_FILE_LOC_SCRIPT} {}",
        crate::shell::quote(default_branch)
    )
}

impl ModelBuilder<'_> {
    pub(super) fn add_rust_file_loc_action(&mut self) -> Result<()> {
        let action_id = RUST_FILE_LOC_ACTION;
        let command_key = CommandScope::Component.command_key(REPO_COMPONENT, action_id)?;
        debug_assert_eq!(command_key, RUST_FILE_LOC_COMMAND_KEY);
        let command = rust_file_loc_command(self.answers.default_branch());
        self.insert_command(&command_key, &command)?;
        let mut action = ActionSpec::new(
            target_id(REPO_COMPONENT, action_id)?,
            ActionIntent::Check,
            ActionRunner::command(command_key.clone()),
        );
        action.description = Some("Enforce the changed-file Rust source size policy.".into());
        action.effects = vec![ActionEffect::ReadOnly, ActionEffect::Process];
        action.inputs = vec![
            "**/*.rs".into(),
            "Cargo.toml".into(),
            "Cargo.lock".into(),
            "rust-toolchain*".into(),
            ".cargo/**".into(),
            "scripts/check-rust-file-loc.sh".into(),
        ];
        action.legacy_aliases = vec!["jig.rust_file_loc".into()];
        action.provenance = provenance(&[
            ("target", FieldProvenance::Inherited),
            ("intent", FieldProvenance::Inherited),
            ("effects", FieldProvenance::Inherited),
            ("runner", FieldProvenance::Inferred),
            ("inputs", FieldProvenance::Inherited),
            ("legacy_aliases", FieldProvenance::Inherited),
        ]);
        self.insert_tool(
            "jig.rust_file_loc",
            "Enforce the changed-file Rust source size policy.",
            Some(&command_key),
        )?;
        self.insert_action(action)
    }
}
