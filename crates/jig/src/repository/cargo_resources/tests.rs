use super::*;
use jig_contract::{ActionIntent, RustNextestConfigV1};
use tempfile::{TempDir, tempdir};

fn fixture() -> (TempDir, RepoContext, PlannedTarget) {
    let temp = tempdir().unwrap();
    let root = temp.path();
    crate::test_env::TestRepoBuilder::new(root)
        .repo_name("ExampleResourceWorkspace")
        .write();
    fs::create_dir(root.join("src")).unwrap();
    fs::create_dir(root.join("cargo-home")).unwrap();
    fs::create_dir(root.join("artifacts")).unwrap();
    fs::write(root.join("Cargo.toml"), "[package]\nname = \"example-resource\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[features]\nextra = []\n").unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "compile_error!(\"metadata must not compile\");\n",
    )
    .unwrap();
    fs::write(
        root.join("Cargo.lock"),
        "version = 4\n\n[[package]]\nname = \"example-resource\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    let runner = ActionRunner::Shell {
        command: "example_check".into(),
        working_directory: None,
        environment: BTreeMap::from([
            (
                "CARGO_HOME".into(),
                root.join("cargo-home").to_str().unwrap().into(),
            ),
            (
                "CARGO_TARGET_DIR".into(),
                root.join("artifacts").to_str().unwrap().into(),
            ),
            (
                "CARGO_BUILD_BUILD_DIR".into(),
                root.join("artifacts").to_str().unwrap().into(),
            ),
        ]),
    };
    let mut planned = PlannedTarget::new(
        "example:test".parse().unwrap(),
        ActionIntent::Check,
        runner,
        "example",
    );
    planned.resources.push(ExecutionResourceV1::CargoV1 {
        workspace_manifest: "Cargo.toml".into(),
        working_directory: None,
        context: CargoImpactContextV1::default(),
    });
    let ctx = RepoContext::load_from(root).unwrap();
    (temp, ctx, planned)
}

fn resolve_fixture(ctx: &RepoContext, planned: &PlannedTarget) -> ResolvedCargoResources {
    resolve(ctx, planned, Duration::from_secs(15), &|| false).unwrap()
}

#[test]
fn metadata_claims_declared_environment_and_physical_directory_without_building() {
    let (temp, ctx, planned) = fixture();
    let lock_before = fs::read(temp.path().join("Cargo.lock")).unwrap();
    let resolved = resolve_fixture(&ctx, &planned);
    assert_eq!(resolved.partial_reason, None, "{resolved:?}");
    assert_eq!(resolved.claims.len(), 3, "{resolved:?}");
    assert_eq!(resolved.claims[0].mode, ResourceClaimMode::Shared);
    assert_eq!(resolved.claims[1].mode, ResourceClaimMode::Exclusive);
    assert!(resolved.claims.iter().any(|claim| claim.opaque_key
        == path_key(
            b"jig-cargo-artifact-v1",
            &temp.path().canonicalize().unwrap().join("artifacts")
        )));
    assert!(
        resolved.claims.iter().any(|claim| claim.opaque_key
            == physical_directory_key(&temp.path().join("artifacts")).unwrap())
    );
    assert_eq!(
        fs::read_dir(temp.path().join("artifacts")).unwrap().count(),
        0
    );
    assert_eq!(
        fs::read(temp.path().join("Cargo.lock")).unwrap(),
        lock_before
    );
    assert!(!format!("{resolved:?}").contains(temp.path().to_str().unwrap()));
    let again = resolve_fixture(&ctx, &planned);
    assert!(resolved.same_identity(&again));
}

