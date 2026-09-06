use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use fs4::fs_std::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ulid::Ulid;

use super::file_budget_lifecycle::{
    LEGACY_CHECKER_PATH, LifecycleProof, revalidate_lifecycle_proof,
};
use super::git::git_stdout;
use super::path::{self, RepositoryFileLeaf};
use super::staged_render::StagedRender;

mod filesystem;
use filesystem::*;

const JOURNAL_VERSION: u32 = 1;
const JIG_METADATA_DIRECTORY: &str = "jig";
const METADATA_DIRECTORY: &str = "repository-update";
const LOCK_FILE: &str = "update.lock";
const JOURNAL_DIRECTORY: &str = "transaction-v1";
const STATE_FILE: &str = "state";
const MANIFEST_FILE: &str = "manifest.json";
const PROGRESS_FILE: &str = "progress.jsonl";
const MAX_OPERATIONS: usize = 2_048;
const MAX_MANIFEST_BYTES: u64 = 4 * 1024 * 1024;
const MAX_ENTRY_BYTES: u64 = 64 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum TransactionKind {
    Update,
    Recopy,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum EntryKind {
    Missing,
    Regular,
    Symlink,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredEntry {
    kind: EntryKind,
    sha256: Option<String>,
    length: u64,
    mode: u32,
    target_is_directory: bool,
    payload: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct UpdateOperation {
    index: usize,
    path: String,
    before: StoredEntry,
    after: StoredEntry,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct UpdateManifest {
    version: u32,
    transaction_id: String,
    kind: TransactionKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    lifecycle_proof: Option<LifecycleProof>,
    operations: Vec<UpdateOperation>,
}

#[derive(Serialize)]
struct ProgressRecord<'a> {
    version: u32,
    transaction_id: &'a str,
    completed_operation: usize,
    path: &'a str,
}

pub(super) struct RepositoryUpdateLock {
    destination: PathBuf,
    metadata_root: PathBuf,
    journal_root: PathBuf,
    _guard: File,
}

pub(super) struct RepositoryUpdateTransaction<'a> {
    lock: &'a RepositoryUpdateLock,
    destination: PathBuf,
    manifest: UpdateManifest,
    operations_by_path: BTreeMap<PathBuf, usize>,
    committed: bool,
}

impl RepositoryUpdateLock {
    pub(super) fn acquire(destination: &Path) -> Result<Self> {
        let worktree = git_stdout(destination, ["rev-parse", "--show-toplevel"])
            .context("Full update requires a usable Git worktree")?;
        let destination = PathBuf::from(worktree);
        let reported = git_stdout(
            destination.as_path(),
            ["rev-parse", "--git-path", JIG_METADATA_DIRECTORY],
        )
        .context("Full update requires a usable Git worktree metadata path")?;
        if reported.is_empty() {
            bail!("Git returned an empty repository-update metadata path");
        }
        let reported = PathBuf::from(reported);
        let metadata_root = if reported.is_absolute() {
            reported
        } else {
            destination.join(reported)
        };
        prepare_private_metadata_directory(&metadata_root)?;
        let metadata_root = metadata_root.join(METADATA_DIRECTORY);
        prepare_private_metadata_directory(&metadata_root)?;
        let lock_path = metadata_root.join(LOCK_FILE);
        reject_symlink(&lock_path, true)?;
        let guard = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)
            .with_context(|| format!("Failed to open update lock {}", lock_path.display()))?;
        set_private_file_permissions(&guard)?;
        FileExt::lock_exclusive(&guard)
            .with_context(|| format!("Failed to lock update state {}", lock_path.display()))?;
        let lock = Self {
            destination,
            journal_root: metadata_root.join(JOURNAL_DIRECTORY),
            metadata_root,
            _guard: guard,
        };
        lock.recover()?;
        Ok(lock)
    }

    pub(super) fn recover(&self) -> Result<()> {
        let metadata = match fs::symlink_metadata(&self.journal_root) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "Failed to inspect update recovery journal {}",
                        self.journal_root.display()
                    )
                });
            }
        };
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            bail!(
                "Update recovery journal is not a private real directory: {}",
                self.journal_root.display()
            );
        }
        let state = read_state(&self.journal_root)?;
        if state == "Committed" {
            cleanup_journal(&self.metadata_root, &self.journal_root)?;
            return Ok(());
        }
        if state == "Preparing" && !self.journal_root.join(MANIFEST_FILE).exists() {
            cleanup_journal(&self.metadata_root, &self.journal_root)?;
            return Ok(());
        }
        let manifest = read_manifest(&self.journal_root)?;
        let destination_text = fs::read_to_string(self.journal_root.join("destination")).context(
            "Update recovery journal is missing its worktree-relative destination marker",
        )?;
        if destination_text.trim() != "." {
            bail!("Update recovery journal has an invalid destination marker");
        }
        rollback_manifest(&self.journal_root, &self.destination, &manifest).map_err(|error| {
            anyhow::anyhow!(
                "Incomplete update recovery is preserved at {}. Restore the named preimages manually or restore the foreign paths, then retry the update. Cause: {error:#}",
                self.journal_root.display()
            )
        })?;
        cleanup_journal(&self.metadata_root, &self.journal_root)
    }
}

