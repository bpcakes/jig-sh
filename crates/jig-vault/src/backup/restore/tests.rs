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
    let staging = FreshStaging::create(&target).unwrap();
    let staging_path = staging.path().to_path_buf();
    staging.write_file(VAULT_FILE, b"vault").unwrap();
    staging.write_file(AUDIT_FILE, b"audit").unwrap();
    // Fresh staging this operation still owns is cleaned up when dropped.
    drop(staging);
    assert!(!staging_path.exists());
    assert!(!target.home.exists());
}

#[test]
fn staging_cleanup_refuses_a_replaced_directory_identity() {
    let temp = private_tempdir();
    let target = preflight_target(temp.path().join("restored-home")).unwrap();
    let staging = FreshStaging::create(&target).unwrap();
    let staging_path = staging.path().to_path_buf();
    let original = staging_path.with_extension("original-stage");
    fs::rename(&staging_path, &original).unwrap();
    fs::create_dir(&staging_path).unwrap();
    fs::set_permissions(&staging_path, fs::Permissions::from_mode(0o700)).unwrap();

    // Abandoning consumes ownership, so nothing can act on the replacement.
    let error = staging.abandon(anyhow::anyhow!("probe"));
    assert!(error.to_string().contains("identity changed"), "{error:#}");
    assert!(staging_path.exists());
    assert!(original.exists());

    // Test-only explicit cleanup of the two exact paths.
    fs::remove_dir(&staging_path).unwrap();
    fs::remove_dir(&original).unwrap();
}

