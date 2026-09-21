use std::collections::BTreeMap;

use jig_contract::{ComponentSpec, ProfileSpec, RustNextestConfigV1, SourceIdentity};
use serde_json::json;

use super::*;
use crate::repository::{
    RepositoryCatalog,
    planner::{PlanRunRequest, plan_run_with_source},
};

fn resource() -> ExecutionResourceV1 {
    serde_json::from_value(json!({"kind":"cargo_v1", "workspace_manifest":"Cargo.toml"})).unwrap()
}

fn action() -> ActionSpec {
    let mut action = ActionSpec::new(
        "repo:test".parse().unwrap(),
        ActionIntent::Check,
        ActionRunner::Argv {
            program: "cargo".into(),
            args: vec![jig_contract::ArgvValue::Literal("test".into())],
            working_directory: None,
            environment: BTreeMap::new(),
        },
    );
    action.effects = vec![ActionEffect::ReadOnly, ActionEffect::Process];
    action.resources = vec![resource()];
    action
}

fn catalog(action: &ActionSpec) -> Result<RepositoryCatalog> {
    let profile = ProfileSpec::new("verify".parse().unwrap(), vec![action.target.clone()]);
    RepositoryCatalog::from_native(
        8,
        "example_digest",
        &[ComponentSpec::new("repo".parse().unwrap(), ".")],
        std::slice::from_ref(action),
        std::slice::from_ref(&profile),
        Some(&profile.id),
    )
}

#[test]
fn execution_resources_require_readonly_process_runners_and_bounded_unique_declarations() {
    assert!(catalog(&action()).is_ok());
    for change in 0..7 {
        let mut invalid = action();
        match change {
            0 => invalid.runner = ActionRunner::native(jig_contract::tool::CONTRACT_CHECK),
            1 => invalid.intent = ActionIntent::Generate,
            2 => invalid.effects.push(ActionEffect::Worktree),
            3 => invalid.effects.push(ActionEffect::External),
            4 => invalid.effects = vec![ActionEffect::ReadOnly],
            5 => invalid.resources = vec![resource(); 9],
            6 => invalid.resources = vec![resource(); 2],
            _ => unreachable!(),
        }
        assert!(catalog(&invalid).is_err(), "accepted variant {change}");
    }
    let mut duplicate = action();
    let mut alias = resource();
    let ExecutionResourceV1::CargoV1 {
        workspace_manifest,
        working_directory,
        ..
    } = &mut alias;
    *workspace_manifest = "./Cargo.toml".into();
    *working_directory = Some(".".into());
    duplicate.resources.push(alias);
    assert!(
        catalog(&duplicate)
            .unwrap_err()
            .to_string()
            .contains("duplicate")
    );
}

#[test]
fn execution_resource_paths_context_and_runner_directory_fail_closed() {
    for path in [
        "",
        "/tmp/example/Cargo.toml",
        "../Cargo.toml",
        "C:/example/Cargo.toml",
        "example\\Cargo.toml",
        "Cargo.toml\0",
        "manifest.json",
    ] {
        let mut invalid = action();
        let ExecutionResourceV1::CargoV1 {
            workspace_manifest, ..
        } = &mut invalid.resources[0];
        *workspace_manifest = path.into();
        assert!(catalog(&invalid).is_err(), "accepted {path:?}");
    }
    for cwd in ["..", "/tmp", "C:/example", "example\\nested", "bad\0"] {
        let mut invalid = action();
        let ExecutionResourceV1::CargoV1 {
            working_directory, ..
        } = &mut invalid.resources[0];
        *working_directory = Some(cwd.into());
        assert!(catalog(&invalid).is_err(), "accepted {cwd:?}");
    }
    let mut invalid = action();
    let ExecutionResourceV1::CargoV1 { context, .. } = &mut invalid.resources[0];
    context.metadata_format_version = 2;
    assert!(catalog(&invalid).is_err());
    let mut nested = action();
    let ExecutionResourceV1::CargoV1 {
        workspace_manifest,
        working_directory,
        ..
    } = &mut nested.resources[0];
    *workspace_manifest = "example/Cargo.toml".into();
    *working_directory = Some("example".into());
    assert!(
        catalog(&nested).is_ok(),
        "generic wrapper may declare its inner Cargo working directory"
    );
}

#[test]
fn typed_rust_resource_must_match_declared_runner_manifest_context_and_root() {
    let mut typed = action();
    typed.runner = ActionRunner::RustNextestV1 {
        configuration: RustNextestConfigV1 {
            workspace_manifest: "Cargo.toml".into(),
            focused: false,
            context: Default::default(),
            cargo_profile: None,
            nextest_profile: None,
        },
    };
    assert!(catalog(&typed).is_ok());
    for change in 0..3 {
        let mut invalid = typed.clone();
        let ExecutionResourceV1::CargoV1 {
            workspace_manifest,
            working_directory,
            context,
        } = &mut invalid.resources[0];
        match change {
            0 => *workspace_manifest = "example/Cargo.toml".into(),
            1 => *working_directory = Some("example".into()),
            2 => context.all_features = true,
            _ => unreachable!(),
        }
        assert!(catalog(&invalid).is_err());
    }
}

