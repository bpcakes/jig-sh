use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use anyhow::{Context, anyhow, bail};

use self::error::{PrivateOutputConflict, install_error_kind, preflight_error_kind};
use super::{OutputFailureStage, OutputInstallFailure};

mod error;

pub(super) fn preflight_private_destination(
    path: &Path,
    overwrite: bool,
) -> Result<(), OutputInstallFailure> {
    preflight(path, overwrite)
        .map(|_| ())
        .map_err(|error| OutputInstallFailure {
            stage: OutputFailureStage::Preflight,
            kind: preflight_error_kind(&error),
            error,
        })
}

pub(super) fn preview_private_destination(
    path: &Path,
) -> Result<PrivateDestinationPrecondition, OutputInstallFailure> {
    preview(path).map_err(|error| OutputInstallFailure {
        stage: OutputFailureStage::Preflight,
        kind: preflight_error_kind(&error),
        error,
    })
}

pub(super) fn prepare_private_bytes(
    path: &Path,
    bytes: &[u8],
    overwrite: bool,
) -> Result<PreparedPrivateOutput, OutputInstallFailure> {
    let path = preflight(path, overwrite).map_err(|error| OutputInstallFailure {
        stage: OutputFailureStage::Preflight,
        kind: preflight_error_kind(&error),
        error,
    })?;
    prepare_bytes(
        path,
        bytes,
        if overwrite {
            InstallPolicy::Upsert
        } else {
            InstallPolicy::Create
        },
    )
}

pub(super) fn prepare_private_bytes_if_unchanged(
    precondition: PrivateDestinationPrecondition,
    bytes: &[u8],
    allow_replace: bool,
) -> Result<PreparedPrivateOutput, OutputInstallFailure> {
    let result = (|| -> anyhow::Result<_> {
        if precondition.destination_exists() && !allow_replace {
            return Err(PrivateOutputConflict::ExistingWithoutReplacement(
                precondition.destination.clone(),
            )
            .into());
        }
        validate_precondition(&precondition)?;
        let policy = InstallPolicy::Exact(precondition.state);
        Ok((prepared_path(&precondition)?, policy))
    })();
    let (path, policy) = result.map_err(|error| OutputInstallFailure {
        stage: OutputFailureStage::Preflight,
        kind: preflight_error_kind(&error),
        error,
    })?;
    prepare_bytes(path, bytes, policy)
}

fn prepare_bytes(
    mut path: PreparedPath,
    bytes: &[u8],
    policy: InstallPolicy,
) -> Result<PreparedPrivateOutput, OutputInstallFailure> {
    path.temporary_identity = match write_temporary(&path, bytes) {
        Ok(identity) => Some(identity),
        Err(error) => {
            // The create-new file is still owned by this operation here;
            // no path-based handoff has occurred yet.
            let _ = fs::remove_file(&path.temporary);
            return Err(OutputInstallFailure {
                stage: OutputFailureStage::Sink,
                kind: install_error_kind(&error),
                error,
            });
        }
    };
    Ok(PreparedPrivateOutput {
        path: Some(path),
        policy,
    })
}

pub(super) struct PreparedPrivateOutput {
    path: Option<PreparedPath>,
    policy: InstallPolicy,
}

impl PreparedPrivateOutput {
    pub(super) fn install(mut self) -> Result<(), OutputInstallFailure> {
        let path = self
            .path
            .take()
            .expect("prepared private output installs at most once");
        install(path, self.policy).map_err(|error| OutputInstallFailure {
            stage: OutputFailureStage::Sink,
            kind: install_error_kind(&error),
            error,
        })
    }
}

impl Drop for PreparedPrivateOutput {
    fn drop(&mut self) {
        if let Some(path) = self.path.take() {
            let _ = remove_temporary_if_identity_matches(&path);
        }
    }
}

