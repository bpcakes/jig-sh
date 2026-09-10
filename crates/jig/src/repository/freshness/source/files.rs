use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::Path;

use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, Metadata, OpenOptions, ReadDir};
use jig_contract::freshness::FreshnessReasonCode;
use sha2::{Digest, Sha256};

use super::super::{CollectionBudget, CollectionFailure, CollectionResult};
use super::{InputPatterns, SourceProblem};

mod reads;
use reads::PendingReads;
mod native;
pub(crate) use native::read_native_authority_bytes;

#[derive(Clone, Debug)]
pub(super) struct CurrentEntry {
    pub(super) kind: &'static str,
    pub(super) mode: u64,
    pub(super) digest: Option<String>,
}

pub(super) struct FileProjection {
    pub(super) entries: BTreeMap<String, CurrentEntry>,
    pub(super) problems: Vec<SourceProblem>,
    observed: Vec<(String, Signature)>,
}

struct Frame {
    path: String,
    directory: Dir,
    entries: ReadDir,
    before: Signature,
}

/// Cwd paths may be outside the declared source tree, including ignored trees.
/// Observe each component through a directory capability and retain its type
/// and identity for the same final revalidation as source contents.
#[derive(Default)]
pub(super) struct DirectoryAuthority {
    observed: BTreeMap<String, Signature>,
    identity_only: bool,
}

impl DirectoryAuthority {
    pub(super) fn for_execution() -> Self {
        Self {
            observed: BTreeMap::new(),
            identity_only: true,
        }
    }

    fn matches(&self, left: &Signature, right: &Signature) -> bool {
        if self.identity_only {
            left.0[..3] == right.0[..3]
        } else {
            left == right
        }
    }

    pub(super) fn observe(
        &mut self,
        root: &Dir,
        path: &str,
        budget: &mut CollectionBudget<'_>,
    ) -> CollectionResult<()> {
        budget.entries(1)?;
        self.remember(
            "",
            signature(&root.dir_metadata().map_err(|_| failed(path))?),
        )?;
        if path == "." {
            return Ok(());
        }
        let mut current = root.try_clone().map_err(|_| failed(path))?;
        let mut relative = String::new();
        for (depth, component) in path.split('/').enumerate() {
            budget.entries(1)?;
            if depth + 1 >= budget.limits.directory_depth {
                return Err(CollectionFailure::new(
                    FreshnessReasonCode::CollectionLimit,
                    "working-directory authority exceeded the directory-depth limit",
                )
                .at(path));
            }
            if !relative.is_empty() {
                relative.push('/');
            }
            relative.push_str(component);
            let metadata = current
                .symlink_metadata(component)
                .map_err(|_| failed(&relative))?;
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                return Err(CollectionFailure::new(
                    FreshnessReasonCode::UnobservableInput,
                    "working directory traverses a symlink or non-directory",
                )
                .at(&relative));
            }
            let file = current
                .open_with(component, &read_options(true))
                .map_err(|_| raced(&relative))?;
            let before = signature(&metadata);
            if before != signature(&file.metadata().map_err(|_| raced(&relative))?) {
                return Err(raced(&relative));
            }
            self.remember(&relative, before)?;
            current = Dir::from_std_file(file.into_std());
        }
        Ok(())
    }

    fn remember(&mut self, path: &str, observed: Signature) -> CollectionResult<()> {
        if self
            .observed
            .get(path)
            .is_some_and(|previous| !self.matches(previous, &observed))
        {
            return Err(raced(path));
        }
        self.observed.insert(path.to_owned(), observed);
        Ok(())
    }

    pub(super) fn revalidate(
        &self,
        root: &Dir,
        budget: &mut CollectionBudget<'_>,
    ) -> CollectionResult<()> {
        self.revalidate_with_identity_policy(root, budget, false)
    }

    pub(super) fn revalidate_identity(
        &self,
        root: &Dir,
        budget: &mut CollectionBudget<'_>,
    ) -> CollectionResult<()> {
        self.revalidate_with_identity_policy(root, budget, true)
    }

    fn revalidate_with_identity_policy(
        &self,
        root: &Dir,
        budget: &mut CollectionBudget<'_>,
        identity_only: bool,
    ) -> CollectionResult<()> {
        // Check parents after their children, so moving or replacing an ignored
        // ancestor cannot escape the final path/type comparison.
        for (path, expected) in self.observed.iter().rev() {
            budget.entries(1)?;
            let metadata = if path.is_empty() {
                root.dir_metadata()
            } else {
                root.symlink_metadata(path)
            }
            .map_err(|_| raced(path))?;
            let current = signature(&metadata);
            if if identity_only {
                current.0[..3] != expected.0[..3]
            } else {
                !self.matches(&current, expected)
            } {
                return Err(raced(path));
            }
        }
        Ok(())
    }
}