#[test]
fn distinct_effective_build_directory_has_its_own_claim() {
    let (temp, ctx, mut planned) = fixture();
    fs::create_dir(temp.path().join("intermediates")).unwrap();
    let ActionRunner::Shell { environment, .. } = &mut planned.runner else {
        unreachable!()
    };
    environment.insert(
        "CARGO_BUILD_BUILD_DIR".into(),
        temp.path().join("intermediates").to_str().unwrap().into(),
    );
    let resolved = resolve_fixture(&ctx, &planned);
    assert_eq!(resolved.partial_reason, None, "{resolved:?}");
    assert_eq!(resolved.claims.len(), 5, "{resolved:?}");
    assert!(resolved.claims.iter().any(|claim| claim.opaque_key
        == path_key(
            b"jig-cargo-artifact-v1",
            &temp.path().canonicalize().unwrap().join("intermediates")
        )));
    assert_eq!(
        fs::read_dir(temp.path().join("intermediates"))
            .unwrap()
            .count(),
        0
    );
}

#[cfg(unix)]
#[test]
fn symlink_aliases_with_nonexistent_suffix_share_one_artifact_claim() {
    let (temp, ctx, mut planned) = fixture();
    fs::create_dir(temp.path().join("physical")).unwrap();
    std::os::unix::fs::symlink(temp.path().join("physical"), temp.path().join("alias")).unwrap();
    let ActionRunner::Shell { environment, .. } = &mut planned.runner else {
        unreachable!()
    };
    environment.insert(
        "CARGO_TARGET_DIR".into(),
        temp.path()
            .join("physical/future/output")
            .to_str()
            .unwrap()
            .into(),
    );
    environment.insert(
        "CARGO_BUILD_BUILD_DIR".into(),
        temp.path()
            .join("alias/future/output")
            .to_str()
            .unwrap()
            .into(),
    );
    let resolved = resolve_fixture(&ctx, &planned);
    assert_eq!(
        resolved.partial_reason,
        Some("artifact_directory_not_created"),
        "{resolved:?}"
    );
    assert_eq!(resolved.claims.len(), 2, "{resolved:?}");
    assert_eq!(resolved.claims[0].mode, ResourceClaimMode::Exclusive);
    assert!(!temp.path().join("physical/future").exists());
    fs::create_dir_all(temp.path().join("physical/future/output")).unwrap();
    let existing = resolve_fixture(&ctx, &planned);
    assert_eq!(existing.partial_reason, None);
    assert_eq!(
        existing.claims.len(),
        3,
        "both aliases must deduplicate physical and spelling claims"
    );
    assert!(!resolved.same_identity(&existing));
}

#[test]
fn absent_directory_is_partial_and_creation_keeps_bridge_but_changes_authority() {
    let (temp, ctx, planned) = fixture();
    fs::remove_dir(temp.path().join("artifacts")).unwrap();
    let partial = resolve_fixture(&ctx, &planned);
    assert_eq!(
        partial.partial_reason,
        Some("artifact_directory_not_created")
    );
    assert_eq!(partial.claims.len(), 2);
    assert_eq!(partial.claims[0].mode, ResourceClaimMode::Exclusive);
    assert!(!temp.path().join("artifacts").exists());
    fs::create_dir(temp.path().join("artifacts")).unwrap();
    let full = resolve_fixture(&ctx, &planned);
    assert_eq!(full.partial_reason, None);
    assert_eq!(full.claims.len(), 3);
    assert_eq!(full.claims[0].mode, ResourceClaimMode::Shared);
    assert_eq!(partial.claims[0].opaque_key, full.claims[0].opaque_key);
    assert!(
        full.claims.contains(&partial.claims[1]),
        "canonical spelling must bridge creation"
    );
    assert!(
        !partial.same_identity(&full),
        "post-wait admission must reject changed physical authority"
    );
}

#[test]
fn physical_directory_replacement_invalidates_post_wait_identity() {
    let (temp, ctx, planned) = fixture();
    let before = resolve_fixture(&ctx, &planned);
    fs::rename(
        temp.path().join("artifacts"),
        temp.path().join("previous-artifacts"),
    )
    .unwrap();
    fs::create_dir(temp.path().join("artifacts")).unwrap();
    let after = resolve_fixture(&ctx, &planned);
    assert_eq!(after.partial_reason, None);
    assert!(
        !before.same_identity(&after),
        "a replacement inode must not inherit admission"
    );
    assert_ne!(
        physical_directory_key(&temp.path().join("previous-artifacts")).unwrap(),
        physical_directory_key(&temp.path().join("artifacts")).unwrap()
    );
}