struct PreparedPath {
    destination: PathBuf,
    parent: PathBuf,
    temporary: PathBuf,
    temporary_identity: Option<FileIdentity>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FileIdentity {
    device: u64,
    inode: u64,
    owner: u32,
    byte_len: u64,
    changed_at_secs: i64,
    changed_at_nanos: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DestinationState {
    Absent,
    Existing(FileIdentity),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InstallPolicy {
    Create,
    Upsert,
    Exact(DestinationState),
}

pub(super) struct PrivateDestinationPrecondition {
    destination: PathBuf,
    parent: PathBuf,
    state: DestinationState,
}

impl PrivateDestinationPrecondition {
    pub(super) const fn destination_exists(&self) -> bool {
        matches!(self.state, DestinationState::Existing(_))
    }
}

fn preflight(path: &Path, overwrite: bool) -> anyhow::Result<PreparedPath> {
    let precondition = preview(path)?;
    if precondition.destination_exists() && !overwrite {
        return Err(
            PrivateOutputConflict::ExistingWithoutOverwrite(precondition.destination).into(),
        );
    }
    prepared_path(&precondition)
}

fn preview(path: &Path) -> anyhow::Result<PrivateDestinationPrecondition> {
    let path = crate::path_security::physical_path(path, "vault output")?;
    path.file_name()
        .ok_or_else(|| anyhow!("vault output path must name a file: {}", path.display()))?;
    let parent = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
        _ => PathBuf::from("."),
    };
    reject_symlinked_ancestors(&parent)?;
    let parent_metadata = fs::metadata(&parent)
        .with_context(|| format!("failed to inspect output parent {}", parent.display()))?;
    if !parent_metadata.is_dir() {
        bail!(
            "vault output parent is not a directory: {}",
            parent.display()
        );
    }
    let parent_mode = parent_metadata.permissions().mode();
    if parent_mode & 0o022 != 0 && parent_mode & 0o1000 == 0 {
        bail!(
            "refusing private vault output in shared-writable non-sticky parent {}",
            parent.display()
        );
    }
    let state = destination_state(&path)?;

    Ok(PrivateDestinationPrecondition {
        destination: path,
        parent,
        state,
    })
}

fn prepared_path(precondition: &PrivateDestinationPrecondition) -> anyhow::Result<PreparedPath> {
    let file_name = precondition.destination.file_name().ok_or_else(|| {
        anyhow!(
            "vault output path must name a file: {}",
            precondition.destination.display()
        )
    })?;
    let mut temporary_name = OsString::from(".");
    temporary_name.push(file_name);
    temporary_name.push(format!(
        ".{}.{}.jig-vault-output.tmp",
        std::process::id(),
        ulid::Ulid::new()
    ));
    Ok(PreparedPath {
        destination: precondition.destination.clone(),
        temporary: precondition.parent.join(temporary_name),
        parent: precondition.parent.clone(),
        temporary_identity: None,
    })
}

fn validate_precondition(precondition: &PrivateDestinationPrecondition) -> anyhow::Result<()> {
    reject_symlinked_ancestors(&precondition.parent)?;
    let current = destination_state(&precondition.destination)?;
    if current != precondition.state {
        return Err(
            PrivateOutputConflict::ChangedSincePreview(precondition.destination.clone()).into(),
        );
    }
    Ok(())
}

fn write_temporary(prepared: &PreparedPath, bytes: &[u8]) -> anyhow::Result<FileIdentity> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&prepared.temporary)
        .with_context(|| {
            format!(
                "failed to create private output temporary file beside {}",
                prepared.destination.display()
            )
        })?;
    file.write_all(bytes).with_context(|| {
        format!(
            "failed to write private vault output {}",
            prepared.destination.display()
        )
    })?;
    file.set_permissions(fs::Permissions::from_mode(0o600))
        .with_context(|| {
            format!(
                "failed to restrict private vault output permissions for {}",
                prepared.destination.display()
            )
        })?;
    let metadata = file.metadata().with_context(|| {
        format!(
            "failed to inspect private vault output temporary file for {}",
            prepared.destination.display()
        )
    })?;
    if !metadata.is_file()
        || metadata.permissions().mode() & 0o077 != 0
        || metadata.uid() != unsafe { libc::geteuid() }
    {
        bail!(
            "private vault output temporary file is not an owner-only regular file for {}",
            prepared.destination.display()
        );
    }
    file.sync_all().with_context(|| {
        format!(
            "failed to sync private vault output {}",
            prepared.destination.display()
        )
    })?;
    Ok(file_identity(&metadata))
}

