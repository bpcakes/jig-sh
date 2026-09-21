use std::{
    fs,
    path::Path,
    process::{Command, Output},
    time::Duration,
};

use jig_contract::RustTargetV1;
use tempfile::{TempDir, tempdir};

use super::*;
use crate::{
    context::CommandOutputLimit,
    execution::{NoopExecutionObserver, run_supervised_execution_command},
    state::{PlanOpenRequest, plans_open},
    test_env::TestRepoBuilder,
};

pub(super) fn owned_command(root: &Path, command: &mut Command) -> Output {
    command
        .current_dir(root)
        .env("CARGO_NET_OFFLINE", "true")
        .env("CARGO_TARGET_DIR", root.join("target"))
        .env("NEXTEST_TEST_THREADS", "1")
        .env("CARGO_TERM_COLOR", "never")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null");
    for key in [
        "CARGO_BUILD_TARGET",
        "RUSTC_WRAPPER",
        "RUSTC_WORKSPACE_WRAPPER",
        "RUSTFLAGS",
        "CARGO_ENCODED_RUSTFLAGS",
        "NEXTEST_PROFILE",
        "NEXTEST_FILTER",
    ] {
        command.env_remove(key);
    }
    let output = run_supervised_execution_command(
        command,
        Duration::from_secs(90),
        CommandOutputLimit::from_bytes(4 * 1024 * 1024).unwrap(),
        "Rust focus acceptance fixture",
        &mut NoopExecutionObserver,
    )
    .unwrap_or_else(|error| panic!("bounded fixture command failed: {error:?}"));
    Output {
        status: output.status,
        stdout: output.stdout,
        stderr: output.stderr,
    }
}