#[test]
fn atomic_install_never_replaces_a_raced_target() {
    let temp = private_tempdir();
    let target = preflight_target(temp.path().join("restored-home")).unwrap();
    let staging = FreshStaging::create(&target).unwrap();
    let staging_path = staging.path().to_path_buf();
    staging.write_file(VAULT_FILE, b"vault").unwrap();
    staging.write_file(AUDIT_FILE, b"audit").unwrap();
    fs::create_dir(&target.home).unwrap();
    fs::write(target.home.join("marker"), b"unchanged").unwrap();

    // Invoke the final primitive directly to model the target
    // appearing after the last ordinary preflight check.
    let error = atomic_rename_noreplace(&staging_path, &target.home).unwrap_err();
    assert_eq!(
        crate::error::classified_kind(&error),
        Some(VaultErrorKind::AlreadyExists)
    );
    assert_eq!(fs::read(target.home.join("marker")).unwrap(), b"unchanged");
    drop(staging);
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
use crate::acl::test_support::{
    UNSAFE_DIRECTORY_ACLS, UNSAFE_DIRECTORY_REFUSAL, acl_fixture_directory, add_acl_entry,
    has_acl_entries, inheriting_read_directory, sticky_shared_tempdir,
};

#[cfg(target_os = "macos")]
#[test]
fn restore_clears_inherited_acls_before_writing_or_installing() {
    let temp = private_tempdir();
    let shared = inheriting_read_directory(temp.path());

    let chained = preflight_target(shared.join("vault-base/scopes/repo-scope")).unwrap();
    assert!(!has_acl_entries(&shared.join("vault-base")));
    assert!(!has_acl_entries(&chained.parent));

    let target = preflight_target(shared.join("restored-home")).unwrap();
    let staging = FreshStaging::create(&target).unwrap();
    assert!(!has_acl_entries(staging.path()));
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
#[test]
fn preflight_refuses_parents_whose_acl_allows_write_or_delete() {
    let temp = sticky_shared_tempdir("jig-vault-restore-acl-");

    for (name, entry) in UNSAFE_DIRECTORY_ACLS {
        let parent = acl_fixture_directory(temp.path(), name, entry);
        for home in [
            parent.join("restored-home"),
            parent.join("vault-base/scopes/repo-scope"),
        ] {
            let error = preflight_target(home).unwrap_err().to_string();
            assert!(error.contains(UNSAFE_DIRECTORY_REFUSAL), "{entry}: {error}");
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
    let staging = FreshStaging::create(&target).unwrap();
    let staging_path = staging.path().to_path_buf();
    staging.write_file(VAULT_FILE, b"vault").unwrap();
    staging.write_file(AUDIT_FILE, b"audit").unwrap();
    add_acl_entry(&staging_path.join(AUDIT_FILE), "everyone allow read");

    // A refused install cleans up the staging it still owned.
    let error = format!("{:#}", staging.install(&target).unwrap_err());

    assert!(error.contains("access control list"), "{error}");
    assert!(!target.home.exists());
    assert!(!staging_path.exists());
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
        let staging = FreshStaging::create(&target).unwrap();
        let staging_path = staging.path().to_path_buf();
        staging.write_file(VAULT_FILE, b"vault").unwrap();
        staging.write_file(AUDIT_FILE, b"audit").unwrap();
        tamper(&staging_path);

        let error = format!("{:#}", staging.install(&target).unwrap_err());

        assert!(error.contains(expected), "{label}: {error}");
        assert!(!target.home.exists(), "{label}");
        // Cleanup stopped at the tampered entry; remove what is left.
        untamper(&staging_path);
        for name in [VAULT_FILE, AUDIT_FILE] {
            let _ = fs::remove_file(staging_path.join(name));
        }
        fs::remove_dir(&staging_path).unwrap();
    }
}

#[test]
fn ancestor_owner_policy_trusts_only_the_current_user_and_root() {
    let effective_user = unsafe { libc::geteuid() };
    let other_user = if effective_user == u32::MAX {
        1
    } else {
        effective_user + 1
    };

    assert!(ancestor_owner_is_trusted(effective_user, effective_user));
    assert!(ancestor_owner_is_trusted(0, effective_user));
    assert!(!ancestor_owner_is_trusted(other_user, effective_user));
}

#[test]
fn preflight_and_revalidation_refuse_a_shared_writable_higher_ancestor() {
    let temp = private_tempdir();
    let shared = temp.path().join("shared");
    let private = shared.join("private");
    fs::create_dir_all(&private).unwrap();
    fs::set_permissions(&private, fs::Permissions::from_mode(0o700)).unwrap();
    fs::set_permissions(&shared, fs::Permissions::from_mode(0o770)).unwrap();

    for home in [
        private.join("restored-home"),
        private.join("vault-base/scopes/repo-scope"),
    ] {
        let error = preflight_target(home).unwrap_err().to_string();
        assert!(error.contains("shared-writable ancestor"), "{error}");
    }
    assert!(!private.join("vault-base").exists());

    fs::set_permissions(&shared, fs::Permissions::from_mode(0o1770)).unwrap();
    let target = preflight_target(private.join("restored-home")).unwrap();
    fs::set_permissions(&shared, fs::Permissions::from_mode(0o770)).unwrap();

    let error = revalidate_target(&target).unwrap_err().to_string();
    assert!(error.contains("shared-writable ancestor"), "{error}");
    let error = FreshStaging::create(&target).err().unwrap().to_string();
    assert!(error.contains("shared-writable ancestor"), "{error}");
    assert!(!target.home.exists());
}

#[test]
fn preflight_accepts_a_traversable_but_unlistable_higher_ancestor() {
    let temp = private_tempdir();
    let unlistable = temp.path().join("unlistable");
    let private = unlistable.join("private");
    fs::create_dir_all(&private).unwrap();
    fs::set_permissions(&private, fs::Permissions::from_mode(0o700)).unwrap();
    fs::set_permissions(&unlistable, fs::Permissions::from_mode(0o300)).unwrap();

    let result = preflight_target(private.join("restored-home"));

    fs::set_permissions(&unlistable, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(!result.unwrap().home.exists());
}

#[cfg(target_os = "macos")]
#[test]
fn preflight_and_revalidation_refuse_an_unsafe_acl_on_a_higher_ancestor() {
    let temp = sticky_shared_tempdir("jig-vault-restore-acl-ancestor-");
    let upper = acl_fixture_directory(temp.path(), "upper", "everyone allow delete");
    let private = upper.join("private");
    fs::create_dir(&private).unwrap();
    fs::set_permissions(&private, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(!has_acl_entries(&private));

    for home in [
        private.join("restored-home"),
        private.join("vault-base/scopes/repo-scope"),
    ] {
        let error = preflight_target(home).unwrap_err().to_string();
        assert!(error.contains(UNSAFE_DIRECTORY_REFUSAL), "{error}");
    }
    assert!(!private.join("vault-base").exists());

    let later = temp.path().join("later");
    let later_private = later.join("private");
    fs::create_dir_all(&later_private).unwrap();
    fs::set_permissions(&later, fs::Permissions::from_mode(0o700)).unwrap();
    fs::set_permissions(&later_private, fs::Permissions::from_mode(0o700)).unwrap();
    let target = preflight_target(later_private.join("restored-home")).unwrap();
    add_acl_entry(&later, "everyone allow delete");

    let error = revalidate_target(&target).unwrap_err().to_string();
    assert!(error.contains(UNSAFE_DIRECTORY_REFUSAL), "{error}");
    assert!(!target.home.exists());
}

#[cfg(target_os = "macos")]
#[test]
fn whole_path_checks_refuse_an_ownership_ignoring_volume_above_the_parent() {
    // Mounting an ownership-honoring volume inside this one would model a
    // nested mount exactly, but hdiutil requires root for `-owners on`. The
    // whole-path walk visits the volume's top directory before any deeper
    // directory, so a refusal naming it proves higher ancestors are checked
    // rather than only the creation boundary and target parent.
    let volume = OwnershipIgnoringVolume::attach();
    let parent = volume.mountpoint.join("upper/private");
    fs::create_dir_all(&parent).unwrap();
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o700)).unwrap();

    let error = validate_trusted_ancestors(&parent).unwrap_err().to_string();

    assert!(error.contains("ignores file ownership"), "{error}");
    assert!(
        error.ends_with(&volume.mountpoint.display().to_string()),
        "{error}"
    );
}

/// Run as root, because only root can give a directory to another user:
/// `cargo test -p jig-vault --lib --no-run`, then run the printed test
/// binary with `sudo <binary> --ignored --exact <this test's path>`.
#[test]
#[ignore = "requires root to give a directory to another non-root user"]
fn preflight_and_revalidation_refuse_an_ancestor_owned_by_another_user() {
    const OTHER_USER: u32 = 1;
    assert_eq!(unsafe { libc::geteuid() }, 0, "run this test as root");
    // A root-owned sticky root keeps every fixture ancestor trusted, unlike a
    // TMPDIR inherited from the invoking user.
    let temp = tempfile::Builder::new()
        .prefix("jig-vault-restore-foreign-")
        .tempdir_in("/tmp")
        .unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    let foreign = root.join("foreign");
    let private = foreign.join("private");
    fs::create_dir_all(&private).unwrap();
    fs::set_permissions(&foreign, fs::Permissions::from_mode(0o755)).unwrap();
    fs::set_permissions(&private, fs::Permissions::from_mode(0o700)).unwrap();
    std::os::unix::fs::chown(&foreign, Some(OTHER_USER), None).unwrap();

    for home in [
        private.join("restored-home"),
        private.join("vault-base/scopes/repo-scope"),
    ] {
        let error = preflight_target(home).unwrap_err().to_string();
        assert!(error.contains("owned by another user"), "{error}");
        assert!(error.ends_with(&foreign.display().to_string()), "{error}");
    }
    assert!(!private.join("vault-base").exists());

    std::os::unix::fs::chown(&foreign, Some(0), None).unwrap();
    let target = preflight_target(private.join("restored-home")).unwrap();
    std::os::unix::fs::chown(&foreign, Some(OTHER_USER), None).unwrap();

    let error = revalidate_target(&target).unwrap_err().to_string();
    assert!(error.contains("owned by another user"), "{error}");
    assert!(!target.home.exists());
}