fn install(prepared: PreparedPath, policy: InstallPolicy) -> anyhow::Result<()> {
    let PreparedPath {
        destination,
        parent,
        temporary,
        temporary_identity,
    } = prepared;
    let temporary_identity =
        temporary_identity.expect("prepared private output records its temporary file identity");
    let result = (|| -> anyhow::Result<()> {
        // Recheck immediately before the atomic namespace operation. This
        // narrows same-user directory-entry races without claiming an OS
        // isolation boundary stronger than the containing directory.
        reject_symlinked_ancestors(&parent)?;
        validate_install_policy(&destination, policy)?;
        validate_temporary_identity(&temporary, temporary_identity)?;
        if matches!(
            policy,
            InstallPolicy::Upsert | InstallPolicy::Exact(DestinationState::Existing(_))
        ) {
            fs::rename(&temporary, &destination).with_context(|| {
                format!(
                    "failed to atomically replace private vault output {}",
                    destination.display()
                )
            })?;
        } else {
            // A same-directory hard link is an atomic no-replace install:
            // it fails if any directory entry already occupies the leaf.
            fs::hard_link(&temporary, &destination).with_context(|| {
                format!(
                    "failed to atomically install private vault output without replacing {}",
                    destination.display()
                )
            })?;
            fs::remove_file(&temporary).with_context(|| {
                format!(
                    "private vault output was installed at {}, but its temporary link could not be removed",
                    destination.display()
                )
            })?;
        }
        sync_parent(&parent)?;
        Ok(())
    })();

    if result.is_err() {
        // Remove only the exact inode this operation created. A replaced
        // directory entry is never deleted during error cleanup.
        let cleanup_path = PreparedPath {
            destination,
            parent,
            temporary,
            temporary_identity: Some(temporary_identity),
        };
        let _ = remove_temporary_if_identity_matches(&cleanup_path);
    }
    result
}

fn validate_install_policy(path: &Path, policy: InstallPolicy) -> anyhow::Result<()> {
    let current = destination_state(path)?;
    match policy {
        InstallPolicy::Create if current == DestinationState::Absent => Ok(()),
        InstallPolicy::Create => {
            Err(PrivateOutputConflict::ExistingWithoutOverwrite(path.to_path_buf()).into())
        }
        InstallPolicy::Upsert => Ok(()),
        InstallPolicy::Exact(expected) if current == expected => Ok(()),
        InstallPolicy::Exact(_) => {
            Err(PrivateOutputConflict::ChangedSincePreview(path.to_path_buf()).into())
        }
    }
}

fn validate_temporary_identity(path: &Path, expected: FileIdentity) -> anyhow::Result<()> {
    let metadata = fs::symlink_metadata(path).with_context(|| {
        format!(
            "failed to revalidate private output temporary file {}",
            path.display()
        )
    })?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || file_identity(&metadata) != expected
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.permissions().mode() & 0o077 != 0
    {
        bail!(
            "private output temporary file identity changed before installation for {}",
            path.display()
        );
    }
    Ok(())
}

