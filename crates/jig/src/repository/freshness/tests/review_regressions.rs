use super::*;

#[test]
fn missing_ignored_directory_glob_is_unknown_and_does_not_block_planning() {
    let mut fixture = Fixture::new();
    fixture.actions[0].inputs.push("node_modules/**".into());
    fixture.actions[2].inputs_policy = None;
    fixture.write_authority();
    let ctx = fixture.context();
    let catalog = fixture.catalog(&ctx);
    let plan = super::super::super::plan_run(
        &ctx,
        &catalog,
        super::super::super::PlanRunRequest::default(),
    )
    .unwrap();
    let web = plan
        .targets
        .iter()
        .find(|target| target.target.to_string() == "web:test")
        .unwrap();
    assert!(web.target_identity.is_none());
    assert_eq!(
        web.target_identity_error.as_ref().unwrap().code,
        FreshnessReasonCode::UnobservableInput
    );
    let leaf = plan
        .targets
        .iter()
        .find(|target| target.target.to_string() == "shared:check-model")
        .unwrap();
    assert!(leaf.target_identity.is_some());
    assert!(
        plan.targets
            .iter()
            .filter_map(|target| target.target_identity.as_ref())
            .all(|identity| identity.source_preview.is_empty())
    );
    super::super::super::validate_run_plan(&ctx, &catalog, &plan).unwrap();
}

