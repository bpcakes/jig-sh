use std::collections::BTreeMap;

use jig_contract::{ActionEffect, ActionIntent, ActionRunner};

use super::*;

fn action(target: &str, inputs: &[&str]) -> ActionSpec {
    let mut action = ActionSpec::new(
        target.parse().unwrap(),
        ActionIntent::Check,
        ActionRunner::Argv {
            program: "scripts/check.sh".into(),
            args: vec![],
            working_directory: None,
            environment: BTreeMap::new(),
        },
    );
    action.effects = vec![ActionEffect::ReadOnly, ActionEffect::Process];
    action.inputs = inputs.iter().map(|value| (*value).into()).collect();
    action
}

#[test]
fn old_epoch_policy_presence_and_empty_exhaustive_declarations_are_rejected() {
    let mut action = action("web:test", &["apps/web/**"]);
    for policy in [
        ActionInputsPolicy::WholeRepository,
        ActionInputsPolicy::Exhaustive,
    ] {
        action.inputs_policy = Some(policy);
        for epoch in 2..8 {
            assert!(validate_inputs_policy(epoch, &action).is_err());
        }
        validate_inputs_policy(8, &action).unwrap();
    }
    action.inputs.clear();
    assert!(validate_inputs_policy(8, &action).is_err());
    action.inputs_policy = None;
    validate_inputs_policy(8, &action).unwrap();
}

#[test]
fn source_state_requires_new_epoch_and_native_comparison_cannot_opt_out() {
    let mut command = action("web:test", &["apps/web/**"]);
    for state in [ActionSourceState::Git, ActionSourceState::Worktree] {
        command.source_state = Some(state);
        for epoch in 2..8 {
            assert!(validate_inputs_policy(epoch, &command).is_err());
        }
        assert!(validate_inputs_policy(8, &command).is_ok());
    }
    command.runner = ActionRunner::Native {
        operation: "file_budget".into(),
        configuration: None,
    };
    assert!(validate_inputs_policy(8, &command).is_err());
}
