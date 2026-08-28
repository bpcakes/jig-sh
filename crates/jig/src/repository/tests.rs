use jig_contract::{
    ActionEffect, ActionId, ActionIntent, ActionRunner, ActionSpec, ComponentId, ComponentSpec,
    ManifestTool, ProfileId, ProfileSpec, TargetId, kind, tool,
};
use std::collections::BTreeSet;
use tempfile::tempdir;

use crate::{
    context::{MAX_COMMAND_TIMEOUT_SECONDS, RepoContext},
    test_env::TestRepoBuilder,
};

use super::{RepositoryCatalog, unique_legacy_action_id, validate_action_working_directories};

fn command_tool(name: &str, command: &str) -> ManifestTool {
    ManifestTool::new(name, kind::COMMAND, format!("Run {name}.")).with_command(command)
}

#[test]
fn legacy_tools_are_repo_targets_with_bidirectional_aliases() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .contract_version(5)
        .config(
            r#"
[commands]
go_test_command = "go test ./..."

[work]
checks = ["jig.test"]
"#,
        )
        .tool(serde_json::to_value(command_tool("jig.test", "go_test_command")).unwrap())
        .write();
    let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

    let catalog = RepositoryCatalog::from_context(&ctx).unwrap();
    let target: TargetId = "repo:test".parse().unwrap();
    assert_eq!(catalog.contract_version(), 5);
    assert_eq!(catalog.components().count(), 1);
    assert_eq!(catalog.target_for_alias("jig.test"), Some(&target));
    assert_eq!(catalog.aliases_for_target(&target), ["jig.test"]);
    assert_eq!(catalog.default_check_profile().unwrap().as_str(), "verify");
    assert_eq!(
        catalog
            .profile(catalog.default_check_profile().unwrap())
            .unwrap()
            .targets,
        [target]
    );
    assert!(catalog.config_digest().starts_with("sha256:"));
}

#[test]
fn native_catalog_validation_rejects_missing_working_directories_before_planning() {
    let temp = tempdir().unwrap();
    let target: TargetId = "api:test".parse().unwrap();
    let action = ActionSpec::new(
        target,
        ActionIntent::Check,
        ActionRunner::Command {
            command: "go_test_command".into(),
            working_directory: Some("missing".into()),
            environment: Default::default(),
        },
    );

    let error = validate_action_working_directories(temp.path(), &[action])
        .unwrap_err()
        .to_string();

    assert!(error.contains("invalid working_directory"), "{error}");
}

#[test]
fn native_catalog_rejects_actions_without_effect_authority() {
    let component = ComponentSpec::new(ComponentId::parse("api").unwrap(), "api");
    let target: TargetId = "api:operate".parse().unwrap();
    let action = ActionSpec::new(
        target.clone(),
        ActionIntent::Operate,
        ActionRunner::command("operate_command"),
    );
    let profile_id = ProfileId::parse("operate").unwrap();
    let profile = ProfileSpec::new(profile_id.clone(), vec![target]);

    let error = RepositoryCatalog::from_native(
        6,
        "sha256:config",
        &[component],
        &[action],
        &[profile],
        Some(&profile_id),
    )
    .unwrap_err()
    .to_string();

    assert!(
        error.contains("must declare at least one effect"),
        "{error}"
    );
    assert!(
        error.contains("execution isolation and approval"),
        "{error}"
    );
}

#[test]
fn legacy_alias_collisions_get_order_independent_target_ids() {
    let tools = [
        command_tool("jig.foo_bar", "foo_command"),
        command_tool("jig.foo-bar", "bar_command"),
    ];
    let reversed = [tools[1].clone(), tools[0].clone()];

    let first = RepositoryCatalog::from_legacy(5, "digest", &tools, &[]).unwrap();
    let second = RepositoryCatalog::from_legacy(5, "digest", &reversed, &[]).unwrap();
    assert_eq!(
        first.target_for_alias("jig.foo_bar"),
        second.target_for_alias("jig.foo_bar")
    );
    assert_ne!(
        first.target_for_alias("jig.foo_bar"),
        first.target_for_alias("jig.foo-bar")
    );
}

