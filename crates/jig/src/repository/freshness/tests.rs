use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use jig_contract::{ActionEffect, ActionIntent, ActionRunner, ComponentSpec, ProfileSpec};
use serde_json::json;
use tempfile::{TempDir, tempdir};

use super::*;

mod benchmark;
mod conservative;
mod invocation;
mod review_regressions;
mod worktree;

struct Fixture {
    epoch: u32,
    temp: TempDir,
    nested_root: Option<std::path::PathBuf>,
    actions: Vec<ActionSpec>,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempdir().unwrap();
        for directory in [
            ".agent",
            "apps/web/src",
            "shared/fixtures",
            "shared/source",
            "scripts",
            "docs",
        ] {
            fs::create_dir_all(temp.path().join(directory)).unwrap();
        }
        fs::write(
            temp.path().join(".gitignore"),
            "target/\nnode_modules/\n.env*\n*.ignored\n",
        )
        .unwrap();
        fs::write(
            temp.path().join("apps/web/src/page.ts"),
            "export const page = 1;\n",
        )
        .unwrap();
        fs::write(
            temp.path().join("shared/fixtures/schema.txt"),
            "example schema\n",
        )
        .unwrap();
        fs::write(
            temp.path().join("shared/source/model.txt"),
            "example model\n",
        )
        .unwrap();
        fs::write(temp.path().join("docs/guide.md"), "Example guide\n").unwrap();
        fs::write(temp.path().join("scripts/check.sh"), "#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(
                temp.path().join("scripts/check.sh"),
                fs::Permissions::from_mode(0o755),
            )
            .unwrap();
        }
        let mut web = action("web:test", &["apps/web/**", "scripts/check.sh"]);
        web.depends_on
            .push("shared:verify-generated".parse().unwrap());
        let mut shared = action(
            "shared:verify-generated",
            &["shared/fixtures/**", "scripts/check.sh"],
        );
        shared
            .depends_on
            .push("shared:check-model".parse().unwrap());
        let leaf = action(
            "shared:check-model",
            &["shared/source/**", "scripts/check.sh"],
        );
        let fixture = Self {
            epoch: 8,
            temp,
            nested_root: None,
            actions: vec![web, shared, leaf],
        };
        fixture.write_authority();
        git(fixture.root(), &["init", "-q", "-b", "main"]);
        git(fixture.root(), &["config", "user.name", "Jig Test"]);
        git(
            fixture.root(),
            &["config", "user.email", "jig@example.invalid"],
        );
        git(fixture.root(), &["add", "."]);
        git(
            fixture.root(),
            &["commit", "-q", "-m", "ExampleProject fixture"],
        );
        fixture
    }

    fn root(&self) -> &Path {
        self.nested_root
            .as_deref()
            .unwrap_or_else(|| self.temp.path())
    }

    fn write_authority(&self) {
        // Preserve historical fixtures; source-state cases use the live epoch.
        let mut actions = self.actions.clone();
        for action in &mut actions {
            if self.epoch < 8 {
                action.inputs_policy = None;
                action.source_state = None;
            }
        }
        let components = vec![
            ComponentSpec::new("web".parse().unwrap(), "apps/web"),
            ComponentSpec::new("shared".parse().unwrap(), "shared"),
        ];
        let profiles = vec![ProfileSpec::new(
            "verify".parse().unwrap(),
            vec!["web:test".parse().unwrap()],
        )];
        let repository = json!({"components":components,"actions":actions,"profiles":profiles,"default_check_profile":"verify"});
        let config = json!({"repo_name":"ExampleProject","default_branch":"main","commands":{"example_check_command":"true"},"repository":repository});
        fs::write(
            self.root().join(".jig.toml"),
            format!(
                "_src_path = \"/tmp/ExampleTemplate\"\n_commit = \"example\"\n{}",
                toml::to_string(&config).unwrap()
            ),
        )
        .unwrap();
        let mut manifest = repository;
        manifest["contract_version"] = json!(self.epoch);
        manifest["tool_namespace"] = json!("jig");
        manifest["required_commands"] = json!([]);
        manifest["tools"] = json!([]);
        fs::write(
            self.root().join(".agent/jig-contract.json"),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
    }

    fn context(&self) -> RepoContext {
        RepoContext::load_from_root(self.root().to_path_buf()).unwrap()
    }

    fn catalog(&self, ctx: &RepoContext) -> RepositoryCatalog {
        RepositoryCatalog::from_native(
            self.epoch,
            ctx.contract_digest(),
            ctx.component_specs(),
            &self.actions,
            ctx.profile_specs(),
            ctx.default_check_profile(),
        )
        .unwrap()
    }

    fn invocations(&self) -> Vec<PlannedTarget> {
        self.actions
            .iter()
            .map(|action| {
                let mut target = PlannedTarget::new(
                    action.target.clone(),
                    action.intent,
                    action.runner.clone(),
                    "legacy-digest",
                );
                target.effects = action.effects.clone();
                target.inputs = action.inputs.clone();
                target.depends_on = action.depends_on.clone();
                target
            })
            .collect()
    }

    fn collect(&self) -> TargetIdentityCollection {
        self.collect_with_limits(CollectionLimits::with_timeout(Duration::from_millis(
            30_000,
        )))
        .unwrap()
    }

    fn collect_with_limits(
        &self,
        limits: CollectionLimits,
    ) -> CollectionResult<TargetIdentityCollection> {
        let ctx = self.context();
        let catalog = self.catalog(&ctx);
        let whole_repository = if self
            .actions
            .iter()
            .any(|action| action.inputs_policy != Some(ActionInputsPolicy::Exhaustive))
        {
            crate::git_receipts::repository_source_snapshot(self.root())
                .unwrap()
                .worktree_fingerprint
        } else {
            "unused-whole-repository-token".into()
        };
        let mut budget = CollectionBudget::new(limits, &|| false);
        collect_target_identities(
            &ctx,
            &catalog,
            &self.invocations(),
            &whole_repository,
            &mut budget,
        )
    }

    fn identity(&self, target: &str) -> TargetIdentityV1 {
        self.collect()
            .targets
            .remove(&target.parse().unwrap())
            .unwrap()
            .unwrap()
    }
}

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
    action.inputs_policy = Some(ActionInputsPolicy::Exhaustive);
    action.source_state = Some(ActionSourceState::Git);
    action
}

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn unrelated_source_edits_and_commits_preserve_scoped_identity_but_stale_plans() {
    let fixture = Fixture::new();
    let ctx = fixture.context();
    let catalog = fixture.catalog(&ctx);
    let plan = super::super::plan_run(
        &ctx,
        &catalog,
        super::super::PlanRunRequest {
            selectors: vec!["web:test".into()],
            ..Default::default()
        },
    )
    .unwrap();
    let before = fixture.identity("web:test");
    fs::write(
        fixture.root().join("docs/guide.md"),
        "Unrelated guide edit\n",
    )
    .unwrap();
    assert_eq!(
        before.identity_digest,
        fixture.identity("web:test").identity_digest
    );
    assert!(super::super::validate_run_plan_source(&ctx, &plan).is_err());
    git(fixture.root(), &["add", "docs/guide.md"]);
    git(fixture.root(), &["commit", "-q", "-m", "unrelated source"]);
    assert_ne!(before, fixture.identity("web:test"));
    assert!(super::super::validate_run_plan(&ctx, &catalog, &plan).is_err());
}