#[test]
fn one_unavailable_declaration_retains_other_proven_artifact_claims() {
    let (_temp, ctx, mut planned) = fixture();
    let full = resolve_fixture(&ctx, &planned);
    planned.resources.insert(
        0,
        ExecutionResourceV1::CargoV1 {
            workspace_manifest: "missing/Cargo.toml".into(),
            working_directory: None,
            context: CargoImpactContextV1::default(),
        },
    );
    let partial = resolve_fixture(&ctx, &planned);
    assert_eq!(
        partial.partial_reason,
        Some("workspace_manifest_unavailable")
    );
    assert_eq!(partial.claims[0].mode, ResourceClaimMode::Exclusive);
    assert_eq!(
        &partial.claims[1..],
        &full.claims[1..],
        "continue collecting known claims after a partial declaration"
    );
}

#[cfg(unix)]
#[test]
fn repository_guards_use_physical_identity_with_distinct_resource_domain() {
    let (temp, ctx, planned) = fixture();
    let resolved = resolve_fixture(&ctx, &planned);
    assert_eq!(
        resolved.claims[0].opaque_key,
        physical_directory_key_in_domain(temp.path(), b"jig-cargo-repository-physical-v1").unwrap()
    );
    assert_ne!(
        resolved.claims[0].opaque_key,
        physical_directory_key(temp.path()).unwrap()
    );
    let aliases = tempdir().unwrap();
    let alias = aliases.path().join("example-repository-alias");
    std::os::unix::fs::symlink(temp.path(), &alias).unwrap();
    let aliased_ctx = RepoContext::load_from(&alias).unwrap();
    let aliased = resolve_fixture(&aliased_ctx, &planned);
    assert_eq!(resolved.claims, aliased.claims);
    let (_other, other_ctx, other_planned) = fixture();
    let other = resolve_fixture(&other_ctx, &other_planned);
    assert_ne!(resolved.claims[0].opaque_key, other.claims[0].opaque_key);
}

#[test]
fn unavailable_authority_is_partial_and_conflicts_with_known_repo_guard() {
    let (temp, ctx, mut planned) = fixture();
    let known = resolve_fixture(&ctx, &planned);
    let ExecutionResourceV1::CargoV1 {
        workspace_manifest, ..
    } = &mut planned.resources[0];
    *workspace_manifest = "missing/Cargo.toml".into();
    let partial = resolve_fixture(&ctx, &planned);
    assert_eq!(
        partial.partial_reason,
        Some("workspace_manifest_unavailable")
    );
    assert_eq!(partial.claims.len(), 1);
    assert_eq!(partial.claims[0].mode, ResourceClaimMode::Exclusive);
    assert_eq!(partial.claims[0].opaque_key, known.claims[0].opaque_key);
    assert!(!known.same_identity(&partial));
    assert!(!format!("{partial:?}").contains(temp.path().to_str().unwrap()));
}

#[test]
fn cancellation_and_exhausted_budget_are_terminal_not_partial() {
    let (_temp, ctx, planned) = fixture();
    for (duration, cancelled, expected) in [
        (Duration::from_secs(15), true, CargoResourceStop::Cancelled),
        (Duration::ZERO, false, CargoResourceStop::TimedOut),
    ] {
        let error = resolve(&ctx, &planned, duration, &|| cancelled).unwrap_err();
        assert_eq!(error.downcast_ref::<CargoResourceStop>(), Some(&expected));
    }
}