impl<'a> RepositoryUpdateTransaction<'a> {
    pub(super) fn prepare(
        lock: &'a RepositoryUpdateLock,
        destination: &Path,
        staged: &StagedRender,
        recopy: bool,
        lifecycle_proof: Option<LifecycleProof>,
    ) -> Result<Self> {
        match Self::prepare_inner(lock, destination, staged, recopy, lifecycle_proof) {
            Ok(transaction) => Ok(transaction),
            Err(primary) if !lock.journal_root.exists() => Err(primary),
            Err(primary) => match cleanup_journal(&lock.metadata_root, &lock.journal_root) {
                Ok(()) => Err(primary),
                Err(cleanup) => Err(anyhow::anyhow!(
                    "{primary:#}\nAdditionally, the unused update preparation journal could not be removed: {cleanup:#}\nJournal: {}",
                    lock.journal_root.display()
                )),
            },
        }
    }

    fn prepare_inner(
        lock: &'a RepositoryUpdateLock,
        destination: &Path,
        staged: &StagedRender,
        recopy: bool,
        lifecycle_proof: Option<LifecycleProof>,
    ) -> Result<Self> {
        if lock.journal_root.exists() {
            bail!(
                "Update recovery journal still exists after recovery: {}",
                lock.journal_root.display()
            );
        }
        create_private_directory(&lock.journal_root)?;
        write_synced(lock.journal_root.join("destination"), b".\n")?;
        write_state(&lock.journal_root, "Preparing")?;
        create_private_directory(&lock.journal_root.join("preimages"))?;
        create_private_directory(&lock.journal_root.join("staged"))?;

        let paths = transaction_paths(staged, destination)?;
        if paths.len() > MAX_OPERATIONS {
            bail!(
                "Full update plans {} file operations, exceeding the journal limit of {MAX_OPERATIONS}",
                paths.len()
            );
        }
        let transaction_id = Ulid::new().to_string();
        let mut total_bytes = 0_u64;
        let mut operations = Vec::with_capacity(paths.len());
        for (index, relative) in paths.into_iter().enumerate() {
            validate_relative_path(&relative)?;
            let before = capture_entry(
                destination,
                &relative,
                &lock.journal_root,
                "preimages",
                index,
                &mut total_bytes,
            )?;
            let after_root = &staged.destination;
            let after = capture_entry(
                after_root,
                &relative,
                &lock.journal_root,
                "staged",
                index,
                &mut total_bytes,
            )?;
            operations.push(UpdateOperation {
                index,
                path: relative_path_text(&relative)?,
                before,
                after,
            });
        }
        let manifest = UpdateManifest {
            version: JOURNAL_VERSION,
            transaction_id,
            kind: if recopy {
                TransactionKind::Recopy
            } else {
                TransactionKind::Update
            },
            lifecycle_proof,
            operations,
        };
        let mut bytes = serde_json::to_vec_pretty(&manifest)
            .context("Failed to serialize repository update transaction")?;
        if bytes.len() as u64 > MAX_MANIFEST_BYTES {
            bail!("Repository update transaction manifest exceeds {MAX_MANIFEST_BYTES} bytes");
        }
        bytes.push(b'\n');
        write_synced(lock.journal_root.join(MANIFEST_FILE), &bytes)?;
        sync_directory(&lock.journal_root.join("preimages"))?;
        sync_directory(&lock.journal_root.join("staged"))?;
        sync_directory(&lock.journal_root)?;
        write_state(&lock.journal_root, "Prepared")?;
        maybe_fail("after_prepare")?;
        let operations_by_path = manifest
            .operations
            .iter()
            .map(|operation| (PathBuf::from(&operation.path), operation.index))
            .collect();
        Ok(Self {
            lock,
            destination: destination.to_path_buf(),
            manifest,
            operations_by_path,
            committed: false,
        })
    }