fn remove_temporary_if_identity_matches(prepared: &PreparedPath) -> anyhow::Result<()> {
    let Some(identity) = prepared.temporary_identity else {
        return Ok(());
    };
    match fs::symlink_metadata(&prepared.temporary) {
        Ok(_) => validate_temporary_identity(&prepared.temporary, identity)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(error).with_context(|| {
                format!(
                    "failed to inspect private output temporary file {} during cleanup",
                    prepared.temporary.display()
                )
            });
        }
    }
    fs::remove_file(&prepared.temporary).with_context(|| {
        format!(
            "failed to remove private output temporary file {}",
            prepared.temporary.display()
        )
    })
}

fn reject_symlinked_ancestors(path: &Path) -> anyhow::Result<()> {
    let absolute = crate::path_security::physical_path(path, "vault output")?;
    let mut ancestors = absolute.ancestors().collect::<Vec<_>>();
    ancestors.reverse();
    for ancestor in ancestors {
        let metadata = fs::symlink_metadata(ancestor).with_context(|| {
            format!(
                "failed to inspect output path ancestor {}",
                ancestor.display()
            )
        })?;
        if metadata.file_type().is_symlink() {
            bail!(
                "refusing private vault output through symlinked parent {}",
                ancestor.display()
            );
        }
    }
    Ok(())
}

fn destination_state(path: &Path) -> anyhow::Result<DestinationState> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => bail!(
            "refusing to write private vault output through symlink {}",
            path.display()
        ),
        Ok(metadata) if !metadata.is_file() => bail!(
            "refusing to replace non-regular private vault output {}",
            path.display()
        ),
        Ok(metadata) => Ok(DestinationState::Existing(file_identity(&metadata))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(DestinationState::Absent),
        Err(error) => {
            Err(error).with_context(|| format!("failed to inspect vault output {}", path.display()))
        }
    }
}

fn file_identity(metadata: &fs::Metadata) -> FileIdentity {
    FileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
        owner: metadata.uid(),
        byte_len: metadata.len(),
        changed_at_secs: metadata.ctime(),
        changed_at_nanos: metadata.ctime_nsec(),
    }
}

fn sync_parent(parent: &Path) -> anyhow::Result<()> {
    let directory = File::open(parent)
        .with_context(|| format!("failed to open output parent {}", parent.display()))?;
    directory
        .sync_all()
        .with_context(|| format!("failed to sync output parent {}", parent.display()))
}

