use super::{tests::*, *};
use crate::execution::{ExecutionCancellation, ExecutionObserver, NoopExecutionObserver};

#[test]
fn requested_metadata_context_is_preserved_except_for_readonly_lock_authority() {
    let temp = tempfile::tempdir().unwrap();
    let mut runner = runner_with_stdout(fixture_metadata(temp.path()));
    let context = CargoImpactContextV1 {
        target: Some("aarch64-unknown-linux-gnu".into()),
        features: vec!["ExampleWorkspace/feature-a".into(), "feature-b".into()],
        all_features: true,
        no_default_features: true,
        locked: false,
        offline: false,
        ..Default::default()
    };
    let acquired = acquire_cargo_metadata_in_context_with_runner(
        temp.path(),
        &temp.path().join("Cargo.toml"),
        &context,
        &mut NoopExecutionObserver,
        &mut runner,
    )
    .unwrap();
    let mut expected = context.clone();
    expected.locked = true;
    assert_eq!(acquired.context, expected);
    assert!(!context.locked);
    let command = runner.command.unwrap();
    assert_eq!(
        command.args,
        vec![
            OsString::from("metadata"),
            "--format-version".into(),
            "1".into(),
            "--locked".into(),
            "--manifest-path".into(),
            temp.path().join("Cargo.toml").into_os_string(),
            "--filter-platform".into(),
            "aarch64-unknown-linux-gnu".into(),
            "--features".into(),
            "ExampleWorkspace/feature-a".into(),
            "--features".into(),
            "feature-b".into(),
            "--all-features".into(),
            "--no-default-features".into(),
        ]
    );
    assert_eq!(
        command.env,
        vec![("CARGO_NET_OFFLINE".into(), "false".into())]
    );
}

#[test]
fn invalid_or_unbounded_context_never_starts_metadata() {
    let cases = [
        CargoImpactContextV1 {
            metadata_format_version: 2,
            ..Default::default()
        },
        CargoImpactContextV1 {
            features: vec!["".into()],
            ..Default::default()
        },
        CargoImpactContextV1 {
            features: vec!["a,b".into()],
            ..Default::default()
        },
        CargoImpactContextV1 {
            features: vec!["--all-features".into()],
            ..Default::default()
        },
        CargoImpactContextV1 {
            features: vec!["feature".into(); 257],
            ..Default::default()
        },
        CargoImpactContextV1 {
            features: vec!["x".repeat(4096); 17],
            ..Default::default()
        },
        CargoImpactContextV1 {
            target: Some("target\nextra".into()),
            ..Default::default()
        },
        CargoImpactContextV1 {
            target: Some("x".repeat(4097)),
            ..Default::default()
        },
    ];
    for context in cases {
        let mut runner = RecordingRunner::default();
        let error = acquire_cargo_metadata_in_context_with_runner(
            Path::new("/ExampleWorkspace"),
            Path::new("/ExampleWorkspace/Cargo.toml"),
            &context,
            &mut NoopExecutionObserver,
            &mut runner,
        )
        .unwrap_err();
        assert_eq!(error, CargoMetadataAcquisitionError::UnsupportedContext);
        assert_eq!(
            error.public_reason(),
            Some(CargoImpactReasonV1::UnsupportedContext)
        );
        assert!(runner.command.is_none());
    }
}

struct Cancelled;
impl ExecutionObserver for Cancelled {}
impl ExecutionCancellation for Cancelled {
    fn cancelled(&self) -> bool {
        true
    }
}

#[test]
fn cancellation_before_context_acquisition_does_not_spawn() {
    let mut runner = RecordingRunner::default();
    let error = acquire_cargo_metadata_in_context_with_runner(
        Path::new("/ExampleWorkspace"),
        Path::new("/ExampleWorkspace/Cargo.toml"),
        &Default::default(),
        &mut Cancelled,
        &mut runner,
    )
    .unwrap_err();
    assert_eq!(error, CargoMetadataAcquisitionError::CancelledBeforeStart);
    assert!(runner.command.is_none());
}

#[test]
fn cancellation_after_metadata_output_never_returns_graph() {
    struct CancelAfterSpawn(std::cell::Cell<usize>);
    impl ExecutionObserver for CancelAfterSpawn {}
    impl ExecutionCancellation for CancelAfterSpawn {
        fn cancelled(&self) -> bool {
            let calls = self.0.get();
            self.0.set(calls + 1);
            calls > 0
        }
    }
    let temp = tempfile::tempdir().unwrap();
    let mut runner = runner_with_stdout(fixture_metadata(temp.path()));
    let error = acquire_cargo_metadata_in_context_with_runner(
        temp.path(),
        &temp.path().join("Cargo.toml"),
        &Default::default(),
        &mut CancelAfterSpawn(std::cell::Cell::new(0)),
        &mut runner,
    )
    .unwrap_err();
    assert_eq!(error, CargoMetadataAcquisitionError::Cancelled);
    assert!(runner.command.is_some());
}

