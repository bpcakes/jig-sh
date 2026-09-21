mod literal_exec;
pub(crate) use literal_exec::prepare as prepare_literal_exec;

pub(crate) const DEFAULT_ARGV_SEARCH_PATH: &str = "/bin:/usr/bin";

use std::{collections::BTreeMap, process::Command};

use anyhow::{Result, ensure};
use jig_contract::{ActionRunner, ActionSpec, ArgvValue};

use super::ACTION_EXECUTION_CONTRACT_VERSION;

pub(crate) fn validate(version: u32, action: &ActionSpec) -> Result<()> {
    match &action.runner {
        ActionRunner::RustNextestV1 { configuration } => {
            ensure!(
                version >= ACTION_EXECUTION_CONTRACT_VERSION,
                "Rust Nextest v1 requires contract v8 or later"
            );
            jig_rust::rust_focus::validate_config(configuration).map_err(anyhow::Error::msg)?;
            let expected = if configuration.focused {
                BTreeMap::from([(
                    "focus".into(),
                    jig_contract::ActionArgumentSpec::RustFocusV1 {},
                )])
            } else {
                BTreeMap::new()
            };
            ensure!(
                action.arguments == expected,
                "Rust Nextest arguments must match the fixed full or focused v1 capability"
            );
            ensure!(
                action.legacy_aliases.is_empty(),
                "Rust Nextest v1 uses typed target execution, not legacy tool aliases"
            );
            ensure!(
                action.intent == jig_contract::ActionIntent::Check
                    && action.result_parser == jig_contract::ResultParser::ExitCode,
                "Rust Nextest v1 requires a check action with the exit_code result parser"
            );
            ensure!(
                action.inputs_policy.unwrap_or_default()
                    == jig_contract::ActionInputsPolicy::WholeRepository,
                "Rust Nextest v1 requires whole_repository input authority; narrow input reuse is not supported"
            );
            return Ok(());
        }
        ActionRunner::Command { .. } => ensure!(
            version < ACTION_EXECUTION_CONTRACT_VERSION,
            "target '{}' must explicitly select an argv or shell runner in contract v8",
            action.target
        ),
        ActionRunner::Shell { .. } | ActionRunner::Argv { .. } => ensure!(
            version >= ACTION_EXECUTION_CONTRACT_VERSION,
            "target '{}' argv and shell runners require contract version 8 or later",
            action.target
        ),
        ActionRunner::Native { .. } => return Ok(()),
    }
    match &action.runner {
        ActionRunner::Argv {
            program,
            args,
            environment,
            ..
        } => {
            ensure!(
                !program.is_empty() && !program.contains('\0'),
                "target '{}' requires a nonempty NUL-free literal program",
                action.target
            );
            crate::repository_path::validate_runner_environment(environment)?;
            for value in args {
                match value {
                    ArgvValue::Literal(value) => ensure!(
                        !value.contains('\0'),
                        "target '{}' argv contains NUL",
                        action.target
                    ),
                    ArgvValue::Argument { argument } => ensure!(
                        action.arguments.contains_key(argument),
                        "target '{}' argv references undeclared argument '{argument}'",
                        action.target
                    ),
                }
            }
        }
        ActionRunner::Shell { environment, .. } => {
            ensure!(
                action.arguments.is_empty(),
                "target '{}' shell runners accept no argument declarations or interpolation",
                action.target
            );
            crate::repository_path::validate_runner_environment(environment)?;
        }
        _ => {}
    }
    Ok(())
}

/// Bind whole argv positions once; values are never parsed or expanded again.
/// After setting cwd/environment, callers must use `prepare_literal_exec`
/// before spawning to prevent libc's implicit shell fallback.
pub(crate) fn argv_command(
    program: &str,
    args: &[ArgvValue],
    supplied: &BTreeMap<String, String>,
) -> Command {
    let mut command = Command::new(program);
    for value in args {
        match value {
            ArgvValue::Literal(value) => {
                command.arg(value);
            }
            ArgvValue::Argument { argument } => {
                if let Some(value) = supplied.get(argument) {
                    command.arg(value);
                }
            }
        }
    }
    command
}

#[cfg(test)]
mod tests {
    use super::*;
    use jig_contract::{ActionArgumentSpec, ActionIntent};

    #[test]
    fn runners_enforce_epoch_and_closed_binding_positions() {
        let mut action = ActionSpec::new(
            "api:test".parse().unwrap(),
            ActionIntent::Check,
            ActionRunner::command("test_command"),
        );
        for version in [6, 7] {
            validate(version, &action).unwrap();
        }
        assert!(validate(8, &action).is_err());
        action.runner = ActionRunner::Shell {
            command: "test_command".into(),
            working_directory: None,
            environment: BTreeMap::new(),
        };
        for version in [6, 7] {
            assert!(validate(version, &action).is_err());
        }
        validate(8, &action).unwrap();
        action
            .arguments
            .insert("message".into(), ActionArgumentSpec::migration_name());
        assert!(validate(8, &action).is_err());
        action.runner = ActionRunner::Argv {
            program: "literal $(program)".into(),
            args: vec![ArgvValue::Argument {
                argument: "message".into(),
            }],
            working_directory: None,
            environment: BTreeMap::new(),
        };
        validate(8, &action).unwrap();
        for version in [6, 7] {
            assert!(validate(version, &action).is_err());
        }
        action.arguments.clear();
        assert!(validate(8, &action).is_err());
        for value in [
            serde_json::json!({"argument":"message", "extra":true}),
            serde_json::json!({"environment":"message"}),
        ] {
            assert!(serde_json::from_value::<ArgvValue>(value).is_err());
        }
    }
}
