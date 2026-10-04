use super::*;
use std::os::unix::process::CommandExt;

const UMASK_CHILD_ENV: &str = "JIG_VAULT_RESTORE_UMASK_CHILD";

fn private_tempdir() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
    temp
}

fn rerun_current_test_with_umask(mode: libc::mode_t) -> bool {
    if std::env::var_os(UMASK_CHILD_ENV).is_some() {
        return false;
    }
    let test_name = std::thread::current()
        .name()
        .expect("test harness thread has no name")
        .to_owned();
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .arg(&test_name)
        .arg("--exact")
        .arg("--nocapture")
        .env(UMASK_CHILD_ENV, "1");
    unsafe {
        command.pre_exec(move || {
            libc::umask(mode);
            Ok(())
        });
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "test subprocess failed under umask {mode:03o}:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    true
}

#[test]
fn preflight_creates_private_missing_parents_but_keeps_the_vault_home_absent() {
    if rerun_current_test_with_umask(0o777) {
        return;
    }
    let temp = private_tempdir();
    let parent = temp.path().join("vault-base/scopes");
    let home = parent.join("repo-scope");

    let target = preflight_target(home.clone()).unwrap();

    assert_eq!(target.parent, fs::canonicalize(&parent).unwrap());
    assert_eq!(
        target.home,
        fs::canonicalize(&parent).unwrap().join("repo-scope")
    );
    assert!(!home.exists());
    for created in [temp.path().join("vault-base"), parent] {
        assert_eq!(
            fs::metadata(created).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
}

#[test]
fn preflight_refuses_to_create_parents_below_a_group_writable_ancestor() {
    let temp = private_tempdir();
    let shared = temp.path().join("shared");
    fs::create_dir(&shared).unwrap();
    fs::set_permissions(&shared, fs::Permissions::from_mode(0o770)).unwrap();
    let parent = shared.join("vault-base/scopes");

    let error = preflight_target(parent.join("repo-scope"))
        .unwrap_err()
        .to_string();

    assert!(error.contains("shared-writable ancestor"), "{error}");
    assert!(!parent.exists());
}

#[test]
fn preflight_allows_a_sticky_shared_writable_boundary_owned_by_the_current_user() {
    let temp = private_tempdir();
    let shared = temp.path().join("shared");
    fs::create_dir(&shared).unwrap();
    fs::set_permissions(&shared, fs::Permissions::from_mode(0o1770)).unwrap();
    let parent = shared.join("vault-base/scopes");

    let target = preflight_target(parent.join("repo-scope")).unwrap();

    assert_eq!(target.parent, fs::canonicalize(&parent).unwrap());
    assert!(!target.home.exists());
}

#[test]
fn sticky_boundary_policy_rejects_an_untrusted_directory_owner() {
    let effective_user = unsafe { libc::geteuid() };
    let other_user = if effective_user == u32::MAX {
        1
    } else {
        effective_user + 1
    };

    assert!(creation_boundary_is_safe(
        0o1770,
        effective_user,
        effective_user
    ));
    assert!(creation_boundary_is_safe(0o1777, 0, effective_user));
    assert!(!creation_boundary_is_safe(
        0o1770,
        other_user,
        effective_user
    ));
    assert!(!creation_boundary_is_safe(
        0o0770,
        effective_user,
        effective_user
    ));
}

#[test]
fn preflight_resolves_a_bare_relative_missing_parent_from_the_current_directory() {
    let temp = private_tempdir();
    let parent = Path::new("recovery/vault-base/scopes");

    let prepared = prepare_target_parent_from(parent, temp.path()).unwrap();

    let expected = temp.path().join(parent);
    assert_eq!(prepared, fs::canonicalize(&expected).unwrap());
    assert!(expected.is_dir());
}

#[test]
fn preflight_rejects_parent_traversal_through_a_missing_component_without_mutation() {
    let temp = private_tempdir();
    let parent = Path::new("recovery/../vault-base/scopes");

    let error = prepare_target_parent_from(parent, temp.path())
        .unwrap_err()
        .to_string();

    assert!(error.contains("cannot traverse through a missing component"));
    assert!(!temp.path().join("recovery").exists());
    assert!(!temp.path().join("vault-base").exists());
}

#[test]
fn preflight_preserves_leading_parent_traversal_when_the_prefix_exists() {
    let temp = private_tempdir();
    let invocation_dir = temp.path().join("invocation");
    fs::create_dir(&invocation_dir).unwrap();
    fs::set_permissions(&invocation_dir, fs::Permissions::from_mode(0o700)).unwrap();
    let parent = Path::new("../recovery/vault-base/scopes");

    let prepared = prepare_target_parent_from(parent, &invocation_dir).unwrap();

    let expected = temp.path().join("recovery/vault-base/scopes");
    assert_eq!(prepared, fs::canonicalize(&expected).unwrap());
    assert!(expected.is_dir());
}

#[test]
fn preflight_refuses_an_existing_symlink_above_the_creation_boundary() {
    if rerun_current_test_with_umask(0o000) {
        return;
    }
    let temp = private_tempdir();
    let real = temp.path().join("real");
    let existing = real.join("existing");
    fs::create_dir_all(&existing).unwrap();
    fs::set_permissions(&real, fs::Permissions::from_mode(0o700)).unwrap();
    fs::set_permissions(&existing, fs::Permissions::from_mode(0o700)).unwrap();
    let link = temp.path().join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let parent = link.join("existing/vault-base/scopes");

    let error = preflight_target(parent.join("repo-scope"))
        .unwrap_err()
        .to_string();

    assert!(error.contains("symlinked ancestor"), "{error}");
    assert!(!existing.join("vault-base").exists());
}

#[test]
fn owned_staging_cleanup_removes_only_validated_generated_entries() {
    let temp = private_tempdir();
    let target = preflight_target(temp.path().join("restored-home")).unwrap();
    let mut staging = OwnedStaging::create(&target).unwrap();
    let staging_path = staging.path.clone();
    staging.write_file(VAULT_FILE, b"vault").unwrap();
    staging.write_file(AUDIT_FILE, b"audit").unwrap();
    staging.cleanup().unwrap();
    assert!(!staging_path.exists());
    assert!(!target.home.exists());
}

#[test]
fn staging_cleanup_refuses_a_replaced_directory_identity() {
    let temp = private_tempdir();
    let target = preflight_target(temp.path().join("restored-home")).unwrap();
    let mut staging = OwnedStaging::create(&target).unwrap();
    let original = staging.path.with_extension("original-stage");
    fs::rename(&staging.path, &original).unwrap();
    fs::create_dir(&staging.path).unwrap();
    fs::set_permissions(&staging.path, fs::Permissions::from_mode(0o700)).unwrap();

    let error = staging.cleanup().unwrap_err();
    assert!(error.to_string().contains("identity changed"));
    assert!(staging.path.exists());
    assert!(original.exists());

    // Test-only explicit cleanup of the two exact paths. Disable the
    // guard first so Drop cannot act on the replacement.
    staging.active = false;
    fs::remove_dir(&staging.path).unwrap();
    fs::remove_dir(&original).unwrap();
}

#[test]
fn atomic_install_never_replaces_a_raced_target() {
    let temp = private_tempdir();
    let target = preflight_target(temp.path().join("restored-home")).unwrap();
    let mut staging = OwnedStaging::create(&target).unwrap();
    let staging_path = staging.path.clone();
    staging.write_file(VAULT_FILE, b"vault").unwrap();
    staging.write_file(AUDIT_FILE, b"audit").unwrap();
    fs::create_dir(&target.home).unwrap();
    fs::write(target.home.join("marker"), b"unchanged").unwrap();

    // Invoke the final primitive directly to model the target
    // appearing after the last ordinary preflight check.
    let error = atomic_rename_noreplace(&staging.path, &target.home).unwrap_err();
    assert_eq!(
        crate::error::classified_kind(&error),
        Some(VaultErrorKind::AlreadyExists)
    );
    assert_eq!(fs::read(target.home.join("marker")).unwrap(), b"unchanged");
    staging.cleanup().unwrap();
    assert!(!staging_path.exists());
}

#[cfg(target_os = "macos")]
#[test]
fn preflight_resolves_macos_system_root_aliases_before_ancestor_checks() {
    let temp = tempfile::Builder::new()
        .prefix("jig-vault-restore-alias-")
        .tempdir_in("/tmp")
        .unwrap();
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
    assert!(temp.path().starts_with("/tmp"));

    let target = preflight_target(temp.path().join("restored-home")).unwrap();

    assert!(target.parent.starts_with("/private/tmp"), "{target:?}");
    assert_eq!(target.home, target.parent.join("restored-home"));
    assert!(!target.home.exists());
}

#[cfg(target_os = "macos")]
#[test]
fn darwin_enotsup_is_classified_as_an_unsupported_noreplace_rename() {
    assert_ne!(libc::ENOTSUP, libc::EOPNOTSUPP);
    assert!(noreplace_is_unsupported(libc::ENOTSUP));
    assert!(noreplace_is_unsupported(libc::EOPNOTSUPP));
    assert!(!noreplace_is_unsupported(libc::EEXIST));
    assert!(!noreplace_is_unsupported(libc::EXDEV));
}

#[cfg(target_os = "macos")]
fn add_acl_entry(path: &Path, entry: &str) {
    let status = std::process::Command::new("/bin/chmod")
        .arg("+a")
        .arg(entry)
        .arg(path)
        .status()
        .unwrap();
    assert!(status.success(), "chmod +a {entry:?} failed");
}

/// Uses `ls -le` as an oracle independent of the restore ACL module.
#[cfg(target_os = "macos")]
fn has_acl_entries(path: &Path) -> bool {
    let output = std::process::Command::new("/bin/ls")
        .arg("-led")
        .arg(path)
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap().lines().count() > 1
}

#[cfg(target_os = "macos")]
const INHERITED_READ_ACL: &str =
    "everyone allow read,readattr,readextattr,readsecurity,file_inherit,directory_inherit";

#[cfg(target_os = "macos")]
#[test]
fn restore_clears_inherited_acls_before_writing_or_installing() {
    let temp = private_tempdir();
    let shared = temp.path().join("shared-read");
    fs::create_dir(&shared).unwrap();
    fs::set_permissions(&shared, fs::Permissions::from_mode(0o700)).unwrap();
    add_acl_entry(&shared, INHERITED_READ_ACL);
    let control = shared.join("control");
    fs::create_dir(&control).unwrap();
    assert!(has_acl_entries(&control), "fixture ACL was not inherited");

    let chained = preflight_target(shared.join("vault-base/scopes/repo-scope")).unwrap();
    assert!(!has_acl_entries(&shared.join("vault-base")));
    assert!(!has_acl_entries(&chained.parent));

    let target = preflight_target(shared.join("restored-home")).unwrap();
    let mut staging = OwnedStaging::create(&target).unwrap();
    assert!(!has_acl_entries(&staging.path));
    staging.write_file(VAULT_FILE, b"vault").unwrap();
    staging.write_file(AUDIT_FILE, b"audit").unwrap();
    staging.install(&target).unwrap();

    for installed in [
        target.home.clone(),
        target.home.join(VAULT_FILE),
        target.home.join(AUDIT_FILE),
    ] {
        assert!(!has_acl_entries(&installed), "{}", installed.display());
    }
}

#[cfg(target_os = "macos")]
fn acl_fixture_directory(root: &Path, name: &str, entry: &str) -> PathBuf {
    let directory = root.join(name);
    fs::create_dir(&directory).unwrap();
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
    add_acl_entry(&directory, entry);
    directory
}

#[cfg(target_os = "macos")]
#[test]
fn preflight_refuses_parents_whose_acl_allows_write_or_delete() {
    // Place fixtures below the sticky shared /private/tmp: an allowed
    // `delete` still lets another user rename a parent away from there.
    let temp = tempfile::Builder::new()
        .prefix("jig-vault-restore-acl-")
        .tempdir_in("/private/tmp")
        .unwrap();
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();

    for (name, entry) in [
        ("allow-write", "everyone allow add_file,add_subdirectory"),
        ("allow-delete", "everyone allow delete"),
    ] {
        let parent = acl_fixture_directory(temp.path(), name, entry);
        for home in [
            parent.join("restored-home"),
            parent.join("vault-base/scopes/repo-scope"),
        ] {
            let error = preflight_target(home).unwrap_err().to_string();
            assert!(
                error.contains("lets other users write to, delete, or re-permission it"),
                "{entry}: {error}"
            );
        }
        assert!(!parent.join("vault-base").exists(), "{entry}");
    }

    let narrowed = acl_fixture_directory(temp.path(), "deny-delete", "everyone deny delete");
    let direct = preflight_target(narrowed.join("restored-home")).unwrap();
    assert!(!direct.home.exists());
    let chained = preflight_target(narrowed.join("vault-base/scopes/repo-scope")).unwrap();
    assert!(!chained.home.exists());
}

#[cfg(target_os = "macos")]
#[test]
fn install_refuses_a_staged_file_that_gained_an_acl() {
    let temp = private_tempdir();
    let target = preflight_target(temp.path().join("restored-home")).unwrap();
    let mut staging = OwnedStaging::create(&target).unwrap();
    staging.write_file(VAULT_FILE, b"vault").unwrap();
    staging.write_file(AUDIT_FILE, b"audit").unwrap();
    add_acl_entry(&staging.path.join(AUDIT_FILE), "everyone allow read");

    let error = staging.install(&target).unwrap_err().to_string();

    assert!(error.contains("access control list"), "{error}");
    assert!(!target.home.exists());
    staging.cleanup().unwrap();
}

/// A throwaway APFS image attached with ownership ignored, detached on drop.
#[cfg(target_os = "macos")]
struct OwnershipIgnoringVolume {
    mountpoint: PathBuf,
    _temp: tempfile::TempDir,
}

#[cfg(target_os = "macos")]
impl OwnershipIgnoringVolume {
    fn attach() -> Self {
        let temp = private_tempdir();
        let root = fs::canonicalize(temp.path()).unwrap();
        let image = root.join("volume.dmg");
        let mountpoint = root.join("mnt");
        fs::create_dir(&mountpoint).unwrap();
        run_hdiutil(&[
            "create".as_ref(),
            "-quiet".as_ref(),
            "-size".as_ref(),
            "8m".as_ref(),
            "-fs".as_ref(),
            "APFS".as_ref(),
            "-volname".as_ref(),
            "JigRestoreTest".as_ref(),
            image.as_os_str(),
        ]);
        run_hdiutil(&[
            "attach".as_ref(),
            "-quiet".as_ref(),
            "-nobrowse".as_ref(),
            "-owners".as_ref(),
            "off".as_ref(),
            "-mountpoint".as_ref(),
            mountpoint.as_os_str(),
            image.as_os_str(),
        ]);
        Self {
            mountpoint,
            _temp: temp,
        }
    }
}

#[cfg(target_os = "macos")]
impl Drop for OwnershipIgnoringVolume {
    fn drop(&mut self) {
        let _ = std::process::Command::new("/usr/bin/hdiutil")
            .args(["detach", "-quiet", "-force"])
            .arg(&self.mountpoint)
            .status();
    }
}

#[cfg(target_os = "macos")]
fn run_hdiutil(args: &[&OsStr]) {
    let output = std::process::Command::new("/usr/bin/hdiutil")
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "hdiutil {args:?} failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(target_os = "macos")]
#[test]
fn preflight_refuses_a_volume_that_ignores_ownership_before_creating_parents() {
    let volume = OwnershipIgnoringVolume::attach();
    let root = &volume.mountpoint;
    let c_root = CString::new(root.as_os_str().as_bytes()).unwrap();
    let mut stats = std::mem::MaybeUninit::<libc::statfs>::uninit();
    assert_eq!(
        unsafe { libc::statfs(c_root.as_ptr(), stats.as_mut_ptr()) },
        0
    );
    let flags = unsafe { stats.assume_init() }.f_flags;
    assert_ne!(
        flags & libc::MNT_IGNORE_OWNERSHIP as u32,
        0,
        "fixture volume does not ignore ownership"
    );
    let existing = root.join("existing");
    fs::create_dir(&existing).unwrap();
    fs::set_permissions(&existing, fs::Permissions::from_mode(0o700)).unwrap();

    for home in [
        existing.join("restored-home"),
        root.join("vault-base/scopes/repo-scope"),
    ] {
        let error = preflight_target(home).unwrap_err().to_string();
        assert!(error.contains("ignores file ownership"), "{error}");
    }
    assert!(!root.join("vault-base").exists());
}

#[test]
fn install_refuses_unexpected_or_non_private_staged_entries() {
    type Tamper = fn(&Path);
    let cases: [(&str, Tamper, Tamper, &str); 3] = [
        (
            "unexpected-entry",
            |stage| fs::write(stage.join("extra"), b"extra").unwrap(),
            |stage| fs::remove_file(stage.join("extra")).unwrap(),
            "unexpected entry",
        ),
        (
            "group-readable-file",
            |stage| {
                fs::set_permissions(stage.join(AUDIT_FILE), fs::Permissions::from_mode(0o640))
                    .unwrap();
            },
            |stage| {
                fs::set_permissions(stage.join(AUDIT_FILE), fs::Permissions::from_mode(0o600))
                    .unwrap();
            },
            "not owner-only",
        ),
        (
            "symlinked-lock",
            |stage| std::os::unix::fs::symlink(VAULT_FILE, stage.join(LOCK_FILE)).unwrap(),
            |stage| fs::remove_file(stage.join(LOCK_FILE)).unwrap(),
            "not a regular file",
        ),
    ];
    let temp = private_tempdir();
    for (label, tamper, untamper, expected) in cases {
        let target = preflight_target(temp.path().join(label)).unwrap();
        let mut staging = OwnedStaging::create(&target).unwrap();
        staging.write_file(VAULT_FILE, b"vault").unwrap();
        staging.write_file(AUDIT_FILE, b"audit").unwrap();
        tamper(&staging.path);

        let error = staging.install(&target).unwrap_err().to_string();

        assert!(error.contains(expected), "{label}: {error}");
        assert!(!target.home.exists(), "{label}");
        untamper(&staging.path);
        staging.cleanup().unwrap();
    }
}