#[cfg(unix)]
#[test]
fn symlink_before_parent_component_cannot_attest_a_different_runner() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let mut fixture = Fixture::new();
    fs::create_dir_all(fixture.root().join("tools/subdir")).unwrap();
    fs::write(fixture.root().join("tools/check.sh"), "#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(
        fixture.root().join("tools/check.sh"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    symlink("../tools/subdir", fixture.root().join("scripts/link")).unwrap();
    let ActionRunner::Argv { program, .. } = &mut fixture.actions[0].runner else {
        unreachable!()
    };
    *program = "scripts/link/../check.sh".into();
    fixture.write_authority();
    assert!(fixture.collect().targets[&"web:test".parse().unwrap()].is_err());
}

#[cfg(unix)]
#[test]
fn whole_repository_keeps_symlinked_executable_and_cwd_compatibility() {
    use std::os::unix::fs::symlink;
    let mut fixture = Fixture::new();
    symlink("check.sh", fixture.root().join("scripts/alias.sh")).unwrap();
    symlink("apps/web", fixture.root().join("app-alias")).unwrap();
    for cwd in [false, true] {
        let ActionRunner::Argv {
            program,
            working_directory,
            ..
        } = &mut fixture.actions[0].runner
        else {
            unreachable!()
        };
        *program = if cwd {
            "../../scripts/check.sh"
        } else {
            "scripts/alias.sh"
        }
        .into();
        *working_directory = cwd.then(|| "app-alias".into());
        for policy in [
            None,
            Some(ActionInputsPolicy::WholeRepository),
            Some(ActionInputsPolicy::Exhaustive),
        ] {
            fixture.actions[0].inputs_policy = policy;
            fixture.write_authority();
            assert_eq!(
                fixture.collect().targets[&"web:test".parse().unwrap()].is_ok(),
                policy != Some(ActionInputsPolicy::Exhaustive)
            );
        }
    }
}

#[test]
fn nested_jig_root_uses_one_namespace_for_committed_index_and_current_paths() {
    let mut fixture = Fixture::new();
    let nested = fixture.root().join("ExampleNested");
    fs::create_dir(&nested).unwrap();
    for entry in fs::read_dir(fixture.root()).unwrap() {
        let entry = entry.unwrap();
        if entry.file_name() != ".git" && entry.path() != nested {
            fs::rename(entry.path(), nested.join(entry.file_name())).unwrap();
        }
    }
    git(fixture.root(), &["add", "--all"]);
    git(
        fixture.root(),
        &["commit", "-q", "-m", "Nested generic repository"],
    );
    fixture.nested_root = Some(nested);
    let before = fixture.identity("web:test");
    assert!(
        before
            .source_preview
            .iter()
            .all(|entry| !entry.path.starts_with("ExampleNested/"))
    );
    let path = fixture.root().join("apps/web/src/page.ts");
    fs::write(&path, "staged nested change").unwrap();
    git(fixture.root(), &["add", "apps/web/src/page.ts"]);
    fs::write(&path, "export const page = 1;\n").unwrap();
    assert_ne!(
        before.source_digest,
        fixture.identity("web:test").source_digest
    );
    fs::write(fixture.root().join("apps/web/src/pending.ts"), "pending").unwrap();
    git(
        fixture.root(),
        &["add", "--intent-to-add", "apps/web/src/pending.ts"],
    );
    assert!(fixture.collect().targets[&"web:test".parse().unwrap()].is_err());
}

#[test]
fn unrelated_tracked_tree_does_not_spend_the_git_entry_budget() {
    let fixture = Fixture::new();
    fs::create_dir(fixture.root().join("unrelated")).unwrap();
    for index in 0..2_000 {
        fs::write(
            fixture.root().join(format!("unrelated/example-{index}")),
            "unrelated",
        )
        .unwrap();
    }
    git(fixture.root(), &["add", "unrelated"]);
    git(
        fixture.root(),
        &["commit", "-q", "-m", "Unrelated generic inputs"],
    );
    let mut limits = CollectionLimits::with_timeout(Duration::from_secs(30));
    limits.entries = 200;
    assert!(
        fixture
            .collect_with_limits(limits)
            .unwrap()
            .targets
            .values()
            .all(Result::is_ok)
    );
}

#[test]
fn shell_text_and_declared_helper_contents_are_both_authority() {
    let mut fixture = Fixture::new();
    fixture.actions[0].runner = ActionRunner::Shell {
        command: "example_check_command".into(),
        working_directory: None,
        environment: BTreeMap::new(),
    };
    fixture.write_authority();
    let before = fixture.identity("web:test");
    let config = fixture.root().join(".jig.toml");
    let changed = fs::read_to_string(&config).unwrap().replace(
        "example_check_command = \"true\"",
        "example_check_command = \"scripts/check.sh\"",
    );
    fs::write(config, changed).unwrap();
    let command = fixture.identity("web:test");
    assert_ne!(before.runner_digest, command.runner_digest);
    fs::write(
        fixture.root().join("scripts/check.sh"),
        "#!/bin/sh\n# changed helper\nexit 0\n",
    )
    .unwrap();
    assert_ne!(
        command.source_digest,
        fixture.identity("web:test").source_digest
    );
}

#[test]
fn cancelled_planning_and_earlier_collection_deadline_never_publish_an_identity() {
    let fixture = Fixture::new();
    let ctx = fixture.context();
    let catalog = fixture.catalog(&ctx);
    let mut plan = crate::repository::plan_run_with_cancellation(
        &ctx,
        &catalog,
        crate::repository::PlanRunRequest::default(),
        &|| true,
    )
    .unwrap();
    assert!(
        plan.targets
            .iter()
            .all(|target| target.target_identity.is_none()
                && target.target_identity_error.as_ref().unwrap().code
                    == FreshnessReasonCode::CollectionFailed)
    );
    let mut budget =
        CollectionBudget::new(CollectionLimits::with_timeout(Duration::ZERO), &|| false);
    prepare_plan_identities(&ctx, &catalog, &mut plan, &mut budget).unwrap();
    assert!(
        plan.targets
            .iter()
            .all(|target| target.target_identity.is_none()
                && target.target_identity_error.as_ref().unwrap().code
                    == FreshnessReasonCode::CollectionLimit)
    );
    assert_eq!(budget.stats.discovered_entries, 0);
}

#[test]
fn unicode_diagnostic_previews_obey_byte_limits() {
    let failure = CollectionFailure::new(FreshnessReasonCode::CollectionFailed, &"界".repeat(1000))
        .at(&"界".repeat(1000));
    assert!(failure.message.len() <= 1000);
    assert!(failure.reason.path.unwrap().len() <= 512);
}

#[test]
fn real_unmerged_index_entries_are_unknown_only_for_relevant_targets() {
    let fixture = Fixture::new();
    let oid = Command::new("git")
        .current_dir(fixture.root())
        .args(["rev-parse", "HEAD:apps/web/src/page.ts"])
        .output()
        .unwrap();
    assert!(oid.status.success());
    let oid = String::from_utf8(oid.stdout).unwrap();
    for path in ["docs/conflict.md", "apps/web/src/conflict.ts"] {
        use std::io::Write;
        let mut process = Command::new("git")
            .current_dir(fixture.root())
            .args(["update-index", "--index-info"])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        {
            let mut input = process.stdin.take().unwrap();
            for stage in 1..=3 {
                writeln!(input, "100644 {} {stage}\t{path}", oid.trim()).unwrap();
            }
        }
        assert!(process.wait().unwrap().success());
        let result = fixture.collect();
        assert_eq!(
            result.targets[&"web:test".parse().unwrap()].is_ok(),
            path.starts_with("docs/")
        );
        assert!(result.targets[&"shared:check-model".parse().unwrap()].is_ok());
    }
}

#[test]
fn unborn_and_absent_git_authority_never_produce_partial_scoped_identity() {
    let fixture = Fixture::new();
    let ctx = fixture.context();
    git(
        fixture.root(),
        &["symbolic-ref", "HEAD", "refs/heads/ExampleUnborn"],
    );
    for remove_git in [false, true] {
        if remove_git {
            fs::remove_dir_all(fixture.root().join(".git")).unwrap();
        }
        let mut budget = CollectionBudget::new(
            CollectionLimits::with_timeout(Duration::from_secs(30)),
            &|| false,
        );
        let result = source::SourceSnapshot::capture(&ctx, &[&fixture.actions[0]], &mut budget);
        assert!(result.is_err());
    }
}

#[test]
fn optional_proof_availability_and_submitted_proof_cannot_change_execution_authority() {
    let fixture = Fixture::new();
    let ctx = fixture.context();
    let catalog = fixture.catalog(&ctx);
    let complete =
        crate::repository::plan_run(&ctx, &catalog, crate::repository::PlanRunRequest::default())
            .unwrap();
    let incomplete = crate::repository::plan_run_with_cancellation(
        &ctx,
        &catalog,
        crate::repository::PlanRunRequest::default(),
        &|| true,
    )
    .unwrap();
    assert_eq!(complete.id, incomplete.id);
    let clean = crate::repository::validate_run_plan(&ctx, &catalog, &complete).unwrap();
    assert_eq!(
        clean,
        crate::repository::validate_run_plan(&ctx, &catalog, &incomplete).unwrap()
    );
    assert!(
        clean.targets.iter().all(
            |target| target.target_identity.is_none() && target.target_identity_error.is_none()
        )
    );
    let mut forged = complete;
    forged.targets[0]
        .target_identity
        .as_mut()
        .unwrap()
        .identity_digest = "untrusted-client-proof".into();
    assert_eq!(
        crate::repository::validate_run_plan(&ctx, &catalog, &forged).unwrap(),
        clean
    );
    forged.targets[0]
        .arguments
        .insert("undeclared".into(), "changed".into());
    assert!(crate::repository::validate_run_plan(&ctx, &catalog, &forged).is_err());
    fs::write(
        fixture.root().join("docs/guide.md"),
        "unrelated edit still stales execution plans",
    )
    .unwrap();
    assert!(crate::repository::validate_run_plan(&ctx, &catalog, &incomplete).is_err());
}

#[cfg(unix)]
#[test]
fn argv_path_fallthrough_covers_every_reachable_repository_candidate() {
    use std::os::unix::fs::PermissionsExt;
    let mut fixture = Fixture::new();
    fs::create_dir(fixture.root().join("fallback")).unwrap();
    fs::write(
        fixture.root().join("scripts/example-check"),
        "#!/nonexistent-example-interpreter\n",
    )
    .unwrap();
    fs::write(
        fixture.root().join("fallback/example-check"),
        "#!/bin/sh\nprintf 'fallback executed'\n",
    )
    .unwrap();
    for name in ["scripts/example-check", "fallback/example-check"] {
        fs::set_permissions(fixture.root().join(name), fs::Permissions::from_mode(0o755)).unwrap();
    }
    let ActionRunner::Argv {
        program,
        environment,
        ..
    } = &mut fixture.actions[0].runner
    else {
        unreachable!()
    };
    *program = "example-check".into();
    environment.insert("PATH".into(), "scripts:fallback:missing-bin".into());
    fixture.actions[0]
        .inputs
        .push("scripts/example-check".into());
    fixture.write_authority();
    let mut process = Command::new("example-check");
    process
        .current_dir(fixture.root())
        .env("PATH", "scripts:fallback:missing-bin");
    crate::repository::runners::prepare_literal_exec(&mut process).unwrap();
    let executed = process.output().unwrap();
    assert!(executed.status.success());
    assert_eq!(executed.stdout, b"fallback executed");
    let failed = fixture.collect();
    assert_eq!(
        failed.targets[&"web:test".parse().unwrap()]
            .as_ref()
            .unwrap_err()
            .reason
            .path
            .as_deref(),
        Some("fallback/example-check")
    );
    fixture.actions[0]
        .inputs
        .extend(["fallback/**".into(), "missing-bin/**".into()]);
    fixture.write_authority();
    let before = fixture.identity("web:test").identity_digest;
    fs::write(
        fixture.root().join("fallback/example-check"),
        "#!/bin/sh\nexit 4\n",
    )
    .unwrap();
    assert_ne!(before, fixture.identity("web:test").identity_digest);
    let before = fixture.identity("web:test").identity_digest;
    fs::create_dir(fixture.root().join("missing-bin")).unwrap();
    fs::write(
        fixture.root().join("missing-bin/example-check"),
        "#!/bin/sh\nexit 0\n",
    )
    .unwrap();
    assert_ne!(before, fixture.identity("web:test").identity_digest);
}

#[cfg(unix)]
#[test]
fn unsupported_paths_outside_inputs_do_not_poison_an_independent_target() {
    use std::os::unix::ffi::OsStrExt;
    let fixture = Fixture::new();
    fs::create_dir(fixture.root().join("apps/legacy")).unwrap();
    for name in [
        b"odd\\name.txt".as_slice(),
        // macOS CI rejects this non-UTF-8 fixture name at file creation.
        #[cfg(not(target_os = "macos"))]
        b"odd\xff.txt",
        b"odd\nname.txt",
    ] {
        let name = std::ffi::OsStr::from_bytes(name);
        let path = fixture.root().join("apps/legacy").join(name);
        fs::write(&path, "unrelated").unwrap();
        let added = Command::new("git")
            .current_dir(fixture.root())
            .arg("add")
            .arg("--")
            .arg(&path)
            .status()
            .unwrap();
        assert!(added.success());
        assert!(fixture.collect().targets[&"web:test".parse().unwrap()].is_ok());
        fs::write(fixture.root().join("apps/web/src").join(name), "relevant").unwrap();
        assert_eq!(
            fixture.collect().targets[&"web:test".parse().unwrap()]
                .as_ref()
                .unwrap_err()
                .reason
                .code,
            FreshnessReasonCode::UnobservableInput
        );
        fs::remove_file(fixture.root().join("apps/web/src").join(name)).unwrap();
    }
}

#[test]
fn shallow_globs_do_not_walk_unrelated_deeper_trees() {
    let mut fixture = Fixture::new();
    fixture.actions[0].inputs = vec!["apps/web/src/*.ts".into(), "scripts/check.sh".into()];
    fixture.write_authority();
    fs::create_dir(fixture.root().join("apps/web/src/deep")).unwrap();
    for index in 0..2000 {
        fs::write(
            fixture
                .root()
                .join(format!("apps/web/src/deep/example-{index}")),
            "unrelated",
        )
        .unwrap();
    }
    let collection = fixture
        .collect_with_limits(CollectionLimits {
            entries: 200,
            ..CollectionLimits::with_timeout(Duration::from_secs(30))
        })
        .unwrap();
    assert!(collection.targets[&"web:test".parse().unwrap()].is_ok());
}

#[test]
fn nested_plain_repository_metadata_is_unknown_without_recursive_authority() {
    let fixture = Fixture::new();
    fs::create_dir(fixture.root().join("apps/web/module")).unwrap();
    git(&fixture.root().join("apps/web/module"), &["init", "-q"]);
    assert_eq!(
        fixture.collect().targets[&"web:test".parse().unwrap()]
            .as_ref()
            .unwrap_err()
            .reason
            .code,
        FreshnessReasonCode::UnobservableInput
    );
    assert!(fixture.collect().targets[&"shared:check-model".parse().unwrap()].is_ok());
}

#[test]
fn assume_unchanged_never_substitutes_index_metadata_for_current_bytes() {
    let fixture = Fixture::new();
    let before = fixture.identity("web:test").identity_digest;
    git(
        fixture.root(),
        &["update-index", "--assume-unchanged", "apps/web/src/page.ts"],
    );
    fs::write(
        fixture.root().join("apps/web/src/page.ts"),
        "modified behind index hint",
    )
    .unwrap();
    assert_ne!(before, fixture.identity("web:test").identity_digest);
}

#[test]
fn git_diagnostics_explain_the_failure_without_recording_raw_values() {
    let fixture = Fixture::new();
    let mut command = Command::new("bash");
    command.args(["--noprofile", "--norc", "-c", "printf 'warning: unable to access /private/ExampleCredential: Permission denied\\n' >&2; exit 7"]);
    crate::shell::sanitize_bash_environment(&mut command);
    let error = crate::git_receipts::read_freshness_git_batch(
        fixture.root(),
        &mut command,
        1024,
        Duration::from_secs(30),
        &|| false,
    )
    .unwrap_err();
    let detail = error
        .downcast_ref::<crate::git_receipts::FreshnessGitObservationFailure>()
        .unwrap();
    assert!(detail.0.contains("permissions"));
    assert!(detail.0.contains('7'));
    assert!(!detail.0.contains("ExampleCredential"));
}

#[test]
fn complete_source_tokens_keep_bounded_serialized_path_previews() {
    let fixture = Fixture::new();
    for index in 0..120 {
        fs::write(
            fixture.root().join(format!(
                "apps/web/src/example-{index:03}-documentation-name.ts"
            )),
            "example",
        )
        .unwrap();
    }
    let identity = fixture.identity("web:test");
    assert!(identity.source_entry_count > identity.source_preview.len() as u64);
    assert!(identity.source_preview_truncated);
    assert!(
        serde_json::to_vec(&identity.source_preview).unwrap().len()
            <= jig_contract::freshness::MAX_FRESHNESS_DIAGNOSTIC_BYTES
    );
    assert!(identity.identity_digest.starts_with("sha256:"));
}

#[cfg(unix)]
#[test]
fn ignored_working_directory_is_revalidated_after_runner_collection() {
    use std::os::unix::fs::symlink;
    let mut fixture = Fixture::new();
    fs::create_dir_all(fixture.root().join("scratch/cwd")).unwrap();
    fs::create_dir_all(fixture.root().join("scratch/other")).unwrap();
    fs::write(fixture.root().join(".gitignore"), "scratch/\n").unwrap();
    fixture.actions[0].runner = ActionRunner::Argv {
        program: "/bin/sh".into(),
        args: vec![
            jig_contract::ArgvValue::Literal("-c".into()),
            jig_contract::ArgvValue::Literal("exit 0".into()),
        ],
        working_directory: Some("scratch/cwd".into()),
        environment: BTreeMap::new(),
    };
    fixture.write_authority();
    let ctx = fixture.context();
    let catalog = fixture.catalog(&ctx);
    let mut budget = CollectionBudget::new(
        CollectionLimits::with_timeout(Duration::from_secs(30)),
        &|| false,
    );
    let mut snapshot = source::SourceSnapshot::capture(
        &ctx,
        &fixture.actions.iter().collect::<Vec<_>>(),
        &mut budget,
    )
    .unwrap();
    authority::collect(
        &ctx,
        &catalog,
        &fixture.actions[0],
        &fixture.invocations()[0],
        &mut snapshot,
        &mut budget,
    )
    .unwrap();
    snapshot.revalidate(&ctx, &mut budget).unwrap();
    fs::remove_dir(fixture.root().join("scratch/cwd")).unwrap();
    symlink("other", fixture.root().join("scratch/cwd")).unwrap();
    assert_eq!(
        snapshot
            .revalidate(&ctx, &mut budget)
            .unwrap_err()
            .reason
            .code,
        FreshnessReasonCode::SourceRaced
    );
}

#[test]
fn successful_git_with_warnings_is_explicitly_unknown() {
    let fixture = Fixture::new();
    let mut command = Command::new("bash");
    command.args(["--noprofile", "--norc", "-c", "printf 'warning: unable to access example configuration: Permission denied\\n' >&2; exit 0"]);
    crate::shell::sanitize_bash_environment(&mut command);
    let error = crate::git_receipts::read_freshness_git_batch(
        fixture.root(),
        &mut command,
        1024,
        Duration::from_secs(30),
        &|| false,
    )
    .unwrap_err();
    assert!(error.is::<crate::git_receipts::FreshnessGitObservationFailure>());
    assert!(error.to_string().contains("permissions"));
}

#[test]
fn whole_dependency_revalidation_rejects_an_edit_outside_exhaustive_inputs() {
    let mut fixture = Fixture::new();
    fixture.actions[2].inputs_policy = None;
    fixture.write_authority();
    let ctx = fixture.context();
    let catalog = fixture.catalog(&ctx);
    let invocations = fixture.invocations();
    let token = crate::state::current_worktree_fingerprint(&ctx)
        .fingerprint
        .unwrap();
    let budget = CollectionBudget::new(
        CollectionLimits::with_timeout(Duration::from_secs(30)),
        &|| false,
    );
    revalidate_whole_source(&ctx, &catalog, &invocations, Some(&token), &budget).unwrap();
    assert_eq!(
        revalidate_whole_source(&ctx, &catalog, &invocations, None, &budget)
            .unwrap_err()
            .reason
            .code,
        FreshnessReasonCode::CollectionFailed,
    );
    fs::write(
        fixture.root().join("docs/guide.md"),
        "Changed outside narrow inputs\n",
    )
    .unwrap();
    let error =
        revalidate_whole_source(&ctx, &catalog, &invocations, Some(&token), &budget).unwrap_err();
    assert_eq!(error.reason.code, FreshnessReasonCode::SourceRaced);
    fixture.actions[2].inputs_policy = Some(ActionInputsPolicy::Exhaustive);
    fixture.write_authority();
    let ctx = fixture.context();
    let catalog = fixture.catalog(&ctx);
    revalidate_whole_source(&ctx, &catalog, &invocations, Some(&token), &budget).unwrap();
}
