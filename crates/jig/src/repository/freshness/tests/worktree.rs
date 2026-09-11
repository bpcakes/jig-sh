use super::*;

fn worktree_fixture(policy: ActionInputsPolicy) -> Fixture {
    let mut fixture = Fixture::new();
    fixture.epoch = 8;
    for action in &mut fixture.actions {
        action.source_state = Some(ActionSourceState::Worktree);
        action.inputs_policy = Some(policy);
    }
    fixture.write_authority();
    git(fixture.root(), &["add", "."]);
    git(
        fixture.root(),
        &["commit", "-qm", "Declare working-file consumers"],
    );
    fixture
}

#[test]
fn worktree_identity_follows_bytes_paths_and_modes_across_git_placement() {
    for policy in [
        ActionInputsPolicy::Exhaustive,
        ActionInputsPolicy::WholeRepository,
    ] {
        let fixture = worktree_fixture(policy);
        let identity = || fixture.identity("web:test");
        let path = fixture.root().join("apps/web/src/page.ts");
        let original = identity();
        fs::write(&path, "export const page = 2;\n").unwrap();
        let checked = identity();
        assert_ne!(original.source_digest, checked.source_digest);
        git(fixture.root(), &["add", "apps/web/src/page.ts"]);
        assert_eq!(
            checked,
            identity(),
            "staging changed {policy:?} working files"
        );
        git(fixture.root(), &["commit", "-qm", "Commit checked content"]);
        assert_eq!(
            checked,
            identity(),
            "commit changed {policy:?} working files"
        );
        let added = fixture.root().join("apps/web/src/added.ts");
        fs::write(&added, "new input\n").unwrap();
        let addition = identity();
        assert_ne!(checked.source_digest, addition.source_digest);
        git(fixture.root(), &["add", "apps/web/src/added.ts"]);
        assert_eq!(addition, identity());
        git(fixture.root(), &["commit", "-qm", "Add checked input"]);
        assert_eq!(addition, identity());
        fs::remove_file(&added).unwrap();
        let deleted = identity();
        assert_ne!(addition.source_digest, deleted.source_digest);
        git(fixture.root(), &["add", "apps/web/src/added.ts"]);
        assert_eq!(deleted, identity());
        git(fixture.root(), &["commit", "-qm", "Remove checked input"]);
        assert_eq!(deleted, identity());
        let renamed = fixture.root().join("apps/web/src/renamed.ts");
        fs::rename(&path, &renamed).unwrap();
        let moved = identity();
        assert_ne!(deleted.source_digest, moved.source_digest);
        git(fixture.root(), &["add", "apps/web/src"]);
        git(fixture.root(), &["commit", "-qm", "Rename checked input"]);
        assert_eq!(moved, identity());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&renamed, fs::Permissions::from_mode(0o755)).unwrap();
            let executable = identity();
            assert_ne!(moved.source_digest, executable.source_digest);
            git(fixture.root(), &["add", "apps/web/src"]);
            assert_eq!(executable, identity());
        }
    }
}

#[test]
fn worktree_dependencies_and_helpers_still_invalidate_dependents() {
    let fixture = worktree_fixture(ActionInputsPolicy::Exhaustive);
    let before = fixture.identity("web:test");
    fs::write(
        fixture.root().join("shared/source/model.txt"),
        "changed leaf\n",
    )
    .unwrap();
    let dependency = fixture.identity("web:test");
    assert_eq!(before.source_digest, dependency.source_digest);
    assert_ne!(before.dependency_digest, dependency.dependency_digest);
    fs::write(
        fixture.root().join("scripts/check.sh"),
        "#!/bin/sh\nexit 1\n",
    )
    .unwrap();
    let helper = fixture.identity("web:test");
    assert_ne!(dependency.source_digest, helper.source_digest);
    assert_ne!(dependency.identity_digest, helper.identity_digest);
}

#[test]
fn whole_worktree_omits_ignored_outputs_but_explicit_ignored_inputs_remain_unknown() {
    let mut fixture = worktree_fixture(ActionInputsPolicy::WholeRepository);
    let before = fixture.identity("web:test");
    fs::create_dir(fixture.root().join("target")).unwrap();
    fs::write(fixture.root().join("target/output"), "generated\n").unwrap();
    assert_eq!(before, fixture.identity("web:test"));
    fixture.actions[0].inputs_policy = Some(ActionInputsPolicy::Exhaustive);
    fixture.actions[0].inputs.push("target/**".into());
    fixture.write_authority();
    assert_eq!(
        fixture.collect().targets[&"web:test".parse().unwrap()]
            .as_ref()
            .unwrap_err()
            .reason
            .code,
        FreshnessReasonCode::UnobservableInput
    );
}

