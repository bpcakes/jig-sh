use std::collections::BTreeMap;

use anyhow::{Result, bail, ensure};
use jig_contract::{ActionArgumentSpec, ActionArguments, ActionRunner, ActionSpec, TargetId};

use super::ACTION_EXECUTION_CONTRACT_VERSION;

const MAX_ARGUMENTS: usize = 32;
const MAX_STRING_BYTES: u32 = 4096;

pub(crate) fn normalize_declarations(version: u32, action: &mut ActionSpec) -> Result<()> {
    if version < ACTION_EXECUTION_CONTRACT_VERSION {
        ensure!(
            action.arguments.is_empty(),
            "target '{}' argument declarations require contract version 8 or later",
            action.target
        );
        return Ok(());
    }
    ensure!(
        action.arguments.len() <= MAX_ARGUMENTS,
        "target '{}' exceeds {MAX_ARGUMENTS} argument declarations",
        action.target
    );
    for (name, spec) in &action.arguments {
        ensure!(
            valid_name(name),
            "target '{}' has invalid argument name '{name}'",
            action.target
        );
        let ActionArgumentSpec::String { max_bytes, .. } = spec;
        ensure!(
            (1..=MAX_STRING_BYTES).contains(max_bytes),
            "target '{}' argument '{name}' max_bytes must be between 1 and {MAX_STRING_BYTES}",
            action.target
        );
    }
    // Native operations have fixed input contracts. Declarations cannot extend
    // or weaken the operation's authority; command actions only bind literals.
    if matches!(action.runner, ActionRunner::Native { .. }) {
        let expected = if migration_action(action) {
            BTreeMap::from([("name".into(), ActionArgumentSpec::migration_name())])
        } else {
            BTreeMap::new()
        };
        ensure!(
            action.arguments == expected,
            "target '{}' argument declarations do not match its native operation",
            action.target
        );
    }
    Ok(())
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.as_bytes()[0].is_ascii_lowercase()
        && name.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-')
        })
}

fn migration_action(action: &ActionSpec) -> bool {
    matches!(&action.runner, ActionRunner::Native { operation, .. } if operation == jig_contract::tool::MIGRATION_ADD)
}

/// Canonical maps and omitted empty target maps are the only normalization.
/// String bytes are preserved, including whitespace and embedded equals signs.
pub(crate) fn bind(
    version: u32,
    action: &ActionSpec,
    supplied: ActionArguments,
) -> Result<ActionArguments> {
    // Legacy native inputs predate bounded declarations. Preserve that input
    // contract without inventing a finite bound for an unbounded legacy name.
    if version < ACTION_EXECUTION_CONTRACT_VERSION && migration_action(action) {
        for name in supplied.keys() {
            ensure!(
                name == "name",
                "target '{}' does not accept argument '{name}'",
                action.target
            );
        }
        let name = supplied.get("name").ok_or_else(|| {
            anyhow::anyhow!("target '{}' requires string argument 'name'", action.target)
        })?;
        ensure!(
            !name.trim().is_empty() && !name.starts_with('-'),
            "target '{}' has invalid argument 'name'",
            action.target
        );
        return Ok(supplied);
    }
    for name in supplied.keys() {
        ensure!(
            action.arguments.contains_key(name),
            "target '{}' does not accept argument '{name}'",
            action.target
        );
    }
    for (name, spec) in &action.arguments {
        let ActionArgumentSpec::String {
            required,
            allow_empty,
            max_bytes,
        } = spec;
        match supplied.get(name) {
            None if *required => bail!(
                "target '{}' requires string argument '{name}'",
                action.target
            ),
            Some(value) => {
                ensure!(
                    *allow_empty || !value.is_empty(),
                    "target '{}' argument '{name}' forbids empty strings",
                    action.target
                );
                ensure!(
                    value.len() <= *max_bytes as usize,
                    "target '{}' argument '{name}' exceeds {max_bytes} bytes",
                    action.target
                );
                ensure!(
                    !value.contains('\0'),
                    "target '{}' argument '{name}' contains NUL",
                    action.target
                );
            }
            None => {}
        }
    }
    if migration_action(action) {
        let name = supplied.get("name").ok_or_else(|| {
            anyhow::anyhow!("target '{}' requires string argument 'name'", action.target)
        })?;
        ensure!(
            !name.starts_with('-') && name.chars().any(|ch| ch.is_ascii_alphanumeric()),
            "target '{}' has invalid argument 'name'",
            action.target
        );
    }
    Ok(supplied)
}