#[derive(Default)]
pub(super) struct RunnerFileAuthority {
    observed: BTreeMap<String, Option<Signature>>,
}

impl RunnerFileAuthority {
    pub(super) fn observe(
        &mut self,
        root: &Dir,
        path: &str,
        projection: Option<&FileProjection>,
        budget: &mut CollectionBudget<'_>,
    ) -> CollectionResult<()> {
        budget.entries(1)?;
        let current = optional_signature(root, path)?;
        let expected = projection.and_then(|projection| {
            projection
                .observed
                .iter()
                .find(|(observed, _)| observed == path)
                .map(|(_, signature)| signature)
        });
        if current.as_ref() != expected {
            return Err(raced(path));
        }
        self.observed.insert(path.into(), current);
        Ok(())
    }

    pub(super) fn revalidate(
        &self,
        root: &Dir,
        budget: &mut CollectionBudget<'_>,
    ) -> CollectionResult<()> {
        for (path, expected) in &self.observed {
            budget.entries(1)?;
            if &optional_signature(root, path)? != expected {
                return Err(raced(path));
            }
        }
        Ok(())
    }
}

fn optional_signature(root: &Dir, path: &str) -> CollectionResult<Option<Signature>> {
    match root.symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
            Ok(Some(signature(&metadata)))
        }
        Ok(_) => Err(raced(path)),
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) =>
        {
            Ok(None)
        }
        Err(_) => Err(raced(path)),
    }
}

pub(super) fn same_directory(root: &Dir, current: &Dir) -> CollectionResult<()> {
    let before = signature(&root.dir_metadata().map_err(|_| raced("."))?);
    let after = signature(&current.dir_metadata().map_err(|_| raced("."))?);
    if before.0[..3] != after.0[..3] {
        return Err(raced("."));
    }
    Ok(())
}