#[test]
fn planned_resources_bind_plan_and_input_authority_without_dependency_edges() {
    let action = action();
    let source = SourceIdentity::new(None, "example_source");
    let planned = plan_run_with_source(
        &catalog(&action).unwrap(),
        PlanRunRequest::default(),
        source.clone(),
    )
    .unwrap();
    assert_eq!(planned.targets[0].resources, action.resources);
    assert!(planned.targets[0].depends_on.is_empty());
    assert_eq!(planned.execution_layers, vec![vec![action.target.clone()]]);
    let mut changed = action;
    let ExecutionResourceV1::CargoV1 { context, .. } = &mut changed.resources[0];
    context.offline = false;
    let updated = plan_run_with_source(
        &catalog(&changed).unwrap(),
        PlanRunRequest::default(),
        source.clone(),
    )
    .unwrap();
    assert_ne!(planned.id, updated.id);
    assert_ne!(
        planned.targets[0].input_digest,
        updated.targets[0].input_digest
    );
    changed.resources.clear();
    let legacy = plan_run_with_source(
        &catalog(&changed).unwrap(),
        PlanRunRequest::default(),
        source,
    )
    .unwrap();
    assert_ne!(planned.id, legacy.id);
    assert_ne!(
        planned.targets[0].input_digest,
        legacy.targets[0].input_digest
    );
    assert!(
        serde_json::to_value(&legacy.targets[0])
            .unwrap()
            .get("resources")
            .is_none()
    );
}

#[test]
fn execution_resource_replay_and_manifest_authority_reject_changed_coordination() {
    use crate::repository::planner::{plan_run_with_cancellation, validate_run_plan};
    use crate::{context::RepoContext, test_env::TestRepoBuilder};
    use std::{fs, process::Command};

    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let action = action();
    let repository = json!({
        "components":[{"id":"repo", "root":"."}],
        "actions":[action],
        "profiles":[{"id":"verify", "targets":[{"component":"repo", "action":"test"}]}],
        "default_check_profile":"verify"
    });
    let config = toml::Value::try_from(json!({"repository":repository})).unwrap();
    TestRepoBuilder::new(root)
        .contract_version(8)
        .repo_name("ExampleResources")
        .config(toml::to_string(&config).unwrap())
        .write();
    let mut manifest = repository;
    manifest["contract_version"] = json!(8);
    manifest["tool_namespace"] = json!("jig");
    manifest["required_commands"] = json!([]);
    manifest["tools"] = json!([]);
    let manifest_path = root.join(".agent/jig-contract.json");
    fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    fs::write(root.join(".gitignore"), ".agent/state/\n").unwrap();
    let output = Command::new("git")
        .current_dir(root)
        .args(["init", "--quiet"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let ctx = RepoContext::load_from_root(root.to_path_buf()).unwrap();
    let catalog = RepositoryCatalog::from_context(&ctx).unwrap();
    let plan =
        plan_run_with_cancellation(&ctx, &catalog, PlanRunRequest::default(), &|| false).unwrap();
    validate_run_plan(&ctx, &catalog, &plan).unwrap();
    assert_eq!(plan.targets[0].resources, action.resources);
    let mut forged = plan.clone();
    forged.targets[0].resources.clear();
    assert!(
        validate_run_plan(&ctx, &catalog, &forged)
            .unwrap_err()
            .to_string()
            .contains("modified")
    );

    let first_invocation = plan.targets[0]
        .target_identity
        .as_ref()
        .unwrap()
        .invocation_digest
        .clone();
    manifest["actions"][0]["resources"][0]["context"]["offline"] = json!(false);
    fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    assert!(
        RepoContext::load_from_root(root.to_path_buf()).is_err(),
        "source/manifest resource mismatch must fail closed"
    );
    assert!(validate_run_plan(&ctx, &catalog, &plan).is_err());
    let config_path = root.join(".jig.toml");
    let mut config: toml::Value =
        toml::from_str(&fs::read_to_string(&config_path).unwrap()).unwrap();
    config["repository"]["actions"][0]["resources"][0]["context"]["offline"] =
        toml::Value::Boolean(false);
    fs::write(config_path, toml::to_string(&config).unwrap()).unwrap();
    let updated = RepoContext::load_from_root(root.to_path_buf()).unwrap();
    assert_ne!(ctx.contract_digest(), updated.contract_digest());
    assert!(validate_run_plan(&ctx, &catalog, &plan).is_err());
    let updated_catalog = RepositoryCatalog::from_context(&updated).unwrap();
    let next = plan_run_with_cancellation(
        &updated,
        &updated_catalog,
        PlanRunRequest::default(),
        &|| false,
    )
    .unwrap();
    assert_ne!(
        first_invocation,
        next.targets[0]
            .target_identity
            .as_ref()
            .unwrap()
            .invocation_digest
    );
}
