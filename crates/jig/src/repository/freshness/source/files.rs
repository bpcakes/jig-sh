use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::Path;

use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, Metadata, OpenOptions, ReadDir};
use jig_contract::freshness::FreshnessReasonCode;
use sha2::{Digest, Sha256};

use super::super::{CollectionBudget, CollectionFailure, CollectionResult};
use super::{InputPatterns, SourceProblem};

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
}

impl DirectoryAuthority {
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
            .is_some_and(|previous| *previous != observed)
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
            if signature(&metadata) != *expected {
                return Err(raced(path));
            }
        }
        Ok(())
    }
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
        while let Some(parent) = stack.last_mut() {
            budget.ensure_active()?;
            let Some(entry) = parent.entries.next() else {
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
            if source_excluded(&path) || !patterns.intersects(&path) {
                continue;
            }
            let metadata = parent
                .directory
                .symlink_metadata(name)
                .map_err(|_| failed(&path))?;
            if metadata.file_type().is_symlink() || gitlinks.contains(&path) {
                result.problems.push(SourceProblem::unobservable(
                    &path,
                    true,
                    "a relevant symlink or submodule cannot prove scoped contents",
                ));
                continue;
            }
            if ignored_path(&path, ignored)
                && !(metadata.is_file() && observable_dotenv(&path, ignored))
            {
                result.problems.push(SourceProblem::unobservable(
                    &path,
                    metadata.is_dir(),
                    "required input is ignored and unobservable",
                ));
                continue;
            }
            if metadata.is_dir() {
                if patterns.matches(&path) {
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
            } else if metadata.is_file() && patterns.matches(&path) {
                let mut file = entry
                    .open_with(&read_options(false))
                    .map_err(|_| failed(&path))?;
                let before = signature(&metadata);
                if before != signature(&file.metadata().map_err(|_| failed(&path))?) {
                    return Err(raced(&path));
                }
                let digest = hash_file(&mut file, &metadata, budget, &path, &mut hash_buffer)?;
                // hash_file rechecks the open file's identity after streaming;
                // revalidate checks its name and all parent identities after the
                // complete Git observation. No duplicate per-file parent reopen.
                result.observed.push((path.clone(), before));
                result.entries.insert(
                    path,
                    CurrentEntry {
                        kind: "regular",
                        mode: executable_mode(&metadata),
                        digest: Some(digest),
                    },
                );
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
            if signature(&current) != *expected {
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
    loop {
        budget.ensure_active()?;
        let remaining = budget
            .limits
            .bytes
            .saturating_sub(budget.stats.content_bytes_read);
        let capacity = buffer.len().min(remaining.saturating_add(1) as usize);
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
    let after = file.metadata().map_err(|_| failed(path))?;
    if length != before.len() || signature(before) != signature(&after) {
        return Err(raced(path));
    }
    Ok(format!("sha256:{:x}", hash.finalize()))
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
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn concurrent_writer_between_file_chunks_cannot_publish_a_projection() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("example.txt");
        std::fs::write(&path, vec![b'x'; 1024 * 1024]).unwrap();
        let (start_tx, start_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let writer = std::thread::spawn(move || {
            start_rx.recv().unwrap();
            std::fs::write(path, b"changed during streaming").unwrap();
            done_tx.send(()).unwrap();
        });
        let calls = AtomicUsize::new(0);
        let cancelled = || {
            // Calls 3 and 4 bracket the first content read; pause before the
            // second chunk until a separate writer has changed the open file.
            if calls.fetch_add(1, Ordering::SeqCst) == 4 {
                start_tx.send(()).unwrap();
                done_rx.recv_timeout(Duration::from_secs(10)).unwrap();
            }
            false
        };
        let mut budget = CollectionBudget::new(
            super::super::super::CollectionLimits::with_timeout(Duration::from_secs(30)),
            &cancelled,
        );
        let patterns = InputPatterns {
            patterns: vec![super::super::InputPattern {
                text: "example.txt".into(),
                matcher: globset::Glob::new("example.txt").unwrap().compile_matcher(),
                prefix: "example.txt".into(),
                literal: true,
                max_depth: Some(1),
            }],
        };
        let root = open_root(temp.path()).unwrap();
        let result = FileProjection::capture(
            &root,
            &patterns,
            &BTreeSet::new(),
            &BTreeSet::new(),
            &mut budget,
        );
        writer.join().unwrap();
        assert_eq!(
            result.err().unwrap().reason.code,
            FreshnessReasonCode::SourceRaced
        );
        assert!(budget.stats.content_bytes_read >= 64 * 1024);
    }
}