    pub(super) fn apply_path(&mut self, relative: &Path) -> Result<()> {
        let Some(index) = self.operations_by_path.get(relative).copied() else {
            bail!(
                "Repository update attempted an unjournaled path: {}",
                relative.display()
            );
        };
        let operation = self
            .manifest
            .operations
            .get(index)
            .context("Repository update operation index is invalid")?;
        if relative == Path::new(LEGACY_CHECKER_PATH)
            && operation.after.kind == EntryKind::Missing
            && let Some(proof) = self.manifest.lifecycle_proof.as_ref()
        {
            revalidate_lifecycle_proof(&self.destination, proof)?;
        }
        maybe_fail(&format!("before_operation_{index}"))?;
        require_entry_matches(&self.destination, relative, &operation.before).with_context(
            || {
                format!(
                    "Repository path changed after update preparation; refusing to overwrite {}",
                    relative.display()
                )
            },
        )?;
        install_entry(
            &self.lock.journal_root,
            &self.destination,
            relative,
            &operation.before,
            &operation.after,
        )?;
        maybe_fail(&format!("after_operation_{index}"))?;
        append_progress(&self.lock.journal_root, &self.manifest, operation)?;
        maybe_fail(&format!("after_progress_{index}"))
    }

    pub(super) fn commit(mut self) -> Result<()> {
        if let Err(error) = maybe_fail("before_committed") {
            return Err(self.finish_failed(error));
        }
        if let Err(error) = write_state(&self.lock.journal_root, "Committed") {
            return Err(self.finish_failed(error));
        }
        self.committed = true;
        maybe_fail("after_committed")?;
        cleanup_journal(&self.lock.metadata_root, &self.lock.journal_root)
    }

    pub(super) fn finish_failed(mut self, primary: anyhow::Error) -> anyhow::Error {
        match rollback_manifest(&self.lock.journal_root, &self.destination, &self.manifest)
            .and_then(|()| cleanup_journal(&self.lock.metadata_root, &self.lock.journal_root))
        {
            Ok(()) => {
                self.committed = true;
                primary
            }
            Err(recovery) => anyhow::anyhow!(
                "{primary:#}\nAdditionally, repository update rollback is incomplete: {recovery:#}\nRecovery bundle: {}",
                self.lock.journal_root.display()
            ),
        }
    }
}

impl Drop for RepositoryUpdateTransaction<'_> {
    fn drop(&mut self) {
        // Leaving the prepared journal is intentional. The next invocation
        // performs the same one-way rollback used after process death.
        let _ = self.committed;
    }
}

fn transaction_paths(staged: &StagedRender, destination: &Path) -> Result<Vec<PathBuf>> {
    let manifest = Path::new(super::managed_paths::MANIFEST_PATH);
    let mut paths = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for path in staged
        .authored_seed_paths()
        .into_iter()
        .filter(|path| !destination.join(path).exists())
        .chain(
            staged
                .active_paths
                .iter()
                .filter(|path| path.as_path() != manifest)
                .cloned(),
        )
        .chain(staged.retirement_paths.iter().cloned())
    {
        if seen.insert(path.clone()) {
            paths.push(path);
        }
    }
    if staged.active_paths.contains(manifest) {
        paths.push(manifest.to_path_buf());
    }
    Ok(paths)
}