#[test]
fn metadata_supervision_failure_is_terminal_not_partial() {
    // These failures share the supervisor's post-spawn classification. None
    // supplies authority that the probe tree and capture safely completed.
    for detail in ["await failed", "cleanup unconfirmed", "capture incomplete"] {
        let outcome = metadata_failure(SupervisedExecutionError::Failed {
            error: anyhow::anyhow!("Example private probe detail: {detail}"),
            process_started: true,
        });
        let error = outcome.expect_err("unsafe metadata supervision must stop admission");
        assert!(
            !error.to_string().contains(detail),
            "keep probe details private"
        );
    }
    assert_eq!(
        metadata_failure(SupervisedExecutionError::Failed {
            error: anyhow::anyhow!("Cargo executable unavailable"),
            process_started: false,
        })
        .unwrap(),
        "cargo_metadata_unavailable",
        "a command that never started may retain repository-local fallback"
    );
}

#[cfg(unix)]
#[test]
fn running_metadata_timeout_and_cancellation_are_terminal() {
    use std::os::unix::fs::PermissionsExt;
    let (temp, ctx, mut planned) = fixture();
    let bin = temp.path().join("bin");
    fs::create_dir(&bin).unwrap();
    let cargo = bin.join("cargo");
    fs::write(&cargo, "#!/bin/sh\nexec /bin/sleep 5\n").unwrap();
    fs::set_permissions(cargo, fs::Permissions::from_mode(0o755)).unwrap();
    let ActionRunner::Shell { environment, .. } = &mut planned.runner else {
        unreachable!()
    };
    environment.insert("PATH".into(), bin.to_str().unwrap().into());
    let error = resolve(&ctx, &planned, Duration::from_millis(100), &|| false).unwrap_err();
    assert_eq!(
        error.downcast_ref::<CargoResourceStop>(),
        Some(&CargoResourceStop::TimedOut)
    );
    let started = Instant::now();
    let error = resolve(&ctx, &planned, Duration::from_secs(5), &|| {
        started.elapsed() >= Duration::from_millis(100)
    })
    .unwrap_err();
    assert_eq!(
        error.downcast_ref::<CargoResourceStop>(),
        Some(&CargoResourceStop::Cancelled)
    );
}

#[test]
fn prepared_nextest_features_override_declared_context_without_splitting_claims() {
    let (temp, _, mut planned) = fixture();
    let configuration = RustNextestConfigV1 {
        workspace_manifest: "Cargo.toml".into(),
        focused: true,
        context: CargoImpactContextV1::default(),
        cargo_profile: None,
        nextest_profile: None,
    };
    let mut prepared = crate::repository::rust_focus::full_input(&configuration);
    prepared.context.features = vec!["extra".into()];
    prepared.context.no_default_features = true;
    planned.runner = ActionRunner::RustNextestV1 { configuration };
    planned.prepared_rust_input = Some(prepared);
    let command = metadata_command(
        temp.path(),
        &planned,
        "Cargo.toml",
        None,
        &CargoImpactContextV1::default(),
    )
    .unwrap();
    let args = command
        .get_args()
        .map(|arg| arg.to_str().unwrap())
        .collect::<Vec<_>>();
    assert!(args.windows(2).any(|pair| pair == ["--features", "extra"]));
    assert!(args.contains(&"--no-default-features"));
    assert!(args.contains(&"--no-deps"));
    assert!(args.contains(&"--locked"));
    assert!(args.contains(&"--offline"));
}

#[test]
fn unsupported_or_unresolvable_directory_authority_is_not_guessed() {
    assert_eq!(
        metadata_directories(br#"{"target_directory":"/example"}"#),
        Err("cargo_artifact_directories_unavailable")
    );
    assert!(canonical_future_directory(Path::new("relative/output")).is_none());
    let temp = tempdir().unwrap();
    fs::write(temp.path().join("file"), "Example non-directory").unwrap();
    assert!(canonical_future_directory(&temp.path().join("file/output")).is_none());
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(temp.path().join("absent"), temp.path().join("dangling"))
            .unwrap();
        assert!(canonical_future_directory(&temp.path().join("dangling/output")).is_none());
    }
}