#[cfg(all(test, target_os = "macos"))]
mod macos_path_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    use crate::VaultErrorKind;

    fn private_tempdir() -> (tempfile::TempDir, PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let root = fs::canonicalize(temp.path()).unwrap();
        (temp, root)
    }

    fn install_private_bytes(
        path: &Path,
        bytes: &[u8],
        overwrite: bool,
    ) -> std::result::Result<(), OutputInstallFailure> {
        prepare_private_bytes(path, bytes, overwrite)?.install()
    }

    #[test]
    fn classifies_private_output_conflicts_by_type_through_context() {
        let error = anyhow::Error::new(PrivateOutputConflict::ChangedSincePreview(PathBuf::from(
            "/safe/destination",
        )))
        .context("outer output context");

        assert_eq!(preflight_error_kind(&error), VaultErrorKind::AlreadyExists);
        assert_eq!(install_error_kind(&error), VaultErrorKind::AlreadyExists);
    }

    #[test]
    fn conflict_words_in_unrelated_errors_do_not_change_classification() {
        for message in [
            "unrelated path contains already exists words",
            "unrelated operation changed since preview wording",
        ] {
            let error = anyhow::Error::msg(message);
            assert_eq!(preflight_error_kind(&error), VaultErrorKind::InvalidInput);
            assert_eq!(install_error_kind(&error), VaultErrorKind::Io);
        }
    }

    #[test]
    fn install_classifier_retains_native_already_exists_errors() {
        let error = anyhow::Error::new(std::io::Error::from(std::io::ErrorKind::AlreadyExists))
            .context("atomic no-replace install failed");

        assert_eq!(install_error_kind(&error), VaultErrorKind::AlreadyExists);
    }

    #[test]
    fn installs_owner_only_bytes_without_clobbering() {
        let (_temp, root) = private_tempdir();
        let output = root.join("result.bin");
        install_private_bytes(&output, b"first\0bytes", false).unwrap();
        assert_eq!(fs::read(&output).unwrap(), b"first\0bytes");
        assert_eq!(
            fs::metadata(&output).unwrap().permissions().mode() & 0o777,
            0o600
        );

        let error = install_private_bytes(&output, b"second", false).unwrap_err();
        assert!(error.error.to_string().contains("already exists"));
        assert_eq!(fs::read(&output).unwrap(), b"first\0bytes");

        install_private_bytes(&output, b"second", true).unwrap();
        assert_eq!(fs::read(&output).unwrap(), b"second");
        assert_eq!(
            fs::metadata(&output).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn public_prepared_file_is_private_redacted_and_installs_only_when_consumed() {
        let (_temp, root) = private_tempdir();
        let output = root.join("import.env");
        let contents = b"TOKEN=jig://Production/TOKEN\n";
        let prepared = crate::PreparedPrivateFile::prepare(
            &output,
            crate::SecretBytes::new(contents.to_vec()),
            false,
        )
        .unwrap();
        assert!(!output.exists());
        let debug = format!("{prepared:?}");
        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("jig://Production/TOKEN"));

        let temporary = fs::read_dir(&root)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .next()
            .unwrap();
        assert_eq!(fs::read(&temporary).unwrap(), contents);
        assert_eq!(
            fs::metadata(&temporary).unwrap().permissions().mode() & 0o777,
            0o600
        );

        prepared.install().unwrap();
        assert_eq!(fs::read(&output).unwrap(), contents);
        assert_eq!(
            fs::metadata(&output).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
    }

    #[test]
    fn public_preflight_is_non_writing_and_applies_overwrite_policy() {
        let (_temp, root) = private_tempdir();
        let output = root.join("import.env");
        crate::PreparedPrivateFile::preflight(&output, false).unwrap();
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);

        fs::write(&output, b"existing").unwrap();
        let collision = crate::PreparedPrivateFile::preflight(&output, false).unwrap_err();
        assert_eq!(collision.kind(), VaultErrorKind::AlreadyExists);
        crate::PreparedPrivateFile::preflight(&output, true).unwrap();
        assert_eq!(fs::read(&output).unwrap(), b"existing");
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
    }

    #[test]
    fn prepared_no_clobber_rechecks_destination_and_cleans_its_temporary() {
        let (_temp, root) = private_tempdir();
        let output = root.join("import.env");
        let prepared = crate::PreparedPrivateFile::prepare(
            &output,
            crate::SecretBytes::new(b"prepared".to_vec()),
            false,
        )
        .unwrap();
        fs::write(&output, b"raced-existing").unwrap();

        let error = prepared.install().unwrap_err();
        assert_eq!(error.kind(), VaultErrorKind::AlreadyExists);
        assert_eq!(fs::read(&output).unwrap(), b"raced-existing");
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
    }

    #[test]
    fn previewed_absence_never_widens_into_upsert_permission() {
        let (_temp, root) = private_tempdir();
        let output = root.join("import.env");
        let precondition = crate::PreparedPrivateFile::preview(&output).unwrap();
        assert!(!precondition.destination_exists());
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);

        let prepared = crate::PreparedPrivateFile::prepare_if_unchanged(
            precondition,
            crate::SecretBytes::new(b"approved".to_vec()),
            true,
        )
        .unwrap();
        fs::write(&output, b"raced-existing").unwrap();

        let error = prepared.install().unwrap_err();
        assert_eq!(error.kind(), VaultErrorKind::AlreadyExists);
        assert!(error.message().contains("changed since preview"));
        assert_eq!(fs::read(&output).unwrap(), b"raced-existing");
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
    }

    #[test]
    fn previewed_existing_identity_must_survive_prepare_and_install() {
        let (_temp, root) = private_tempdir();
        let output = root.join("import.env");
        fs::write(&output, b"previewed").unwrap();
        let precondition = crate::PreparedPrivateFile::preview(&output).unwrap();
        assert!(precondition.destination_exists());

        let prepared = crate::PreparedPrivateFile::prepare_if_unchanged(
            precondition,
            crate::SecretBytes::new(b"approved".to_vec()),
            true,
        )
        .unwrap();
        fs::remove_file(&output).unwrap();
        fs::write(&output, b"replacement-identity").unwrap();

        let error = prepared.install().unwrap_err();
        assert_eq!(error.kind(), VaultErrorKind::AlreadyExists);
        assert!(error.message().contains("changed since preview"));
        assert_eq!(fs::read(&output).unwrap(), b"replacement-identity");
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
    }

    #[test]
    fn prepared_install_refuses_replaced_temporary_identity_for_both_policies() {
        for overwrite in [false, true] {
            let (_temp, root) = private_tempdir();
            let output = root.join("result.bin");
            if overwrite {
                fs::write(&output, b"original").unwrap();
            }
            let attacker = root.join("attacker");
            fs::write(&attacker, b"attacker-bytes").unwrap();
            let prepared = prepare_private_bytes(&output, b"protected", overwrite).unwrap();
            let temporary = prepared.path.as_ref().unwrap().temporary.clone();
            fs::remove_file(&temporary).unwrap();
            symlink(&attacker, &temporary).unwrap();

            let error = prepared.install().unwrap_err();
            assert!(error.error.to_string().contains("identity changed"));
            assert_eq!(fs::read(&attacker).unwrap(), b"attacker-bytes");
            if overwrite {
                assert_eq!(fs::read(&output).unwrap(), b"original");
            } else {
                assert!(!output.exists());
            }
            // Identity-safe cleanup deliberately leaves the replacement.
            assert!(
                fs::symlink_metadata(&temporary)
                    .unwrap()
                    .file_type()
                    .is_symlink()
            );
            fs::remove_file(temporary).unwrap();
        }
    }

    #[test]
    fn rejects_shared_writable_non_sticky_output_parent() {
        let (_temp, root) = private_tempdir();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o777)).unwrap();
        let output = root.join("result.bin");

        let error = preflight_private_destination(&output, false).unwrap_err();
        assert!(
            error
                .error
                .to_string()
                .contains("shared-writable non-sticky")
        );
        assert!(!output.exists());
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
    }

    #[test]
    fn refuses_symlinked_parent_and_leaf() {
        let (_temp, root) = private_tempdir();
        let real = root.join("real");
        fs::create_dir(&real).unwrap();
        fs::set_permissions(&real, fs::Permissions::from_mode(0o700)).unwrap();
        let linked_parent = root.join("linked");
        symlink(&real, &linked_parent).unwrap();
        let error =
            install_private_bytes(&linked_parent.join("value"), b"secret", false).unwrap_err();
        assert!(error.error.to_string().contains("symlinked parent"));
        assert!(!real.join("value").exists());

        let target = real.join("target");
        fs::write(&target, b"unchanged").unwrap();
        let leaf = real.join("leaf");
        symlink(&target, &leaf).unwrap();
        let error = install_private_bytes(&leaf, b"secret", true).unwrap_err();
        assert!(error.error.to_string().contains("symlink"));
        assert_eq!(fs::read(&target).unwrap(), b"unchanged");
    }

    #[test]
    fn overwrite_refuses_non_regular_leaf() {
        let (_temp, root) = private_tempdir();
        let output = root.join("directory");
        fs::create_dir(&output).unwrap();
        let error = install_private_bytes(&output, b"secret", true).unwrap_err();
        assert!(error.error.to_string().contains("non-regular"));
        assert!(output.is_dir());
    }
}