fn capture_entry(
    root: &Path,
    relative: &Path,
    journal: &Path,
    section: &str,
    index: usize,
    total_bytes: &mut u64,
) -> Result<StoredEntry> {
    match path::validate_repository_relative_file_leaf(root, relative)? {
        RepositoryFileLeaf::Missing => Ok(missing_entry()),
        RepositoryFileLeaf::RegularFile => {
            let source = root.join(relative);
            let before_fingerprint = path::repository_file_fingerprint_at(&source)?;
            let metadata = fs::symlink_metadata(&source)
                .with_context(|| format!("Failed to inspect {}", source.display()))?;
            charge_bytes(before_fingerprint.content_length, total_bytes)?;
            let bytes = path::read_repository_regular_file_bytes(root, relative)?;
            let after_fingerprint = path::repository_file_fingerprint_at(&source)?;
            if !path::repository_file_commits_match(&before_fingerprint, &after_fingerprint)
                || bytes.len() as u64 != before_fingerprint.content_length
                || Sha256::digest(&bytes).as_slice() != before_fingerprint.content_sha256
                || path::repository_permission_identity(&metadata.permissions())
                    != before_fingerprint.permission_identity
            {
                bail!(
                    "Repository file changed while journaling: {}",
                    source.display()
                );
            }
            let payload_relative = format!("{section}/{index:04}");
            write_synced(journal.join(&payload_relative), &bytes)?;
            Ok(StoredEntry {
                kind: EntryKind::Regular,
                sha256: Some(digest(&bytes)),
                length: bytes.len() as u64,
                mode: permission_mode(&metadata),
                target_is_directory: false,
                payload: Some(payload_relative),
            })
        }
        RepositoryFileLeaf::Symlink => {
            let source = root.join(relative);
            let target = fs::read_link(&source)
                .with_context(|| format!("Failed to read symlink {}", source.display()))?;
            let bytes = os_path_bytes(&target)?;
            charge_bytes(bytes.len() as u64, total_bytes)?;
            let payload_relative = format!("{section}/{index:04}");
            create_payload_symlink(
                &journal.join(&payload_relative),
                &target,
                fs::metadata(&source).is_ok_and(|metadata| metadata.is_dir()),
            )?;
            Ok(StoredEntry {
                kind: EntryKind::Symlink,
                sha256: Some(digest(&bytes)),
                length: bytes.len() as u64,
                mode: 0,
                target_is_directory: fs::metadata(&source).is_ok_and(|metadata| metadata.is_dir()),
                payload: Some(payload_relative),
            })
        }
    }
}

fn missing_entry() -> StoredEntry {
    StoredEntry {
        kind: EntryKind::Missing,
        sha256: None,
        length: 0,
        mode: 0,
        target_is_directory: false,
        payload: None,
    }
}

fn install_entry(
    journal: &Path,
    destination: &Path,
    relative: &Path,
    before: &StoredEntry,
    after: &StoredEntry,
) -> Result<()> {
    require_payload_matches(journal, after)?;
    match after.kind {
        EntryKind::Missing => remove_entry(destination, relative),
        EntryKind::Regular => {
            let payload = payload_path(journal, after)?;
            let permissions = permissions_from_mode(after.mode);
            path::copy_repository_regular_file_atomic_with_permissions(
                destination,
                relative,
                &payload,
                permissions,
                leaf_for_entry(before),
            )?;
            require_entry_matches(destination, relative, after)
        }
        EntryKind::Symlink => {
            if before.kind != EntryKind::Missing {
                remove_entry(destination, relative)?;
            }
            let payload = payload_path(journal, after)?;
            path::copy_repository_symlink_atomic(destination, relative, &payload)?;
            require_entry_matches(destination, relative, after)
        }
    }
}

fn require_payload_matches(journal: &Path, expected: &StoredEntry) -> Result<()> {
    match expected.kind {
        EntryKind::Missing => Ok(()),
        EntryKind::Regular => {
            let payload = payload_path(journal, expected)?;
            let metadata = fs::symlink_metadata(&payload)?;
            if !metadata.file_type().is_file()
                || metadata.len() != expected.length
                || !private_payload_matches_mode(&metadata)
            {
                bail!("Repository update journal contains an invalid regular payload");
            }
            let bytes = fs::read(&payload)?;
            if expected.sha256.as_deref() != Some(digest(&bytes).as_str()) {
                bail!("Repository update journal payload digest does not match its manifest");
            }
            Ok(())
        }
        EntryKind::Symlink => {
            let payload = payload_path(journal, expected)?;
            let metadata = fs::symlink_metadata(&payload)?;
            if !metadata.file_type().is_symlink() {
                bail!("Repository update journal contains an invalid symlink payload");
            }
            let target = fs::read_link(&payload)?;
            let bytes = os_path_bytes(&target)?;
            if bytes.len() as u64 != expected.length
                || expected.sha256.as_deref() != Some(digest(&bytes).as_str())
            {
                bail!("Repository update journal symlink digest does not match its manifest");
            }
            Ok(())
        }
    }
}