#[test]
fn legacy_default_profile_skips_effectful_generated_check_gates() {
    let tools = [
        command_tool("jig.test", "test_command"),
        command_tool("jig.schema_dump", "schema_dump_command"),
    ];
    let catalog = RepositoryCatalog::from_legacy(
        5,
        "digest",
        &tools,
        &["jig.test".into(), "jig.schema_dump".into()],
    )
    .unwrap();
    let profile = catalog
        .profile(catalog.default_check_profile().unwrap())
        .unwrap();

    assert_eq!(profile.targets, ["repo:test".parse().unwrap()]);
    let schema_dump = catalog
        .action(catalog.target_for_alias("jig.schema_dump").unwrap())
        .unwrap();
    assert_eq!(schema_dump.intent, ActionIntent::Generate);
    assert!(schema_dump.effects.contains(&ActionEffect::Worktree));
}

#[test]
fn legacy_default_profile_rejects_unknown_effectful_check_gates() {
    let tools = [command_tool("jig.migration_add", "migration_add_command")];

    let error = RepositoryCatalog::from_legacy(5, "digest", &tools, &["jig.migration_add".into()])
        .unwrap_err();

    assert!(error.to_string().contains("is not a read-only check"));
}

#[test]
fn legacy_action_id_fallback_never_returns_an_occupied_digest_candidate() {
    let base = ActionId::parse("foo-bar").unwrap();
    let mut occupied = BTreeSet::from([base]);
    let first_fallback = unique_legacy_action_id("jig.foo-bar", &occupied).unwrap();
    occupied.insert(first_fallback.clone());

    let second_fallback = unique_legacy_action_id("jig.foo-bar", &occupied).unwrap();

    assert_ne!(first_fallback, second_fallback);
    assert!(!occupied.contains(&second_fallback));
}

#[test]
fn native_catalog_keeps_same_action_distinct_per_component() {
    let api = ComponentSpec::new(ComponentId::parse("api").unwrap(), "api");
    let web = ComponentSpec::new(ComponentId::parse("web").unwrap(), "web");
    let api_target: TargetId = "api:test".parse().unwrap();
    let web_target: TargetId = "web:test".parse().unwrap();
    let mut api_test = ActionSpec::new(
        api_target.clone(),
        ActionIntent::Check,
        ActionRunner::command("go_test_command"),
    );
    api_test.effects = vec![ActionEffect::ReadOnly, ActionEffect::Process];
    api_test.legacy_aliases.push("jig.test".into());
    let mut web_test = ActionSpec::new(
        web_target.clone(),
        ActionIntent::Check,
        ActionRunner::command("typescript_test_command"),
    );
    web_test.effects = vec![ActionEffect::ReadOnly, ActionEffect::Process];
    let profile_id = ProfileId::parse("verify").unwrap();
    let profile = ProfileSpec::new(
        profile_id.clone(),
        vec![api_target.clone(), web_target.clone()],
    );

    let catalog = RepositoryCatalog::from_native(
        6,
        "sha256:config",
        &[api, web],
        &[api_test, web_test],
        &[profile],
        Some(&profile_id),
    )
    .unwrap();

    assert!(catalog.action(&api_target).is_some());
    assert!(catalog.action(&web_target).is_some());
    assert_eq!(catalog.actions().count(), 2);
    assert_eq!(catalog.target_for_alias("jig.test"), Some(&api_target));
}

#[test]
fn native_catalog_rejects_aliases_that_are_canonical_selectors() {
    for alias in ["test", "web:test", "*:test", "web:*"] {
        let component = ComponentSpec::new(ComponentId::parse("web").unwrap(), "web");
        let target: TargetId = "web:lint".parse().unwrap();
        let mut action = ActionSpec::new(
            target.clone(),
            ActionIntent::Check,
            ActionRunner::command("typescript_lint_command"),
        );
        action.effects = vec![ActionEffect::ReadOnly, ActionEffect::Process];
        action.legacy_aliases.push(alias.into());
        let profile_id = ProfileId::parse("verify").unwrap();
        let profile = ProfileSpec::new(profile_id.clone(), vec![target]);

        let error = RepositoryCatalog::from_native(
            6,
            "sha256:config",
            &[component],
            &[action],
            &[profile],
            Some(&profile_id),
        )
        .unwrap_err()
        .to_string();

        assert!(
            error.contains("reserved for canonical repository selectors"),
            "alias {alias:?} was accepted: {error}"
        );
    }
}