pub(super) fn git(root: &Path, args: &[&str]) -> String {
    let output = owned_command(
        root,
        Command::new("git")
            .args([
                "-c",
                "user.name=Example Agent",
                "-c",
                "user.email=agent@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.hooksPath=/dev/null",
            ])
            .args(args),
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

pub(super) fn write(root: &Path, path: &str, content: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

pub(super) fn fixture() -> (TempDir, RepoContext) {
    let temp = tempdir().unwrap();
    let root = temp.path();
    TestRepoBuilder::new(root)
        .repo_name("ExampleWorkspace")
        .write();
    write(
        root,
        ".gitignore",
        "/target/\n/.agent/state/\n/.agent/plans/\n/.agent/.cache/\n",
    );
    write(
        root,
        "Cargo.toml",
        "[workspace]\nmembers = [\"selected\", \"other\"]\nresolver = \"2\"\n",
    );
    write(
        root,
        "selected/Cargo.toml",
        "[package]\nname = \"example-selected\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    write(
        root,
        "selected/src/lib.rs",
        "#[test]\nfn example_lib_passes() { assert_eq!(2 + 2, 4); }\n",
    );
    write(
        root,
        "selected/tests/sibling.rs",
        "compile_error!(\"example_sibling_must_not_build_for_library_focus\");\n",
    );
    write(
        root,
        "other/Cargo.toml",
        "[package]\nname = \"example-other\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    write(
        root,
        "other/src/lib.rs",
        "#[test]\nfn other_lib_passes() { assert_eq!(3 + 3, 6); }\n",
    );
    let lock = owned_command(
        root,
        Command::new("cargo").args(["generate-lockfile", "--offline"]),
    );
    assert!(
        lock.status.success(),
        "{}",
        String::from_utf8_lossy(&lock.stderr)
    );
    git(root, &["init", "--quiet"]);
    git(root, &["add", "."]);
    git(
        root,
        &["commit", "--quiet", "-m", "Example workspace baseline"],
    );
    let ctx = RepoContext::load_from(root).unwrap();
    (temp, ctx)
}

pub(super) fn config(focused: bool) -> RustNextestConfigV1 {
    RustNextestConfigV1 {
        workspace_manifest: "Cargo.toml".into(),
        focused,
        context: Default::default(),
        cargo_profile: None,
        nextest_profile: None,
    }
}

fn explicit(filter: Option<&str>) -> RustFocusV1 {
    RustFocusV1::Explicit {
        packages: vec!["example-selected@0.1.0".into()],
        targets: vec![RustTargetV1::Lib {}],
        features: None,
        filter: filter.map(str::to_owned),
    }
}

pub(super) fn prepare_focus(
    ctx: &RepoContext,
    config: &RustNextestConfigV1,
    focus: Option<RustFocusV1>,
) -> PreparedRustInputV1 {
    prepare(ctx, &"repo:test".parse().unwrap(), config, focus, &|| false).unwrap()
}

pub(super) fn run_prepared(root: &Path, prepared: &PreparedRustInputV1) -> Output {
    // Execute the exact argv emitted by preparation: the fixture does not
    // add selectors, remove workspace scope, or replace any generated flag.
    owned_command(root, Command::new("cargo").args(&prepared.args))
}

#[test]
fn real_library_focus_excludes_sibling_build_and_empty_filter_fails() {
    let (temp, ctx) = fixture();
    let prepared = prepare_focus(&ctx, &config(true), Some(explicit(None)));
    assert_eq!(prepared.disposition, RustScopeDispositionV1::Narrowed);
    assert_eq!(prepared.packages, ["example-selected@0.1.0"]);
    assert_eq!(prepared.targets, [RustTargetV1::Lib {}]);
    assert!(
        prepared
            .args
            .windows(2)
            .any(|args| args == ["--package", "example-selected@0.1.0"])
    );
    assert!(prepared.args.iter().any(|arg| arg == "--lib"));
    assert!(
        !prepared
            .args
            .iter()
            .any(|arg| arg == "--workspace" || arg == "--all-targets")
    );
    let narrow = run_prepared(temp.path(), &prepared);
    assert!(
        narrow.status.success(),
        "{}",
        String::from_utf8_lossy(&narrow.stderr)
    );
    assert!(String::from_utf8_lossy(&narrow.stderr).contains("example_lib_passes"));

    let empty = prepare_focus(
        &ctx,
        &config(true),
        Some(explicit(Some("test(=missing_example_case)"))),
    );
    let output = run_prepared(temp.path(), &empty);
    assert_eq!(
        output.status.code(),
        Some(4),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    // A normal full plan must still build the invalid sibling integration
    // binary. Its compile error is the independent oracle for build exclusion.
    let broad = prepare_focus(&ctx, &config(false), None);
    assert_eq!(broad.disposition, RustScopeDispositionV1::Full);
    assert!(broad.args.iter().any(|arg| arg == "--workspace"));
    let output = run_prepared(temp.path(), &broad);
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("example_sibling_must_not_build_for_library_focus")
    );
}

#[test]
fn automatic_focus_uses_recorded_baseline_across_commit_and_uncommitted_changes() {
    let (temp, ctx) = fixture();
    let root = temp.path();
    let baseline = git(root, &["rev-parse", "HEAD"]);
    let opened = plans_open(
        &ctx,
        PlanOpenRequest {
            title: "Example accumulated Rust changes".into(),
            body: Some("Validate both packages changed since the recorded baseline.".into()),
            body_file: None,
            base: None,
        },
    )
    .unwrap();
    let plan_id = opened["plan_id"].as_str().unwrap();
    write(
        root,
        "selected/src/lib.rs",
        "#[test]\nfn example_lib_passes() { assert_eq!(4 + 4, 8); }\n",
    );
    git(root, &["add", "selected/src/lib.rs"]);
    git(
        root,
        &["commit", "--quiet", "-m", "Example earlier task change"],
    );
    assert_ne!(git(root, &["rev-parse", "HEAD"]), baseline);
    write(
        root,
        "other/src/lib.rs",
        "#[test]\nfn other_lib_passes() { assert_eq!(6 + 6, 12); }\n",
    );
    let prepared = prepare_focus(
        &ctx,
        &config(true),
        Some(RustFocusV1::Automatic {
            plan_id: Some(plan_id.into()),
        }),
    );
    assert_eq!(prepared.comparison_base.as_deref(), Some(baseline.as_str()));
    assert_eq!(
        prepared.disposition,
        RustScopeDispositionV1::Narrowed,
        "{:?}",
        prepared.reasons
    );
    assert_eq!(
        prepared.packages,
        ["example-other@0.1.0", "example-selected@0.1.0"]
    );
    assert!(!prepared.args.iter().any(|arg| arg == "--filter-expr"));
}

#[test]
fn explicit_unknown_package_and_focus_on_full_action_are_rejected() {
    let (_temp, ctx) = fixture();
    let target = "repo:test".parse().unwrap();
    let error = prepare(&ctx, &target, &config(false), Some(explicit(None)), &|| {
        false
    })
    .unwrap_err();
    assert!(error.to_string().contains("cannot be narrowed"));
    let missing = RustFocusV1::Explicit {
        packages: vec!["example-missing@0.1.0".into()],
        targets: vec![],
        features: None,
        filter: None,
    };
    let error = prepare(&ctx, &target, &config(true), Some(missing), &|| false).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("exact unambiguous workspace member")
    );
}

fn planner_fixture() -> (TempDir, RepoContext, crate::repository::RepositoryCatalog) {
    let (temp, _) = fixture();
    let repository = serde_json::json!({
        "components": [{"id":"repo", "root":".", "adapters":["rust"]}],
        "actions": [{
            "target":{"component":"repo", "action":"test"},
            "intent":"check", "effects":["read_only", "process"],
            "runner":{"kind":"rust_nextest_v1", "configuration":{
                "workspace_manifest":"Cargo.toml", "focused":true
            }},
            "arguments":{"focus":{"type":"rust_focus_v1"}}, "inputs":["**"]
        }],
        "profiles":[{"id":"verify", "targets":[{"component":"repo", "action":"test"}]}],
        "default_check_profile":"verify"
    });
    let config = serde_json::json!({"repository": repository});
    let config = toml::Value::try_from(&config).unwrap();
    TestRepoBuilder::new(temp.path())
        .contract_version(8)
        .repo_name("ExampleWorkspace")
        .config(toml::to_string(&config).unwrap())
        .write_config();
    let mut manifest = repository;
    manifest["contract_version"] = serde_json::json!(8);
    manifest["tool_namespace"] = serde_json::json!("jig");
    manifest["required_commands"] = serde_json::json!([]);
    manifest["tools"] = serde_json::json!([]);
    write(
        temp.path(),
        ".agent/jig-contract.json",
        &serde_json::to_string(&manifest).unwrap(),
    );
    git(
        temp.path(),
        &["add", ".jig.toml", ".agent/jig-contract.json"],
    );
    git(
        temp.path(),
        &[
            "commit",
            "--quiet",
            "-m",
            "Example typed execution authority",
        ],
    );
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let catalog = crate::repository::RepositoryCatalog::from_context(&ctx).unwrap();
    (temp, ctx, catalog)
}

#[test]
fn planner_rederives_prepared_rust_scope_and_rejects_tampering_before_execution() {
    use crate::repository::{
        PlanRunRequest, plan_focused_check_run_with_cancellation, validate_run_plan,
    };
    let (temp, ctx, catalog) = planner_fixture();
    let arguments = BTreeMap::from([(
        "repo:test".parse().unwrap(),
        BTreeMap::from([(
            "focus".into(),
            serde_json::to_string(&explicit(None)).unwrap(),
        )]),
    )]);
    let plan = plan_focused_check_run_with_cancellation(
        &ctx,
        &catalog,
        PlanRunRequest::default(),
        arguments,
        &|| false,
    )
    .unwrap();
    assert_eq!(plan.targets.len(), 1);
    assert!(validate_run_plan(&ctx, &catalog, &plan).is_ok());
    let original = plan.targets[0].prepared_rust_input.as_ref().unwrap();
    assert_eq!(original.disposition, RustScopeDispositionV1::Narrowed);
    assert_eq!(original.packages, ["example-selected@0.1.0"]);
    for variant in 0..4 {
        let mut tampered = plan.clone();
        let prepared = tampered.targets[0].prepared_rust_input.as_mut().unwrap();
        match variant {
            0 => prepared.args.push("--workspace".into()),
            1 => prepared.context.no_default_features = true,
            2 => prepared.packages = vec!["example-other@0.1.0".into()],
            3 => prepared.schema_version = 99,
            _ => unreachable!(),
        }
        let error = validate_run_plan(&ctx, &catalog, &tampered).unwrap_err();
        assert!(error.to_string().contains("modified"), "{error:#}");
    }
    assert!(
        !temp.path().join("target").exists(),
        "planning/replay must not build or execute tests"
    );
    write(
        temp.path(),
        "selected/src/lib.rs",
        "#[test]\nfn changed_after_plan() {}\n",
    );
    let error = validate_run_plan(&ctx, &catalog, &plan).unwrap_err();
    assert!(error.to_string().contains("stale"), "{error:#}");
    assert!(!temp.path().join("target").exists());
}

#[test]
fn automatic_missing_recorded_baseline_uses_explicit_broad_fallback() {
    let (_temp, ctx) = fixture();
    crate::state::seed_open_plan_for_test(
        &ctx,
        "plan_example_missing",
        "Example missing baseline",
        "Legacy plan has no baseline authority.",
    )
    .unwrap();
    for plan_id in [None, Some("plan_example_missing".into())] {
        let prepared = prepare_focus(
            &ctx,
            &config(true),
            Some(RustFocusV1::Automatic { plan_id }),
        );
        assert_eq!(prepared.disposition, RustScopeDispositionV1::BroadFallback);
        assert_eq!(prepared.reasons, ["comparison_unavailable"]);
        assert!(prepared.packages.is_empty());
        assert!(prepared.comparison_base.is_none());
        assert!(prepared.args.iter().any(|arg| arg == "--workspace"));
    }
}

#[test]
fn unavailable_metadata_broadens_automatic_scope_but_rejects_explicit_scope() {
    let (_temp, ctx) = fixture();
    let mut missing = config(true);
    missing.workspace_manifest = "missing/Cargo.toml".into();
    let automatic = prepare_focus(
        &ctx,
        &missing,
        Some(RustFocusV1::Automatic { plan_id: None }),
    );
    assert_eq!(automatic.disposition, RustScopeDispositionV1::BroadFallback);
    assert!(
        automatic
            .reasons
            .iter()
            .any(|reason| reason.contains("WorkspaceManifestMissing"))
    );
    assert!(automatic.args.iter().any(|arg| arg == "--workspace"));
    let error = prepare(
        &ctx,
        &"repo:test".parse().unwrap(),
        &missing,
        Some(explicit(None)),
        &|| false,
    )
    .unwrap_err();
    assert!(error.to_string().contains("package authority unavailable"));
    assert!(error.to_string().contains("configured full check"));

    // An extant, invalid manifest exercises Cargo's real nonzero acquisition
    // path, independently from the missing-file preflight path above.
    write(ctx.root(), "selected/Cargo.toml", "not valid TOML {{{\n");
    let automatic = prepare_focus(
        &ctx,
        &config(true),
        Some(RustFocusV1::Automatic { plan_id: None }),
    );
    assert_eq!(automatic.disposition, RustScopeDispositionV1::BroadFallback);
    assert!(
        automatic
            .reasons
            .iter()
            .any(|reason| reason.contains("MetadataCommandNonZero"))
    );
    let error = prepare(
        &ctx,
        &"repo:test".parse().unwrap(),
        &config(true),
        Some(explicit(None)),
        &|| false,
    )
    .unwrap_err();
    assert!(error.to_string().contains("package authority unavailable"));
}

#[test]
fn automatic_focus_accounts_for_root_library_relocated_inside_another_member() {
    let (temp, ctx) = fixture();
    let root = temp.path();
    write(
        root,
        "Cargo.toml",
        "[package]\nname = \"example-root\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\
         [lib]\npath = \"selected/shared.rs\"\n\
         [workspace]\nmembers = [\"selected\", \"other\"]\nresolver = \"2\"\n",
    );
    let root_source = "mod shared_module;\n#[test]\nfn example_root_shared_rule() { assert_eq!(shared_module::VALUE, 1); }\n";
    let module_source = "pub const VALUE: u32 = 1;\n";
    write(root, "selected/shared.rs", root_source);
    write(root, "selected/shared_module.rs", module_source);
    // This fixture's broad check must build successfully, so failures below
    // identify the relocated root package rather than the ordinary fixture's
    // deliberately invalid integration target.
    write(
        root,
        "selected/tests/sibling.rs",
        "#[test]\nfn example_integration_passes() {}\n",
    );
    let lock = owned_command(
        root,
        Command::new("cargo").args(["generate-lockfile", "--offline"]),
    );
    assert!(
        lock.status.success(),
        "{}",
        String::from_utf8_lossy(&lock.stderr)
    );
    git(root, &["add", "."]);
    git(
        root,
        &[
            "commit",
            "--quiet",
            "-m",
            "Example relocated root library baseline",
        ],
    );
    let opened = plans_open(
        &ctx,
        PlanOpenRequest {
            title: "Example relocated root library ownership".into(),
            body: Some("Retain root package coverage for its source and sibling module.".into()),
            body_file: None,
            base: None,
        },
    )
    .unwrap();
    let automatic = RustFocusV1::Automatic {
        plan_id: Some(opened["plan_id"].as_str().unwrap().into()),
    };

    for (changed_path, changed_source) in [
        (
            "selected/shared.rs",
            "mod shared_module;\n#[test]\nfn example_root_shared_rule() { assert_eq!(shared_module::VALUE, 2); }\n",
        ),
        ("selected/shared_module.rs", "pub const VALUE: u32 = 2;\n"),
    ] {
        write(root, "selected/shared.rs", root_source);
        write(root, "selected/shared_module.rs", module_source);
        write(root, changed_path, changed_source);
        let prepared = prepare_focus(&ctx, &config(true), Some(automatic.clone()));
        assert!(
            prepared.disposition == RustScopeDispositionV1::BroadFallback
                || prepared
                    .packages
                    .iter()
                    .any(|package| package == "example-root@0.1.0"),
            "{changed_path} omitted its root-library owner: {prepared:?}",
        );
        // Execute exactly the selected argv. The known failing root test is
        // the independent oracle that the owning package was not omitted.
        let output = run_prepared(root, &prepared);
        let report = format!(
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            !output.status.success(),
            "root regression was not tested: {report}"
        );
        assert!(report.contains("example_root_shared_rule"), "{report}");
        assert!(
            report.contains("assertion `left == right` failed"),
            "{report}"
        );
    }

    // The exceptional ownership layout must not globally disable ordinary
    // narrowing for an unrelated member with conventional source placement.
    write(root, "selected/shared.rs", root_source);
    write(root, "selected/shared_module.rs", module_source);
    write(
        root,
        "other/src/lib.rs",
        "#[test]\nfn other_lib_passes() { assert_eq!(4 + 4, 8); }\n",
    );
    let prepared = prepare_focus(&ctx, &config(true), Some(automatic));
    assert_eq!(
        prepared.disposition,
        RustScopeDispositionV1::Narrowed,
        "{prepared:?}"
    );
    assert_eq!(prepared.packages, ["example-other@0.1.0"]);
    let output = run_prepared(root, &prepared);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn explicit_test_false_target(target: RustTargetV1, cargo_target_args: &[&str], test_name: &str) {
    let (temp, ctx) = fixture();
    let root = temp.path();
    write(
        root,
        "selected/Cargo.toml",
        "[package]\nname = \"example-selected\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\
         [lib]\ntest = false\n\
         [[example]]\nname = \"demo\"\npath = \"examples/demo.rs\"\ntest = false\n",
    );
    write(
        root,
        "selected/examples/demo.rs",
        "fn main() {}\n#[test]\nfn example_demo_passes() { assert_eq!(3 + 4, 7); }\n",
    );
    // Establish Cargo's independent behavior before asking Jig to prepare
    // anything: test=false is a default-selection policy, not a prohibition
    // on selecting this real target explicitly.
    let mut direct = Command::new("cargo");
    direct
        .args([
            "nextest",
            "run",
            "--manifest-path",
            "Cargo.toml",
            "--package",
            "example-selected@0.1.0",
            "--locked",
            "--offline",
            "--no-tests=fail",
        ])
        .args(cargo_target_args);
    let output = owned_command(root, &mut direct);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains(test_name));

    let focus = RustFocusV1::Explicit {
        packages: vec!["example-selected@0.1.0".into()],
        targets: vec![target.clone()],
        features: None,
        filter: None,
    };
    let prepared = prepare_focus(&ctx, &config(true), Some(focus.clone()));
    assert_eq!(prepared.disposition, RustScopeDispositionV1::Narrowed);
    assert_eq!(prepared.packages, ["example-selected@0.1.0"]);
    assert_eq!(prepared.targets, [target]);
    assert!(
        !prepared
            .args
            .iter()
            .any(|arg| arg == "--workspace" || arg == "--all-targets")
    );
    let output = run_prepared(root, &prepared);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains(test_name));

    let RustFocusV1::Explicit {
        packages,
        targets,
        features,
        ..
    } = focus
    else {
        unreachable!()
    };
    let empty = prepare_focus(
        &ctx,
        &config(true),
        Some(RustFocusV1::Explicit {
            packages,
            targets,
            features,
            filter: Some("test(=missing_example_case)".into()),
        }),
    );
    let output = run_prepared(root, &empty);
    assert_eq!(
        output.status.code(),
        Some(4),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let missing = RustFocusV1::Explicit {
        packages: vec!["example-selected@0.1.0".into()],
        targets: vec![RustTargetV1::Example {
            name: "missing_example_target".into(),
        }],
        features: None,
        filter: None,
    };
    let error = prepare(
        &ctx,
        &"repo:test".parse().unwrap(),
        &config(true),
        Some(missing),
        &|| false,
    )
    .unwrap_err();
    assert!(error.to_string().contains("target"), "{error:#}");
}

#[test]
fn explicit_example_with_test_false_runs_its_real_tests() {
    explicit_test_false_target(
        RustTargetV1::Example {
            name: "demo".into(),
        },
        &["--example", "demo"],
        "example_demo_passes",
    );
}

#[test]
fn explicit_library_with_test_false_runs_its_real_tests() {
    explicit_test_false_target(RustTargetV1::Lib {}, &["--lib"], "example_lib_passes");
}
