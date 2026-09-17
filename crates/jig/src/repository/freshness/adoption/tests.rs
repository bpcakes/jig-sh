use super::*;

#[test]
fn report_preserves_authored_presence_across_normalized_manifest_defaults() {
    use crate::context::RepoContext;
    use jig_contract::{ComponentSpec, ProfileSpec};
    use serde_json::json;
    use std::fs;

    for authored_explicit in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir(temp.path().join(".agent")).unwrap();
        let mut source = formatter();
        source.runner = ActionRunner::Shell {
            command: "format_command".into(),
            working_directory: None,
            environment: Default::default(),
        };
        let mut resolved = source.clone();
        if authored_explicit {
            source.source_state = Some(ActionSourceState::Git);
        } else {
            resolved.source_state = Some(ActionSourceState::Git);
        }
        let mut repository = json!({
            "components": [ComponentSpec::new("workspace".parse().unwrap(), ".")],
            "actions": [source],
            "profiles": [ProfileSpec::new("verify".parse().unwrap(), vec!["workspace:fmt".parse().unwrap()])],
            "default_check_profile": "verify"
        });
        let config = json!({
            "_src_path": "embedded", "_commit": "example",
            "repo_name": "ExampleProject", "default_branch": "main",
            "commands": {"format_command": "cargo fmt --all -- --check"},
            "repository": repository
        });
        let config_path = temp.path().join(".jig.toml");
        let original = toml::to_string(&config).unwrap();
        fs::write(&config_path, &original).unwrap();
        repository["actions"] = json!([resolved]);
        repository["contract_version"] = json!(8);
        repository["tool_namespace"] = json!("jig");
        let manifest_path = temp.path().join(".agent/jig-contract.json");
        let manifest = serde_json::to_string_pretty(&repository).unwrap();
        fs::write(&manifest_path, &manifest).unwrap();
        let ctx = RepoContext::load_from_root(temp.path().into()).unwrap();
        let result = report(&ctx).unwrap();
        assert_eq!(
            result["targets"][0]["reason"],
            if authored_explicit {
                "authored_source_policy"
            } else {
                "known_formatter"
            }
        );
        assert_eq!(fs::read_to_string(config_path).unwrap(), original);
        assert_eq!(fs::read_to_string(manifest_path).unwrap(), manifest);
        assert!(format_report(&result).contains("workspace:fmt"));
    }
}

fn formatter() -> ActionSpec {
    let mut action = ActionSpec::new(
        "workspace:fmt".parse().unwrap(),
        ActionIntent::Check,
        ActionRunner::command("format_command"),
    );
    action.effects = vec![ActionEffect::ReadOnly, ActionEffect::Process];
    action.inputs = vec!["**/*.rs".into()];
    action
}

#[test]
fn known_formatter_only_recommends_git_independence() {
    let action = formatter();
    let recommendation = recommend(8, &action, Some("cargo fmt --all -- --check"));
    assert_eq!(recommendation.reason, Reason::KnownFormatter);
    assert_eq!(
        recommendation.proposed.source_state,
        ActionSourceState::Worktree
    );
    assert_eq!(
        recommendation.proposed.inputs_policy,
        ActionInputsPolicy::WholeRepository
    );
    assert!(recommendation.exhaustive_requires_owner_assertion);
    for command in [
        "just fmt",
        "cargo test",
        "cargo clippy",
        "cargo fmt --all -- --check && git diff",
        "scripts/fmt.sh",
    ] {
        assert_eq!(
            recommend(8, &action, Some(command)).reason,
            Reason::UnknownCommand
        );
    }
}

#[test]
fn authored_conservative_policy_and_legacy_epochs_are_preserved() {
    let mut action = formatter();
    action.source_state = Some(ActionSourceState::Git);
    assert_eq!(
        recommend(8, &action, Some("cargo fmt --all -- --check")).reason,
        Reason::AuthoredSourcePolicy
    );
    action
        .provenance
        .insert("source_state".into(), FieldProvenance::Inferred);
    assert_eq!(
        recommend(8, &action, Some("cargo fmt --all -- --check")).reason,
        Reason::KnownFormatter
    );
    assert_eq!(
        recommend(7, &action, Some("cargo fmt --all -- --check")).reason,
        Reason::UnsupportedEpoch
    );
    action.runner = ActionRunner::native("jig.file_budget");
    assert_eq!(recommend(8, &action, None).reason, Reason::NativeAuthority);
}

#[test]
fn altered_execution_context_and_mutating_actions_are_not_qualified() {
    let mut action = formatter();
    if let ActionRunner::Command { environment, .. } = &mut action.runner {
        environment.insert("RUSTFMT".into(), "custom-formatter".into());
    }
    assert_eq!(
        recommend(8, &action, Some("cargo fmt --all -- --check")).reason,
        Reason::UnknownCommand
    );
    action.effects.push(ActionEffect::Worktree);
    assert_eq!(
        recommend(8, &action, Some("cargo fmt --all -- --check")).reason,
        Reason::NotReadOnlyCheck
    );
    action = formatter();
    if let ActionRunner::Command {
        working_directory, ..
    } = &mut action.runner
    {
        *working_directory = Some("nested".into());
    }
    assert_eq!(
        recommend(8, &action, Some("cargo fmt --all -- --check")).reason,
        Reason::UnknownCommand
    );
}

#[test]
fn literal_argv_and_existing_exhaustive_policy_keep_separate_meanings() {
    let mut action = formatter();
    action.runner = ActionRunner::Argv {
        program: "cargo".into(),
        args: ["fmt", "--all", "--", "--check"]
            .map(|s| ArgvValue::Literal(s.into()))
            .into(),
        working_directory: None,
        environment: Default::default(),
    };
    action.inputs_policy = Some(ActionInputsPolicy::Exhaustive);
    let result = recommend(8, &action, None);
    assert_eq!(result.reason, Reason::KnownFormatter);
    assert_eq!(
        result.proposed.inputs_policy,
        ActionInputsPolicy::Exhaustive
    );
    assert!(!result.exhaustive_requires_owner_assertion);
    if let ActionRunner::Argv { args, .. } = &mut action.runner {
        args.push(ArgvValue::Literal("--config-path=custom".into()));
    }
    assert_eq!(recommend(8, &action, None).reason, Reason::UnknownCommand);
}