#[test]
fn native_catalog_rejects_cross_record_drift() {
    let repo = ComponentSpec::new(ComponentId::parse("repo").unwrap(), ".");
    let missing_target: TargetId = "missing:test".parse().unwrap();
    let action = ActionSpec::new(
        missing_target,
        ActionIntent::Check,
        ActionRunner::native("contract"),
    );
    let profile = ProfileSpec::new(
        ProfileId::parse("verify").unwrap(),
        vec!["missing:test".parse().unwrap()],
    );

    let error = RepositoryCatalog::from_native(
        6,
        "digest",
        &[repo],
        &[action],
        &[profile],
        Some(&ProfileId::parse("verify").unwrap()),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("unknown component 'missing'"));
}

#[test]
fn native_catalog_rejects_component_dependency_cycles() {
    let mut api = ComponentSpec::new(ComponentId::parse("api").unwrap(), "api");
    let mut web = ComponentSpec::new(ComponentId::parse("web").unwrap(), "web");
    api.depends_on.push(web.id.clone());
    web.depends_on.push(api.id.clone());
    let target: TargetId = "api:test".parse().unwrap();
    let action = ActionSpec::new(
        target.clone(),
        ActionIntent::Check,
        ActionRunner::command("test_command"),
    );
    let profile_id = ProfileId::parse("verify").unwrap();
    let profile = ProfileSpec::new(profile_id.clone(), vec![target]);

    let error = RepositoryCatalog::from_native(
        6,
        "digest",
        &[api, web],
        &[action],
        &[profile],
        Some(&profile_id),
    )
    .unwrap_err()
    .to_string();

    assert_eq!(error, "component dependency cycle: api -> web -> api");
}

#[test]
fn native_catalog_rejects_action_dependency_cycles() {
    let component = ComponentSpec::new(ComponentId::parse("api").unwrap(), "api");
    let lint_target: TargetId = "api:lint".parse().unwrap();
    let test_target: TargetId = "api:test".parse().unwrap();
    let mut lint = ActionSpec::new(
        lint_target.clone(),
        ActionIntent::Check,
        ActionRunner::command("lint_command"),
    );
    lint.effects = vec![ActionEffect::ReadOnly, ActionEffect::Process];
    lint.depends_on.push(test_target.clone());
    let mut test = ActionSpec::new(
        test_target.clone(),
        ActionIntent::Check,
        ActionRunner::command("test_command"),
    );
    test.effects = vec![ActionEffect::ReadOnly, ActionEffect::Process];
    test.depends_on.push(lint_target);
    let profile_id = ProfileId::parse("verify").unwrap();
    let profile = ProfileSpec::new(profile_id.clone(), vec![test_target]);

    let error = RepositoryCatalog::from_native(
        6,
        "digest",
        &[component],
        &[lint, test],
        &[profile],
        Some(&profile_id),
    )
    .unwrap_err()
    .to_string();

    assert_eq!(
        error,
        "action dependency cycle: api:lint -> api:test -> api:lint"
    );
}

#[test]
fn native_catalog_rejects_invalid_action_timeouts() {
    for timeout in [0, MAX_COMMAND_TIMEOUT_SECONDS + 1] {
        let repo = ComponentSpec::new(ComponentId::parse("repo").unwrap(), ".");
        let target: TargetId = "repo:test".parse().unwrap();
        let mut action = ActionSpec::new(
            target.clone(),
            ActionIntent::Check,
            ActionRunner::command("rust_test_command"),
        );
        action.effects = vec![ActionEffect::ReadOnly, ActionEffect::Process];
        action.timeout_seconds = Some(timeout);
        let profile_id = ProfileId::parse("verify").unwrap();
        let profile = ProfileSpec::new(profile_id.clone(), vec![target]);

        let error = RepositoryCatalog::from_native(
            6,
            "digest",
            &[repo],
            &[action],
            &[profile],
            Some(&profile_id),
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("timeout_seconds must be between 1"));
    }
}

#[test]
fn native_catalog_rejects_unpreemptible_native_timeout_overrides() {
    let repo = ComponentSpec::new(ComponentId::parse("repo").unwrap(), ".");
    let target: TargetId = "repo:contract".parse().unwrap();
    let mut action = ActionSpec::new(
        target.clone(),
        ActionIntent::Check,
        ActionRunner::native(tool::CONTRACT_CHECK),
    );
    action.effects = vec![ActionEffect::ReadOnly];
    action.timeout_seconds = Some(30);
    let profile_id = ProfileId::parse("verify").unwrap();
    let profile = ProfileSpec::new(profile_id.clone(), vec![target]);

    let error = RepositoryCatalog::from_native(
        6,
        "digest",
        &[repo],
        &[action],
        &[profile],
        Some(&profile_id),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("cannot be preempted safely"));
}