impl FileProjection {
    pub(super) fn capture(
        root: &Dir,
        patterns: &InputPatterns,
        ignored: &BTreeSet<String>,
        gitlinks: &BTreeSet<String>,
        budget: &mut CollectionBudget<'_>,
    ) -> CollectionResult<Self> {
        let mut result = Self {
            entries: BTreeMap::new(),
            problems: Vec::new(),
            observed: Vec::new(),
        };
        let mut stack = vec![frame(
            String::new(),
            root.try_clone().map_err(|_| failed(""))?,
        )?];
        let mut hash_buffer = vec![0_u8; 64 * 1024].into_boxed_slice();
        let mut pending = PendingReads::default();
        while let Some(parent) = stack.last_mut() {
            budget.ensure_active()?;
            let Some(entry) = parent.entries.next() else {
                pending.finish(&mut result, budget, &mut hash_buffer)?;
                let parent = stack.pop().expect("directory frame exists");
                let after = signature(
                    &parent
                        .directory
                        .dir_metadata()
                        .map_err(|_| failed(&parent.path))?,
                );
                if parent.before != after {
                    return Err(raced(&parent.path));
                }
                result.observed.push((parent.path, after));
                continue;
            };
            budget.entries(1)?;
            let entry = entry.map_err(|_| failed(&parent.path))?;
            let name_os = entry.file_name();
            let mut raw_path = parent.path.as_bytes().to_vec();
            if !raw_path.is_empty() {
                raw_path.push(b'/');
            }
            raw_path.extend_from_slice(name_os.as_encoded_bytes());
            let path = match super::git::path(&raw_path) {
                Ok(path) => path,
                Err(error) if error.reason.code == FreshnessReasonCode::UnobservableInput => {
                    result
                        .problems
                        .push(SourceProblem::unsupported_path(&raw_path));
                    continue;
                }
                Err(error) => return Err(error),
            };
            let name = name_os.to_str().expect("validated source name is UTF-8");
            if name == ".git" && !parent.path.is_empty() {
                result.problems.push(SourceProblem::unobservable(&parent.path, true,
                    "a relevant nested Git repository needs recursive authority, which policy v1 does not collect"));
                continue;
            }
            if source_excluded(&path) {
                continue;
            }
            let matches = patterns.matches(&path);
            if !matches && !patterns.descends(&path) {
                continue;
            }
            let gitlink = gitlinks.contains(&path);
            let is_ignored = ignored_path(&path, ignored);
            let unobservable_ignored = is_ignored && !observable_dotenv(&path, ignored);
            // For an observable regular entry, open without following links
            // and take initial authority directly from that descriptor. Its
            // metadata is checked after streaming and its name after the full
            // observation, so a separate pre-open path stat adds no proof.
            // Unknown directory-entry types retain the conservative path check.
            let opened = if matches
                && !gitlink
                && !unobservable_ignored
                && entry.file_type().is_ok_and(|kind| kind.is_file())
            {
                Some(
                    entry
                        .open_with(&read_options(false))
                        .map_err(|_| raced(&path))?,
                )
            } else {
                None
            };
            let metadata = if let Some(file) = &opened {
                file.metadata()
            } else {
                parent.directory.symlink_metadata(name)
            }
            .map_err(|_| failed(&path))?;
            if metadata.file_type().is_symlink() || gitlink {
                result.problems.push(SourceProblem::unobservable(
                    &path,
                    true,
                    "a relevant symlink or submodule cannot prove scoped contents",
                ));
                continue;
            }
            if unobservable_ignored || (!metadata.is_file() && is_ignored) {
                result.problems.push(SourceProblem::unobservable(
                    &path,
                    metadata.is_dir(),
                    "required input is ignored and unobservable",
                ));
                continue;
            }
            if metadata.is_dir() {
                if matches {
                    result.entries.insert(
                        path.clone(),
                        CurrentEntry {
                            kind: "directory",
                            mode: 0,
                            digest: None,
                        },
                    );
                }
                if patterns.descends(&path) {
                    pending.finish(&mut result, budget, &mut hash_buffer)?;
                    if stack.len() >= budget.limits.directory_depth {
                        return Err(CollectionFailure::new(
                            FreshnessReasonCode::CollectionLimit,
                            "freshness directory-depth limit was exceeded",
                        )
                        .at(&path));
                    }
                    let file = entry
                        .open_with(&read_options(true))
                        .map_err(|_| failed(&path))?;
                    if signature(&metadata)
                        != signature(&file.metadata().map_err(|_| failed(&path))?)
                    {
                        return Err(raced(&path));
                    }
                    stack.push(frame(path, Dir::from_std_file(file.into_std()))?);
                }
            } else if metadata.is_file() && matches {
                let file = match opened {
                    Some(file) => file,
                    None => {
                        let file = entry
                            .open_with(&read_options(false))
                            .map_err(|_| failed(&path))?;
                        if signature(&metadata)
                            != signature(&file.metadata().map_err(|_| failed(&path))?)
                        {
                            return Err(raced(&path));
                        }
                        file
                    }
                };
                pending.push(path, file, metadata, budget)?;
                if pending.full() {
                    pending.finish_one(&mut result, budget, &mut hash_buffer)?;
                }
            } else if !metadata.is_file() || patterns.has_declared_descendant(&path) {
                result.problems.push(SourceProblem::unobservable(
                    &path,
                    true,
                    "required input is not an observable regular file or directory",
                ));
            }
        }
        Ok(result)
    }

    pub(super) fn revalidate(
        &self,
        root: &Dir,
        budget: &mut CollectionBudget<'_>,
    ) -> CollectionResult<()> {
        self.revalidate_with_directory_contents(root, budget, true)
    }

    pub(super) fn revalidate_execution(
        &self,
        root: &Dir,
        budget: &mut CollectionBudget<'_>,
    ) -> CollectionResult<()> {
        self.revalidate_with_directory_contents(root, budget, false)
    }

