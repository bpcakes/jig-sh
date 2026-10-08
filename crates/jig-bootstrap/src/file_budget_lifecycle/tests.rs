use super::*;
use tempfile::{TempDir, tempdir};

fn write_executable(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
}

fn staged_checker(bytes: &[u8]) -> StagedRender {
    let root = tempdir().unwrap();
    let destination = root.path().join("render");
    write_executable(&destination.join(LEGACY_CHECKER_PATH), bytes);
    let active_paths = BTreeSet::from([
        PathBuf::from(LEGACY_CHECKER_PATH),
        PathBuf::from(managed_paths::MANIFEST_PATH),
    ]);
    managed_paths::write_manifest(&destination, &active_paths).unwrap();
    StagedRender {
        _root: root,
        destination,
        active_paths,
        retirement_paths: BTreeSet::new(),
    }
}

fn destination_with_checker(bytes: &[u8]) -> TempDir {
    let root = tempdir().unwrap();
    write_executable(&root.path().join(LEGACY_CHECKER_PATH), bytes);
    root
}

#[test]
fn durable_generation_table_contains_only_bounded_identities() {
    assert!(!KNOWN_LEGACY_ASSETS.is_empty());
    assert!(KNOWN_LEGACY_ASSETS.len() <= 16);
    for asset in KNOWN_LEGACY_ASSETS {
        assert_eq!(asset.path, LEGACY_CHECKER_PATH);
        assert_eq!(asset.sha256.len(), 64);
        assert!(asset.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert!(asset.executable);
    }
    let serialized = serde_json::to_string(&KNOWN_LEGACY_ASSETS.len()).unwrap();
    assert!(!serialized.contains("#!/"));
}

#[test]
fn durable_table_retains_the_last_published_source_identity() {
    let asset = KNOWN_LEGACY_ASSETS
        .iter()
        .find(|asset| asset.generation == "rust-loc-v5-source")
        .expect("last published source generation");
    assert_eq!(
        asset.sha256,
        "56fc9fe067912c47aa939f9f0044a34111b9361f3ef9e3bb47274e17cd735b8c"
    );
}

#[test]
fn recognized_checker_is_retained_in_phase_one_and_registered_without_source_copy() {
    let destination = destination_with_checker(b"generated checker\n");
    let mut staged = staged_checker(b"generated checker\n");

    let report =
        prepare_legacy_migration(destination.path(), &mut staged, &BTreeSet::new()).unwrap();

    assert_eq!(report.status, "phase_one_retained");
    assert_eq!(report.generation.as_deref(), Some("rust-loc-rendered-v1"));
    assert_eq!(report.rerun_command, Some(RERUN_COMMAND));
    assert_eq!(
        fs::read(staged.destination.join(LEGACY_CHECKER_PATH)).unwrap(),
        b"generated checker\n"
    );
    assert!(staged.active_paths.contains(Path::new(LEGACY_CHECKER_PATH)));
    assert!(
        staged
            .active_paths
            .contains(Path::new(LEGACY_REGISTRY_PATH))
    );
    let registry = read_registry(&staged.destination).unwrap();
    assert_eq!(registry.assets.len(), 1);
    assert_eq!(registry.assets[0].sha256, digest(b"generated checker\n"));
}

#[test]
fn modified_checker_is_preserved_as_authored_and_deowned() {
    let destination = destination_with_checker(b"authored checker\n");
    let mut staged = staged_checker(b"generated checker\n");
    let prior = BTreeSet::from([PathBuf::from(LEGACY_CHECKER_PATH)]);

    let report = prepare_legacy_migration(destination.path(), &mut staged, &prior).unwrap();

    assert_eq!(report.status, "preserved_authored");
    assert!(!staged.active_paths.contains(Path::new(LEGACY_CHECKER_PATH)));
    assert!(
        !staged
            .retirement_paths
            .contains(Path::new(LEGACY_CHECKER_PATH))
    );
    assert!(!staged.destination.join(LEGACY_CHECKER_PATH).exists());
    assert_eq!(
        fs::read(destination.path().join(LEGACY_CHECKER_PATH)).unwrap(),
        b"authored checker\n"
    );
}

#[test]
fn fresh_repository_omits_the_bash_checker_and_its_managed_ownership() {
    let destination = tempdir().unwrap();
    let mut staged = staged_checker(b"generated checker\n");

    let report =
        prepare_legacy_migration(destination.path(), &mut staged, &BTreeSet::new()).unwrap();

    assert_eq!(report.status, "absent");
    assert!(!staged.active_paths.contains(Path::new(LEGACY_CHECKER_PATH)));
    assert!(!staged.destination.join(LEGACY_CHECKER_PATH).exists());
    let managed = managed_paths::load_manifest(&staged.destination)
        .unwrap()
        .unwrap();
    assert!(!managed.contains(Path::new(LEGACY_CHECKER_PATH)));
}