fn rollback_manifest(journal: &Path, destination: &Path, manifest: &UpdateManifest) -> Result<()> {
    validate_manifest(manifest)?;
    let mut foreign = Vec::new();
    for operation in manifest.operations.iter().rev() {
        let relative = Path::new(&operation.path);
        if entry_matches(destination, relative, &operation.before)? {
            continue;
        }
        let transaction_intermediate = operation.before.kind != EntryKind::Missing
            && operation.after.kind != EntryKind::Missing
            && path::validate_repository_relative_file_leaf(destination, relative)?
                == RepositoryFileLeaf::Missing;
        if !entry_matches(destination, relative, &operation.after)? && !transaction_intermediate {
            foreign.push(operation.path.clone());
            continue;
        }
        install_entry(
            journal,
            destination,
            relative,
            &operation.after,
            &operation.before,
        )?;
    }
    if foreign.is_empty() {
        Ok(())
    } else {
        bail!(
            "Foreign writes replaced transaction destinations; preserved without overwrite: {}",
            foreign.join(", ")
        )
    }
}

fn require_entry_matches(root: &Path, relative: &Path, expected: &StoredEntry) -> Result<()> {
    if entry_matches(root, relative, expected)? {
        Ok(())
    } else {
        bail!("path does not match the journaled state")
    }
}

fn entry_matches(root: &Path, relative: &Path, expected: &StoredEntry) -> Result<bool> {
    match (
        path::validate_repository_relative_file_leaf(root, relative)?,
        expected.kind,
    ) {
        (RepositoryFileLeaf::Missing, EntryKind::Missing) => Ok(true),
        (RepositoryFileLeaf::RegularFile, EntryKind::Regular) => {
            let absolute = root.join(relative);
            let metadata = fs::symlink_metadata(&absolute)?;
            if metadata.len() != expected.length || permission_mode(&metadata) != expected.mode {
                return Ok(false);
            }
            let bytes = path::read_repository_regular_file_bytes(root, relative)?;
            Ok(bytes.len() as u64 == expected.length
                && expected.sha256.as_deref() == Some(digest(&bytes).as_str()))
        }
        (RepositoryFileLeaf::Symlink, EntryKind::Symlink) => {
            let target = fs::read_link(root.join(relative))?;
            let bytes = os_path_bytes(&target)?;
            Ok(bytes.len() as u64 == expected.length
                && expected.sha256.as_deref() == Some(digest(&bytes).as_str())
                && fs::metadata(root.join(relative)).is_ok_and(|metadata| metadata.is_dir())
                    == expected.target_is_directory)
        }
        _ => Ok(false),
    }
}

fn remove_entry(root: &Path, relative: &Path) -> Result<()> {
    match path::validate_repository_relative_file_leaf(root, relative)? {
        RepositoryFileLeaf::Missing => Ok(()),
        RepositoryFileLeaf::RegularFile | RepositoryFileLeaf::Symlink => {
            fs::remove_file(root.join(relative))?;
            sync_parent(root.join(relative).as_path())
        }
    }
}

fn append_progress(
    journal: &Path,
    manifest: &UpdateManifest,
    operation: &UpdateOperation,
) -> Result<()> {
    let record = ProgressRecord {
        version: JOURNAL_VERSION,
        transaction_id: &manifest.transaction_id,
        completed_operation: operation.index,
        path: &operation.path,
    };
    let mut bytes = serde_json::to_vec(&record)?;
    bytes.push(b'\n');
    let path = journal.join(PROGRESS_FILE);
    let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    sync_directory(journal)
}