pub(crate) fn parse_cli(values: Vec<String>) -> Result<BTreeMap<TargetId, ActionArguments>> {
    let mut targets: BTreeMap<TargetId, ActionArguments> = BTreeMap::new();
    for value in values {
        let (binding, value) = value
            .split_once('=')
            .ok_or_else(|| anyhow::anyhow!("--arg requires TARGET:NAME=VALUE"))?;
        let (target, name) = binding
            .rsplit_once(':')
            .ok_or_else(|| anyhow::anyhow!("--arg requires TARGET:NAME=VALUE"))?;
        let target: TargetId = target.parse()?;
        ensure!(valid_name(name), "invalid argument name '{name}'");
        let args = targets.entry(target.clone()).or_default();
        ensure!(
            args.insert(name.into(), value.into()).is_none(),
            "duplicate argument '{name}' for target '{target}'"
        );
    }
    Ok(targets)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn action() -> ActionSpec {
        ActionSpec::new(
            "api:generate".parse().unwrap(),
            jig_contract::ActionIntent::Generate,
            ActionRunner::Argv {
                program: "example-generate".into(),
                args: vec![],
                working_directory: None,
                environment: BTreeMap::new(),
            },
        )
    }

    #[test]
    fn declaration_bounds_names_and_epoch_are_enforced() {
        let mut action = action();
        for name in [
            "",
            "Name",
            "1name",
            "name.part",
            "name:value",
            &"a".repeat(65),
        ] {
            action.arguments =
                BTreeMap::from([(name.into(), ActionArgumentSpec::migration_name())]);
            assert!(normalize_declarations(8, &mut action).is_err(), "{name}");
        }
        for max_bytes in [0, 4097, u32::MAX] {
            action.arguments = BTreeMap::from([(
                "message".into(),
                ActionArgumentSpec::String {
                    required: false,
                    allow_empty: true,
                    max_bytes,
                },
            )]);
            assert!(normalize_declarations(8, &mut action).is_err());
        }
        action.arguments = BTreeMap::from([(
            "message".into(),
            ActionArgumentSpec::String {
                required: false,
                allow_empty: false,
                max_bytes: 4,
            },
        )]);
        assert!(
            normalize_declarations(7, &mut action)
                .unwrap_err()
                .to_string()
                .contains("contract version 8")
        );
        normalize_declarations(8, &mut action).unwrap();
        assert!(bind(8, &action, BTreeMap::new()).unwrap().is_empty());
        assert_eq!(
            bind(
                8,
                &action,
                BTreeMap::from([("message".into(), "éé".into())])
            )
            .unwrap()["message"],
            "éé"
        );
        action.arguments = (0..33)
            .map(|i| (format!("arg{i}"), ActionArgumentSpec::migration_name()))
            .collect();
        assert!(normalize_declarations(8, &mut action).is_err());
    }

    #[test]
    fn native_declarations_cannot_weaken_or_extend_the_operation() {
        let mut action = action();
        action.runner = ActionRunner::native(jig_contract::tool::MIGRATION_ADD);
        assert!(normalize_declarations(8, &mut action).is_err());
        normalize_declarations(7, &mut action).unwrap();
        assert!(action.arguments.is_empty());
        action
            .arguments
            .insert("name".into(), ActionArgumentSpec::migration_name());
        normalize_declarations(8, &mut action).unwrap();
        action
            .arguments
            .insert("extra".into(), ActionArgumentSpec::migration_name());
        assert!(normalize_declarations(8, &mut action).is_err());
        action.runner = ActionRunner::native(jig_contract::tool::CONTRACT_CHECK);
        assert!(normalize_declarations(8, &mut action).is_err());
    }

    #[test]
    fn legacy_native_names_keep_their_original_validation() {
        let mut action = action();
        action.runner = ActionRunner::native(jig_contract::tool::MIGRATION_ADD);
        for version in [6, 7] {
            normalize_declarations(version, &mut action).unwrap();
            for name in ["x".repeat(201), format!("Example{}", " ".repeat(5000))] {
                let supplied = BTreeMap::from([("name".into(), name)]);
                assert_eq!(bind(version, &action, supplied.clone()).unwrap(), supplied);
            }
            for supplied in [
                BTreeMap::new(),
                BTreeMap::from([("name".into(), " \t".into())]),
                BTreeMap::from([("name".into(), "-unsafe".into())]),
                BTreeMap::from([
                    ("name".into(), "Example".into()),
                    ("extra".into(), "x".into()),
                ]),
            ] {
                assert!(bind(version, &action, supplied).is_err());
            }
        }
        action
            .arguments
            .insert("name".into(), ActionArgumentSpec::migration_name());
        normalize_declarations(8, &mut action).unwrap();
        for name in ["x".repeat(201), format!("Example{}", " ".repeat(5000))] {
            assert!(
                bind(8, &action, BTreeMap::from([("name".into(), name)]))
                    .unwrap_err()
                    .to_string()
                    .contains("exceeds 200 bytes")
            );
        }
    }

    #[test]
    fn cli_rejects_duplicate_and_unqualified_arguments() {
        for values in [
            vec!["message=value"],
            vec!["api:generate:message"],
            vec!["api:generate:message=one", "api:generate:message=two"],
        ] {
            assert!(parse_cli(values.into_iter().map(str::to_owned).collect()).is_err());
        }
        let values = parse_cli(vec!["api:generate:message=  a=b  ".into()]).unwrap();
        assert_eq!(
            values[&"api:generate".parse().unwrap()]["message"],
            "  a=b  "
        );
    }
}