    fn revalidate_with_directory_contents(
        &self,
        root: &Dir,
        budget: &mut CollectionBudget<'_>,
        directory_contents: bool,
    ) -> CollectionResult<()> {
        // Retain at most one parent capability for consecutive siblings. Every
        // entry and every traversed directory still has its signature checked;
        // the parent path itself is checked after its children in observed order.
        let mut parent: Option<(&str, Dir)> = None;
        for (path, expected) in &self.observed {
            budget.entries(1)?;
            let current = if path.is_empty() {
                root.dir_metadata()
            } else {
                let (directory, name) = path.rsplit_once('/').unwrap_or(("", path));
                if parent
                    .as_ref()
                    .is_none_or(|(cached, _)| *cached != directory)
                {
                    let handle = if directory.is_empty() {
                        root.try_clone()
                    } else {
                        root.open_dir(directory)
                    }
                    .map_err(|_| raced(path))?;
                    parent = Some((directory, handle));
                }
                parent
                    .as_ref()
                    .expect("parent capability is open")
                    .1
                    .symlink_metadata(name)
            }
            .map_err(|_| raced(path))?;
            let observed = signature(&current);
            let matches = if !directory_contents && current.is_dir() {
                observed.0[..3] == expected.0[..3]
            } else {
                observed == *expected
            };
            if !matches {
                return Err(raced(path));
            }
        }
        Ok(())
    }
}

pub(super) fn open_root(root: &Path) -> CollectionResult<Dir> {
    // The caller chooses the repository root; every subsequent read is relative
    // to this capability. No source entry supplies ambient filesystem authority.
    Dir::open_ambient_dir(root, cap_std::ambient_authority()).map_err(|_| failed(""))
}

pub(super) fn configuration(
    root: &Dir,
    budget: &mut CollectionBudget<'_>,
) -> CollectionResult<Vec<String>> {
    let mut hash_buffer = vec![0_u8; 64 * 1024].into_boxed_slice();
    [".jig.toml", ".agent/jig-contract.json"]
        .into_iter()
        .map(|path| {
            let metadata = root.symlink_metadata(path).map_err(|_| failed(path))?;
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return Err(CollectionFailure::new(
                    FreshnessReasonCode::UnobservableInput,
                    "repository configuration is not an observable regular file",
                )
                .at(path));
            }
            let mut file = root
                .open_with(path, &read_options(false))
                .map_err(|_| failed(path))?;
            if signature(&metadata) != signature(&file.metadata().map_err(|_| failed(path))?) {
                return Err(raced(path));
            }
            hash_file(&mut file, &metadata, budget, path, &mut hash_buffer)
        })
        .collect()
}

fn hash_file(
    file: &mut cap_std::fs::File,
    before: &Metadata,
    budget: &mut CollectionBudget<'_>,
    path: &str,
    buffer: &mut [u8],
) -> CollectionResult<String> {
    let mut hash = Sha256::new();
    let mut length = 0_u64;
    while length < before.len() {
        budget.ensure_active()?;
        let remaining = budget
            .limits
            .bytes
            .saturating_sub(budget.stats.content_bytes_read);
        let capacity = buffer
            .len()
            .min(remaining.saturating_add(1).min(before.len() - length) as usize);
        let count = file
            .read(&mut buffer[..capacity])
            .map_err(|_| failed(path))?;
        budget.bytes(count as u64)?;
        if count == 0 {
            break;
        }
        length = length.saturating_add(count as u64);
        hash.update(&buffer[..count]);
    }
    // The final signature verifies both length and modification identity, so
    // reading a separate EOF after the captured length adds no authority.
    budget.ensure_active()?;
    let after = file.metadata().map_err(|_| failed(path))?;
    if length != before.len() || signature(before) != signature(&after) {
        return Err(raced(path));
    }
    Ok(super::super::encoding::finish_hash(hash))
}

fn frame(path: String, directory: Dir) -> CollectionResult<Frame> {
    let before = signature(&directory.dir_metadata().map_err(|_| failed(&path))?);
    let entries = directory.entries().map_err(|_| failed(&path))?;
    Ok(Frame {
        path,
        directory,
        entries,
        before,
    })
}