#[test]
fn real_metadata_resolves_requested_feature_context() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("Cargo.toml"),
        r#"[package]
name = "example-context"
version = "0.1.0"
edition = "2021"
[features]
default = ["default-feature"]
default-feature = []
requested-feature = []
"#,
    )
    .unwrap();
    std::fs::create_dir(temp.path().join("src")).unwrap();
    std::fs::write(temp.path().join("src/lib.rs"), "pub fn example() {}\n").unwrap();
    let output = std::process::Command::new("cargo")
        .args(["generate-lockfile", "--offline"])
        .current_dir(temp.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let context = CargoImpactContextV1 {
        features: vec!["requested-feature".into()],
        no_default_features: true,
        target: Some("aarch64-unknown-linux-gnu".into()),
        ..Default::default()
    };
    let acquired = acquire_cargo_metadata_in_context(
        temp.path(),
        Path::new("Cargo.toml"),
        &context,
        &mut NoopExecutionObserver,
    )
    .unwrap();
    assert_eq!(acquired.context, context);
    assert!(
        acquired
            .graph
            .verifies_workspace_selector("example-context@0.1.0")
    );
    assert_eq!(
        acquired.graph.packages()[0].activated_features,
        ["requested-feature"]
    );
    assert!(
        !temp.path().join("target").exists(),
        "metadata must not build targets"
    );
}

fn unlocked_discovery_fixture() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    fs::write(
        temp.path().join("Cargo.toml"),
        "[package]\nname = \"example-readonly\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::create_dir(temp.path().join("src")).unwrap();
    fs::write(temp.path().join("src/lib.rs"), "pub fn example() {}\n").unwrap();
    temp
}

fn unlocked_discovery(
    root: &Path,
) -> Result<CargoMetadataAcquisition, CargoMetadataAcquisitionError> {
    acquire_cargo_metadata_in_context(
        root,
        Path::new("Cargo.toml"),
        &CargoImpactContextV1 {
            locked: false,
            ..Default::default()
        },
        &mut NoopExecutionObserver,
    )
}

#[test]
fn real_unlocked_discovery_does_not_create_missing_lockfile() {
    let temp = unlocked_discovery_fixture();
    let before_manifest = fs::read(temp.path().join("Cargo.toml")).unwrap();
    let before_source = fs::read(temp.path().join("src/lib.rs")).unwrap();
    let result = unlocked_discovery(temp.path());
    assert!(
        !temp.path().join("Cargo.lock").exists(),
        "read-only discovery created Cargo.lock"
    );
    assert_eq!(
        result.unwrap_err(),
        CargoMetadataAcquisitionError::NonZeroExit
    );
    assert_eq!(
        fs::read(temp.path().join("Cargo.toml")).unwrap(),
        before_manifest
    );
    assert_eq!(
        fs::read(temp.path().join("src/lib.rs")).unwrap(),
        before_source
    );
    assert!(!temp.path().join("target").exists());
}

#[test]
fn real_unlocked_discovery_does_not_rewrite_outdated_lockfile() {
    let temp = unlocked_discovery_fixture();
    let outdated = b"version = 4\n[[package]]\nname = \"example-readonly\"\nversion = \"0.0.1\"\n";
    fs::write(temp.path().join("Cargo.lock"), outdated).unwrap();
    let before_manifest = fs::read(temp.path().join("Cargo.toml")).unwrap();
    let result = unlocked_discovery(temp.path());
    assert_eq!(
        fs::read(temp.path().join("Cargo.lock")).unwrap(),
        outdated,
        "read-only discovery rewrote Cargo.lock"
    );
    assert_eq!(
        result.unwrap_err(),
        CargoMetadataAcquisitionError::NonZeroExit
    );
    assert_eq!(
        fs::read(temp.path().join("Cargo.toml")).unwrap(),
        before_manifest
    );
    assert!(!temp.path().join("target").exists());
}

#[test]
fn real_unlocked_discovery_accepts_valid_lock_without_changing_execution_policy() {
    let temp = unlocked_discovery_fixture();
    let lock = b"version = 4\n[[package]]\nname = \"example-readonly\"\nversion = \"0.1.0\"\n";
    fs::write(temp.path().join("Cargo.lock"), lock).unwrap();
    let requested = CargoImpactContextV1 {
        locked: false,
        ..Default::default()
    };
    let acquired = acquire_cargo_metadata_in_context(
        temp.path(),
        Path::new("Cargo.toml"),
        &requested,
        &mut NoopExecutionObserver,
    )
    .unwrap();
    assert_eq!(fs::read(temp.path().join("Cargo.lock")).unwrap(), lock);
    assert!(
        acquired.context.locked,
        "acquisition must report its actual read-only lock authority"
    );
    assert!(
        !requested.locked,
        "discovery must not modify requested execution policy"
    );
    assert!(
        acquired
            .graph
            .verifies_workspace_selector("example-readonly@0.1.0")
    );
    assert!(!temp.path().join("target").exists());
}