fn read_manifest(journal: &Path) -> Result<UpdateManifest> {
    let path = journal.join(MANIFEST_FILE);
    if fs::symlink_metadata(&path)?.len() > MAX_MANIFEST_BYTES {
        bail!("Repository update transaction manifest exceeds its byte limit");
    }
    let bytes = fs::read(path)?;
    let manifest: UpdateManifest =
        serde_json::from_slice(&bytes).context("Invalid repository update transaction manifest")?;
    validate_manifest(&manifest)?;
    Ok(manifest)
}

fn validate_manifest(manifest: &UpdateManifest) -> Result<()> {
    if manifest.version != JOURNAL_VERSION || manifest.operations.len() > MAX_OPERATIONS {
        bail!("Unsupported or oversized repository update transaction manifest");
    }
    if let Some(proof) = &manifest.lifecycle_proof
        && (proof.receipt_id.is_empty()
            || proof.receipt_id.len() > 128
            || ![
                &proof.config_digest,
                &proof.input_digest,
                &proof.source_fingerprint,
                &proof.policy_raw_digest,
                &proof.evaluation_digest,
            ]
            .into_iter()
            .all(|identity| valid_sha256_identity(identity)))
    {
        bail!("Invalid lifecycle proof in repository update transaction manifest");
    }
    for (index, operation) in manifest.operations.iter().enumerate() {
        if operation.index != index {
            bail!("Repository update transaction operation order is invalid");
        }
        validate_relative_path(Path::new(&operation.path))?;
        validate_stored_entry(&operation.before, "preimages", index)?;
        validate_stored_entry(&operation.after, "staged", index)?;
    }
    Ok(())
}

fn valid_sha256_identity(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn validate_stored_entry(entry: &StoredEntry, section: &str, index: usize) -> Result<()> {
    match entry.kind {
        EntryKind::Missing => {
            if entry.payload.is_some() || entry.sha256.is_some() || entry.length != 0 {
                bail!("Invalid missing entry in repository update manifest");
            }
        }
        EntryKind::Regular | EntryKind::Symlink => {
            let expected = format!("{section}/{index:04}");
            if entry.payload.as_deref() != Some(&expected)
                || entry
                    .sha256
                    .as_deref()
                    .is_none_or(|digest| digest.len() != 64)
                || entry.length > MAX_ENTRY_BYTES
            {
                bail!("Invalid payload entry in repository update manifest");
            }
        }
    }
    Ok(())
}

fn read_state(journal: &Path) -> Result<String> {
    let state = fs::read_to_string(journal.join(STATE_FILE))?;
    let state = state.trim();
    match state {
        "Preparing" | "Prepared" | "Committed" => Ok(state.to_owned()),
        _ => bail!("Invalid repository update transaction state"),
    }
}

fn write_state(journal: &Path, state: &str) -> Result<()> {
    write_synced(
        journal.join(format!(".{STATE_FILE}.new")),
        format!("{state}\n").as_bytes(),
    )?;
    fs::rename(
        journal.join(format!(".{STATE_FILE}.new")),
        journal.join(STATE_FILE),
    )?;
    sync_directory(journal)
}

fn payload_path(journal: &Path, entry: &StoredEntry) -> Result<PathBuf> {
    let relative = entry
        .payload
        .as_deref()
        .context("Journal entry has no payload")?;
    let path = journal.join(relative);
    if !path.starts_with(journal) {
        bail!("Journal payload escapes its transaction directory");
    }
    Ok(path)
}

fn charge_bytes(bytes: u64, total: &mut u64) -> Result<()> {
    if bytes > MAX_ENTRY_BYTES {
        bail!("Repository update entry exceeds the {MAX_ENTRY_BYTES}-byte journal limit");
    }
    *total = total
        .checked_add(bytes)
        .context("Repository update journal byte total overflowed")?;
    if *total > MAX_TOTAL_BYTES {
        bail!("Repository update exceeds the {MAX_TOTAL_BYTES}-byte journal limit");
    }
    Ok(())
}

fn maybe_fail(_point: &str) -> Result<()> {
    #[cfg(test)]
    if fault_injection::matches(_point) {
        bail!("Injected repository update transaction failure at {_point}");
    }
    Ok(())
}

#[cfg(test)]
mod fault_injection;

#[cfg(test)]
mod tests;