#[test]
fn native_catalog_allows_mixed_database_adapters_with_one_migration_owner() {
    let api = ComponentSpec {
        adapters: vec!["go".into(), "go-postgres".into()],
        ..ComponentSpec::new(ComponentId::parse("api").unwrap(), "services/api")
    };
    let worker = ComponentSpec {
        adapters: vec!["rust".into(), "sqlx".into()],
        ..ComponentSpec::new(ComponentId::parse("worker").unwrap(), "services/worker")
    };
    let target: TargetId = "worker:migration-add".parse().unwrap();
    let action = ActionSpec::new(
        target.clone(),
        ActionIntent::Generate,
        ActionRunner::native(tool::MIGRATION_ADD),
    );
    let action = ActionSpec {
        effects: vec![ActionEffect::Worktree, ActionEffect::Process],
        ..action
    };
    let profile_id = ProfileId::parse("operate").unwrap();
    let profile = ProfileSpec::new(profile_id.clone(), vec![target]);

    RepositoryCatalog::from_native(
        6,
        "digest",
        &[api, worker],
        &[action],
        &[profile],
        Some(&profile_id),
    )
    .unwrap();
}

#[test]
fn native_catalog_rejects_incorrect_migration_mutation_semantics() {
    let component = ComponentSpec {
        adapters: vec!["rust".into(), "sqlx".into()],
        ..ComponentSpec::new(ComponentId::parse("api").unwrap(), "services/api")
    };
    let target: TargetId = "api:migration-add".parse().unwrap();
    let profile_id = ProfileId::parse("operate").unwrap();
    let profile = ProfileSpec::new(profile_id.clone(), vec![target.clone()]);

    for (intent, effects, expected) in [
        (
            ActionIntent::Check,
            vec![ActionEffect::Worktree],
            "must declare intent 'generate'",
        ),
        (
            ActionIntent::Generate,
            vec![ActionEffect::Process],
            "must declare the 'worktree' effect",
        ),
    ] {
        let mut action = ActionSpec::new(
            target.clone(),
            intent,
            ActionRunner::native(tool::MIGRATION_ADD),
        );
        action.effects = effects;

        let error = RepositoryCatalog::from_native(
            6,
            "digest",
            std::slice::from_ref(&component),
            &[action],
            std::slice::from_ref(&profile),
            Some(&profile_id),
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn native_catalog_rejects_multiple_migration_format_owners() {
    let api = ComponentSpec {
        adapters: vec!["go".into(), "go-postgres".into()],
        ..ComponentSpec::new(ComponentId::parse("api").unwrap(), "services/api")
    };
    let worker = ComponentSpec {
        adapters: vec!["rust".into(), "sqlx".into()],
        ..ComponentSpec::new(ComponentId::parse("worker").unwrap(), "services/worker")
    };
    let goose_target: TargetId = "api:migration-add".parse().unwrap();
    let sqlx_target: TargetId = "worker:migration-add".parse().unwrap();
    let mut goose = ActionSpec::new(
        goose_target.clone(),
        ActionIntent::Generate,
        ActionRunner::native(tool::MIGRATION_ADD),
    );
    goose.effects = vec![ActionEffect::Worktree, ActionEffect::Process];
    let mut sqlx = ActionSpec::new(
        sqlx_target.clone(),
        ActionIntent::Generate,
        ActionRunner::native(tool::MIGRATION_ADD),
    );
    sqlx.effects = vec![ActionEffect::Worktree, ActionEffect::Process];
    let profile_id = ProfileId::parse("operate").unwrap();
    let profile = ProfileSpec::new(profile_id.clone(), vec![goose_target, sqlx_target]);

    let error = RepositoryCatalog::from_native(
        6,
        "digest",
        &[api, worker],
        &[goose, sqlx],
        &[profile],
        Some(&profile_id),
    )
    .unwrap_err()
    .to_string();

    assert!(
        error.contains("multiple migration authoring targets"),
        "{error}"
    );
    assert!(error.contains("api:migration-add"), "{error}");
    assert!(error.contains("worker:migration-add"), "{error}");
}
