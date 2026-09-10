use super::*;

fn web_failure(fixture: &Fixture) -> FreshnessReasonCode {
    fixture
        .collect()
        .targets
        .remove(&"web:test".parse().unwrap())
        .unwrap()
        .unwrap_err()
        .reason
        .code
}

#[test]
fn intent_to_add_and_skip_worktree_are_unobservable_only_when_relevant() {
    let fixture = Fixture::new();
    fs::write(fixture.root().join("docs/pending.md"), "pending").unwrap();
    git(
        fixture.root(),
        &["add", "--intent-to-add", "docs/pending.md"],
    );
    assert!(fixture.collect().targets.values().all(Result::is_ok));
    fs::write(fixture.root().join("apps/web/src/pending.ts"), "pending").unwrap();
    git(
        fixture.root(),
        &["add", "--intent-to-add", "apps/web/src/pending.ts"],
    );
    assert_eq!(
        web_failure(&fixture),
        FreshnessReasonCode::UnobservableInput
    );
    git(
        fixture.root(),
        &["reset", "-q", "--", "apps/web/src/pending.ts"],
    );
    git(
        fixture.root(),
        &["update-index", "--skip-worktree", "apps/web/src/page.ts"],
    );
    assert_eq!(
        web_failure(&fixture),
        FreshnessReasonCode::UnobservableInput
    );
}

#[test]
fn submodule_object_ids_do_not_stand_in_for_missing_or_present_contents() {
    let fixture = Fixture::new();
    let oid = Command::new("git")
        .current_dir(fixture.root())
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    let oid = String::from_utf8(oid.stdout).unwrap();
    git(
        fixture.root(),
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{},apps/web/module", oid.trim()),
        ],
    );
    assert_eq!(
        web_failure(&fixture),
        FreshnessReasonCode::UnobservableInput
    );
    fs::create_dir(fixture.root().join("apps/web/module")).unwrap();
    fs::write(
        fixture.root().join("apps/web/module/source.ts"),
        "module source",
    )
    .unwrap();
    assert_eq!(
        web_failure(&fixture),
        FreshnessReasonCode::UnobservableInput
    );
}

#[test]
fn directory_replacement_and_quoted_unicode_paths_are_observed() {
    let fixture = Fixture::new();
    let before = fixture.identity("web:test").source_digest;
    fs::write(
        fixture.root().join("apps/web/src/Example 'café'.ts"),
        "example",
    )
    .unwrap();
    let added = fixture.identity("web:test").source_digest;
    assert_ne!(before, added);
    git(fixture.root(), &["add", "apps/web/src/Example 'café'.ts"]);
    assert_ne!(added, fixture.identity("web:test").source_digest);
    fs::remove_file(fixture.root().join("apps/web/src/page.ts")).unwrap();
    fs::create_dir(fixture.root().join("apps/web/src/page.ts")).unwrap();
    fs::write(
        fixture.root().join("apps/web/src/page.ts/nested"),
        "replacement",
    )
    .unwrap();
    assert_ne!(before, fixture.identity("web:test").source_digest);
}

#[test]
fn ignored_parent_never_acquires_the_dotenv_exception() {
    let mut fixture = Fixture::new();
    fixture.actions[0]
        .inputs
        .push("node_modules/.env.example".into());
    fixture.write_authority();
    fs::create_dir(fixture.root().join("node_modules")).unwrap();
    fs::write(
        fixture.root().join("node_modules/.env.example"),
        "EXAMPLE=secret",
    )
    .unwrap();
    assert_eq!(
        web_failure(&fixture),
        FreshnessReasonCode::UnobservableInput
    );
}

#[test]
fn global_configuration_including_unknown_manifest_fields_is_authority() {
    let fixture = Fixture::new();
    let before = fixture.identity("web:test");
    let path = fixture.root().join(".agent/jig-contract.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    manifest["future_authority"] = json!({"example": "changed"});
    fs::write(path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    let after = fixture.identity("web:test");
    assert_ne!(before.configuration_digest, after.configuration_digest);
    assert_ne!(before.identity_digest, after.identity_digest);
    assert_eq!(before.source_digest, after.source_digest);
}

#[test]
fn graph_edges_depth_and_zero_deadline_fail_closed() {
    let fixture = Fixture::new();
    for limits in [
        CollectionLimits {
            edges: 0,
            ..CollectionLimits::with_timeout(Duration::from_secs(30))
        },
        CollectionLimits {
            directory_depth: 1,
            ..CollectionLimits::with_timeout(Duration::from_secs(30))
        },
        CollectionLimits::with_timeout(Duration::ZERO),
    ] {
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
}

#[test]
fn source_and_configuration_races_discard_the_entire_snapshot() {
    let fixture = Fixture::new();
    for path in [
        "apps/web/src/page.ts",
        "apps/web/src/new.ts",
        ".agent/jig-contract.json",
    ] {
        let ctx = fixture.context();
        let mut budget = CollectionBudget::new(
            CollectionLimits::with_timeout(Duration::from_secs(30)),
            &|| false,
        );
        let snapshot = source::SourceSnapshot::capture(
            &ctx,
            &fixture.actions.iter().collect::<Vec<_>>(),
            &mut budget,
        )
        .unwrap();
        let previous = fs::read(fixture.root().join(path)).ok();
        fs::write(fixture.root().join(path), "changed after observation").unwrap();
        assert_eq!(
            snapshot
                .revalidate(&ctx, &mut budget)
                .unwrap_err()
                .reason
                .code,
            FreshnessReasonCode::SourceRaced
        );
        match previous {
            Some(bytes) => fs::write(fixture.root().join(path), bytes).unwrap(),
            None => fs::remove_file(fixture.root().join(path)).unwrap(),
        }
    }
    let ctx = fixture.context();
    let catalog = fixture.catalog(&ctx);
    fs::write(
        fixture.root().join(".agent/jig-contract.json"),
        "changed before observation",
    )
    .unwrap();
    let mut budget = CollectionBudget::new(
        CollectionLimits::with_timeout(Duration::from_secs(30)),
        &|| false,
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
        FreshnessReasonCode::SourceRaced
    );
}