#[cfg(unix)]
#[test]
fn whole_worktree_omits_ignored_symlinks_but_preserves_required_link_authority() {
    use std::os::unix::fs::symlink;

    let mut fixture = worktree_fixture(ActionInputsPolicy::WholeRepository);
    let outside = tempdir().unwrap();
    fs::write(outside.path().join("input"), "outside input\n").unwrap();
    let before = fixture.identity("web:test");
    symlink(outside.path(), fixture.root().join("generated.ignored")).unwrap();
    assert_eq!(before, fixture.identity("web:test"));
    fs::write(outside.path().join("input"), "changed outside input\n").unwrap();
    assert_eq!(before, fixture.identity("web:test"));

    // A declared descendant remains unobservable even though the collector
    // intentionally never follows the ignored ancestor.
    fixture.actions[0].inputs_policy = Some(ActionInputsPolicy::Exhaustive);
    fixture.actions[0].inputs.push("**/input".into());
    fixture.write_authority();
    assert_eq!(
        fixture.collect().targets[&"web:test".parse().unwrap()]
            .as_ref()
            .unwrap_err()
            .reason
            .code,
        FreshnessReasonCode::UnobservableInput
    );
    fixture.actions[0].inputs_policy = Some(ActionInputsPolicy::WholeRepository);
    fixture.actions[0].inputs.pop();
    fixture.write_authority();
    symlink(
        outside.path().join("input"),
        fixture.root().join(".env.local"),
    )
    .unwrap();
    assert_eq!(
        fixture.collect().targets[&"web:test".parse().unwrap()]
            .as_ref()
            .unwrap_err()
            .reason
            .code,
        FreshnessReasonCode::UnobservableInput
    );
    fs::remove_file(fixture.root().join(".env.local")).unwrap();
    if let ActionRunner::Argv { program, .. } = &mut fixture.actions[0].runner {
        *program = "generated.ignored/input".into();
    }
    fixture.write_authority();
    assert_eq!(
        fixture.collect().targets[&"web:test".parse().unwrap()]
            .as_ref()
            .unwrap_err()
            .reason
            .code,
        FreshnessReasonCode::UnobservableInput
    );
}

#[test]
fn git_consumers_retain_index_head_and_branch_authority() {
    for policy in [
        ActionInputsPolicy::Exhaustive,
        ActionInputsPolicy::WholeRepository,
    ] {
        let mut fixture = worktree_fixture(policy);
        for action in &mut fixture.actions {
            action.source_state = None;
        }
        fixture.write_authority();
        let identity = || fixture.identity("web:test");
        fs::write(
            fixture.root().join("apps/web/src/page.ts"),
            "checked edit\n",
        )
        .unwrap();
        let checked = identity();
        git(fixture.root(), &["add", "apps/web/src/page.ts"]);
        let staged = identity();
        assert_ne!(checked.source_digest, staged.source_digest);
        git(
            fixture.root(),
            &["commit", "-qm", "Commit Git-sensitive input"],
        );
        let committed = identity();
        assert_ne!(staged.source_digest, committed.source_digest);
        git(
            fixture.root(),
            &["commit", "--allow-empty", "-qm", "Metadata-only commit"],
        );
        let head = identity();
        assert_ne!(committed.source_digest, head.source_digest);
        git(fixture.root(), &["switch", "-qc", "example-branch"]);
        assert_ne!(head.source_digest, identity().source_digest);
    }
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

#[test]
fn whole_worktree_cannot_hide_an_ignored_runner_or_symlinked_cwd() {
    let mut fixture = worktree_fixture(ActionInputsPolicy::WholeRepository);
    fs::create_dir(fixture.root().join("target")).unwrap();
    fs::copy(
        fixture.root().join("scripts/check.sh"),
        fixture.root().join("target/check.sh"),
    )
    .unwrap();
    if let ActionRunner::Argv { program, .. } = &mut fixture.actions[0].runner {
        *program = "target/check.sh".into();
    }
    fixture.write_authority();
    assert_eq!(
        fixture.collect().targets[&"web:test".parse().unwrap()]
            .as_ref()
            .unwrap_err()
            .reason
            .code,
        FreshnessReasonCode::UnobservableInput
    );
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("apps/web", fixture.root().join("target/cwd")).unwrap();
        if let ActionRunner::Argv {
            program,
            working_directory,
            ..
        } = &mut fixture.actions[0].runner
        {
            *program = "true".into();
            *working_directory = Some("target/cwd".into());
        }
        fixture.write_authority();
        assert_eq!(
            fixture.collect().targets[&"web:test".parse().unwrap()]
                .as_ref()
                .unwrap_err()
                .reason
                .code,
            FreshnessReasonCode::UnobservableInput
        );
    }
}

#[test]
fn whole_worktree_preserves_explicit_tracker_exclusion_across_edits_staging_and_commit() {
    let fixture = worktree_fixture(ActionInputsPolicy::WholeRepository);
    fs::create_dir(fixture.root().join(".beads")).unwrap();
    fs::write(
        fixture.root().join(".beads/issues.jsonl"),
        "Example issue\n",
    )
    .unwrap();
    let default = fixture.identity("web:test");
    fs::write(
        fixture.root().join(".beads/issues.jsonl"),
        "Changed example issue\n",
    )
    .unwrap();
    assert_ne!(
        default.source_digest,
        fixture.identity("web:test").source_digest
    );
    let config_path = fixture.root().join(".jig.toml");
    let mut config = fs::read_to_string(&config_path).unwrap();
    config.push_str("\n[work]\nreceipt_metadata = [\"beads\"]\n");
    fs::write(config_path, config).unwrap();
    fs::write(
        fixture.root().join(".beads/issues.jsonl"),
        "Example issue\n",
    )
    .unwrap();
    git(fixture.root(), &["add", "."]);
    git(
        fixture.root(),
        &["commit", "-qm", "Declare example tracker ownership"],
    );
    let before = fixture.identity("web:test");
    fs::write(
        fixture.root().join(".beads/issues.jsonl"),
        "Updated example issue\n",
    )
    .unwrap();
    assert_eq!(before, fixture.identity("web:test"));
    git(fixture.root(), &["add", ".beads/issues.jsonl"]);
    assert_eq!(before, fixture.identity("web:test"));
    git(
        fixture.root(),
        &["commit", "-qm", "Record example tracker update"],
    );
    assert_eq!(before, fixture.identity("web:test"));
    fs::create_dir(fixture.root().join("apps/web/.beads")).unwrap();
    fs::write(
        fixture.root().join("apps/web/.beads/fixture.json"),
        "Source fixture\n",
    )
    .unwrap();
    assert_ne!(
        before.source_digest,
        fixture.identity("web:test").source_digest
    );
}