#[test]
fn direct_edit_add_delete_rename_staging_and_mode_have_distinct_authority() {
    let fixture = Fixture::new();
    let mut previous = fixture.identity("web:test").source_digest;
    let path = fixture.root().join("apps/web/src/page.ts");
    fs::write(&path, "export const page = 2;\n").unwrap();
    let edited = fixture.identity("web:test").source_digest;
    assert_ne!(previous, edited);
    previous = edited;
    git(fixture.root(), &["add", "apps/web/src/page.ts"]);
    let staged = fixture.identity("web:test").source_digest;
    assert_ne!(previous, staged);
    previous = staged;
    fs::write(
        fixture.root().join("apps/web/src/added.ts"),
        "export const added = 1;\n",
    )
    .unwrap();
    let added = fixture.identity("web:test").source_digest;
    assert_ne!(previous, added);
    previous = added;
    fs::rename(&path, fixture.root().join("apps/web/src/renamed.ts")).unwrap();
    let renamed = fixture.identity("web:test").source_digest;
    assert_ne!(previous, renamed);
    previous = renamed;
    fs::remove_file(fixture.root().join("apps/web/src/added.ts")).unwrap();
    assert_ne!(previous, fixture.identity("web:test").source_digest);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let before = fixture.identity("web:test").source_digest;
        fs::set_permissions(
            fixture.root().join("apps/web/src/renamed.ts"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        assert_ne!(before, fixture.identity("web:test").source_digest);
    }
}

#[test]
fn transitive_dependency_authority_and_default_fallback_are_conservative() {
    let mut fixture = Fixture::new();
    let before = fixture.identity("web:test");
    fs::write(
        fixture.root().join("shared/source/model.txt"),
        "changed leaf model\n",
    )
    .unwrap();
    let after = fixture.identity("web:test");
    assert_eq!(before.source_digest, after.source_digest);
    assert_ne!(before.dependency_digest, after.dependency_digest);
    fixture.actions[2].inputs_policy = None;
    fixture.write_authority();
    let fallback = fixture.identity("web:test");
    fs::write(
        fixture.root().join("docs/guide.md"),
        "unrelated to declared inputs\n",
    )
    .unwrap();
    assert_ne!(
        fallback.identity_digest,
        fixture.identity("web:test").identity_digest
    );
}

#[test]
fn empty_match_and_observable_ignored_dotenv_are_real_source_authority() {
    let mut fixture = Fixture::new();
    fixture.actions[0]
        .inputs
        .extend(["apps/web/new/*.ts".into(), ".env.example".into()]);
    fixture.write_authority();
    let before = fixture.identity("web:test");
    fs::create_dir(fixture.root().join("apps/web/new")).unwrap();
    fs::write(
        fixture.root().join("apps/web/new/item.ts"),
        "export const item = 1;\n",
    )
    .unwrap();
    assert_ne!(
        before.source_digest,
        fixture.identity("web:test").source_digest
    );
    fs::write(fixture.root().join(".env.example"), "EXAMPLE_VALUE=one\n").unwrap();
    let dotenv = fixture.identity("web:test");
    fs::write(fixture.root().join(".env.example"), "EXAMPLE_VALUE=two\n").unwrap();
    let changed = fixture.identity("web:test");
    assert_ne!(dotenv.source_digest, changed.source_digest);
    let encoded = serde_json::to_string(&changed).unwrap();
    assert!(!encoded.contains("EXAMPLE_VALUE"));
}

#[test]
fn runner_configuration_and_bound_arguments_change_their_authority() {
    let mut fixture = Fixture::new();
    fixture.actions[0].arguments.insert(
        "value".into(),
        jig_contract::ActionArgumentSpec::String {
            required: false,
            allow_empty: true,
            max_bytes: 100,
        },
    );
    let ActionRunner::Argv { args, .. } = &mut fixture.actions[0].runner else {
        unreachable!()
    };
    args.push(jig_contract::ArgvValue::Argument {
        argument: "value".into(),
    });
    fixture.write_authority();
    let ctx = fixture.context();
    let catalog = fixture.catalog(&ctx);
    let source = crate::git_receipts::repository_source_snapshot(fixture.root()).unwrap();
    let collect = |value: &str| {
        let mut invocations = fixture.invocations();
        invocations[0]
            .arguments
            .insert("value".into(), value.into());
        let mut budget = CollectionBudget::new(
            CollectionLimits::with_timeout(Duration::from_secs(30)),
            &|| false,
        );
        collect_target_identities(
            &ctx,
            &catalog,
            &invocations,
            &source.worktree_fingerprint,
            &mut budget,
        )
        .unwrap()
        .targets
        .remove(&"web:test".parse().unwrap())
        .unwrap()
        .unwrap()
    };
    let first = collect("first");
    let second = collect("second");
    assert_ne!(first.invocation_digest, second.invocation_digest);
    assert_ne!(first.runner_digest, second.runner_digest);
    assert_eq!(first.source_digest, second.source_digest);
    let before = fixture.identity("web:test");
    fs::write(
        fixture.root().join("scripts/check.sh"),
        "#!/bin/sh\n# changed runner\nexit 0\n",
    )
    .unwrap();
    assert_ne!(
        before.identity_digest,
        fixture.identity("web:test").identity_digest
    );
}

#[test]
fn ignored_inputs_and_uncovered_repository_runners_never_get_identity() {
    let mut fixture = Fixture::new();
    fixture.actions[0]
        .inputs
        .push("node_modules/required.js".into());
    fixture.write_authority();
    let failed = fixture
        .collect()
        .targets
        .remove(&"web:test".parse().unwrap())
        .unwrap()
        .unwrap_err();
    assert_eq!(failed.reason.code, FreshnessReasonCode::UnobservableInput);
    fixture.actions[0].inputs = vec!["apps/web/**".into()];
    fixture.write_authority();
    let failed = fixture
        .collect()
        .targets
        .remove(&"web:test".parse().unwrap())
        .unwrap()
        .unwrap_err();
    assert_eq!(failed.reason.code, FreshnessReasonCode::UnobservableInput);
    assert_eq!(failed.reason.path.as_deref(), Some("scripts/check.sh"));
}

#[test]
fn unrelated_ignored_tree_is_pruned_without_spending_its_entry_budget() {
    let fixture = Fixture::new();
    fs::create_dir(fixture.root().join("node_modules")).unwrap();
    for index in 0..2_000 {
        fs::write(
            fixture.root().join(format!("node_modules/item-{index}")),
            "ignored",
        )
        .unwrap();
    }
    let mut limits = CollectionLimits::with_timeout(Duration::from_secs(30));
    limits.entries = 200;
    let result = fixture.collect_with_limits(limits).unwrap();
    assert!(result.targets.values().all(Result::is_ok));
    assert!(result.stats.discovered_entries < 200);
}

#[test]
fn collection_limits_and_cancellation_never_return_partial_identity() {
    let fixture = Fixture::new();
    for change in [0, 1, 2, 3] {
        let mut limits = CollectionLimits::with_timeout(Duration::from_secs(30));
        match change {
            0 => limits.entries = 1,
            1 => limits.bytes = 1,
            2 => limits.git_output = 1,
            _ => limits.targets = 1,
        }
        assert_eq!(
            fixture
                .collect_with_limits(limits)
                .err()
                .unwrap()
                .reason
                .code,
            FreshnessReasonCode::CollectionLimit
        );
    }
    let ctx = fixture.context();
    let catalog = fixture.catalog(&ctx);
    let mut budget = CollectionBudget::new(
        CollectionLimits::with_timeout(Duration::from_secs(30)),
        &|| true,
    );
    assert_eq!(
        collect_target_identities(
            &ctx,
            &catalog,
            &fixture.invocations(),
            "unused",
            &mut budget
        )
        .err()
        .unwrap()
        .reason
        .code,
        FreshnessReasonCode::CollectionFailed
    );
}

#[cfg(unix)]
#[test]
fn relevant_symlink_and_symlinked_ancestor_are_unknown() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    let outside = tempdir().unwrap();
    fs::write(outside.path().join("page.ts"), "outside contents\n").unwrap();
    fs::remove_file(fixture.root().join("apps/web/src/page.ts")).unwrap();
    symlink(
        outside.path().join("page.ts"),
        fixture.root().join("apps/web/src/page.ts"),
    )
    .unwrap();
    let result = fixture
        .collect()
        .targets
        .remove(&"web:test".parse().unwrap())
        .unwrap()
        .unwrap_err();
    assert_eq!(result.reason.code, FreshnessReasonCode::UnobservableInput);
    fs::remove_file(fixture.root().join("apps/web/src/page.ts")).unwrap();
    fs::remove_dir(fixture.root().join("apps/web/src")).unwrap();
    symlink(outside.path(), fixture.root().join("apps/web/src")).unwrap();
    assert!(fixture.collect().targets[&"web:test".parse().unwrap()].is_err());
}

#[test]
fn old_epoch_policy_presence_and_empty_exhaustive_declarations_are_rejected() {
    let mut fixture = Fixture::new();
    for policy in [
        ActionInputsPolicy::WholeRepository,
        ActionInputsPolicy::Exhaustive,
    ] {
        fixture.actions[0].inputs_policy = Some(policy);
        for epoch in 2..8 {
            assert!(validate_inputs_policy(epoch, &fixture.actions[0]).is_err());
        }
        validate_inputs_policy(8, &fixture.actions[0]).unwrap();
    }
    fixture.actions[0].inputs.clear();
    assert!(validate_inputs_policy(8, &fixture.actions[0]).is_err());
    fixture.actions[0].inputs_policy = None;
    validate_inputs_policy(8, &fixture.actions[0]).unwrap();
    let manifest = fixture.root().join(".agent/jig-contract.json");
    let mut value: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
    value["actions"][0]["inputs_policy"] = json!("whole_repository");
    fs::write(manifest, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(RepoContext::load_from_root(fixture.root().to_path_buf()).is_err());
}