fn read_options(directory: bool) -> OpenOptions {
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt;
        options.custom_flags(
            libc::O_CLOEXEC
                | libc::O_NOFOLLOW
                | libc::O_NONBLOCK
                | if directory { libc::O_DIRECTORY } else { 0 },
        );
    }
    options
}

pub(super) fn source_excluded(path: &str) -> bool {
    path.split('/').any(|part| part == ".git") || path.split('/').next() == Some(".agent")
}

pub(super) fn ignored_path(path: &str, ignored: &BTreeSet<String>) -> bool {
    ignored.contains(path)
        || path
            .match_indices('/')
            .any(|(index, _)| ignored.contains(&path[..index]))
}

pub(super) fn observable_dotenv(path: &str, ignored: &BTreeSet<String>) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    (name == ".env" || name.starts_with(".env."))
        && !path
            .match_indices('/')
            .any(|(index, _)| ignored.contains(&path[..index]))
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Signature([u64; 8]);

#[cfg(unix)]
fn signature(metadata: &Metadata) -> Signature {
    use cap_std::fs::MetadataExt;
    Signature([
        metadata.dev(),
        metadata.ino(),
        u64::from(metadata.mode()),
        metadata.len(),
        metadata.mtime() as u64,
        metadata.mtime_nsec() as u64,
        metadata.ctime() as u64,
        metadata.ctime_nsec() as u64,
    ])
}

#[cfg(not(unix))]
fn signature(metadata: &Metadata) -> Signature {
    Signature([
        metadata.len(),
        u64::from(metadata.is_file()),
        u64::from(metadata.is_dir()),
        0,
        0,
        0,
        0,
        0,
    ])
}

fn executable_mode(metadata: &Metadata) -> u64 {
    #[cfg(unix)]
    {
        use cap_std::fs::MetadataExt;
        if metadata.mode() & 0o111 != 0 {
            0o100755
        } else {
            0o100644
        }
    }
    #[cfg(not(unix))]
    {
        0o100644
    }
}

fn failed(path: &str) -> CollectionFailure {
    CollectionFailure::new(
        FreshnessReasonCode::CollectionFailed,
        "source filesystem observation failed",
    )
    .at(path)
}

fn raced(path: &str) -> CollectionFailure {
    CollectionFailure::new(
        FreshnessReasonCode::SourceRaced,
        "source identity changed while it was being observed",
    )
    .at(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::io::Seek;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn concurrent_writer_between_file_chunks_cannot_publish_a_digest() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("example.txt");
        std::fs::write(&path, vec![b'x'; 1024 * 1024]).unwrap();
        let root = open_root(temp.path()).unwrap();
        let before = root.symlink_metadata("example.txt").unwrap();
        let mut file = root.open_with("example.txt", &read_options(false)).unwrap();
        // A duplicate file handle observes the real shared read position, so
        // injecting this race does not depend on cancellation callback counts.
        let position = RefCell::new(file.try_clone().unwrap());
        let (start_tx, start_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let writer = std::thread::spawn(move || {
            start_rx.recv().unwrap();
            std::fs::write(path, b"changed during streaming").unwrap();
            done_tx.send(()).unwrap();
        });
        let changed = AtomicBool::new(false);
        let cancelled = || {
            if position.borrow_mut().stream_position().unwrap() >= 64 * 1024
                && !changed.swap(true, Ordering::SeqCst)
            {
                start_tx.send(()).unwrap();
                done_rx.recv_timeout(Duration::from_secs(10)).unwrap();
            }
            false
        };
        let mut budget = CollectionBudget::new(
            super::super::super::CollectionLimits::with_timeout(Duration::from_secs(30)),
            &cancelled,
        );
        let result = hash_file(
            &mut file,
            &before,
            &mut budget,
            "example.txt",
            &mut vec![0; 64 * 1024].into_boxed_slice(),
        );
        writer.join().unwrap();
        assert_eq!(
            result.err().unwrap().reason.code,
            FreshnessReasonCode::SourceRaced
        );
        assert!(budget.stats.content_bytes_read >= 64 * 1024);
    }
}
