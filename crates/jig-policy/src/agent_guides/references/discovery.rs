use std::collections::BTreeSet;
use std::io;
use std::path::Path;

use cap_fs_ext::DirExt;
use cap_std::fs::Dir;

use super::GuideFiles;

#[derive(Default)]
pub struct GuideDiscovery {
    pub guides: BTreeSet<String>,
    pub errors: Vec<(String, io::Error)>,
}

impl GuideDiscovery {
    fn capture<T>(&mut self, path: &Path, result: io::Result<T>) -> Option<T> {
        match result {
            Ok(value) => Some(value),
            Err(error) => {
                // Optional entries can disappear at any point during discovery.
                if error.kind() != io::ErrorKind::NotFound {
                    let path = if path.as_os_str().is_empty() {
                        ".".into()
                    } else {
                        path.to_string_lossy()
                            .replace(std::path::MAIN_SEPARATOR, "/")
                    };
                    self.errors.push((path, error));
                }
                None
            }
        }
    }

    fn collect(&mut self, directory: &Dir, parent: &Path) {
        let Some(entries) = self.capture(parent, directory.entries()) else {
            return;
        };
        for entry in entries {
            let Some(entry) = self.capture(parent, entry) else {
                continue;
            };
            let name = entry.file_name();
            let relative = parent.join(&name);
            if relative
                .components()
                .any(super::super::is_ignored_guide_component)
            {
                continue;
            }
            if name == "AGENTS.md" {
                let path = relative.to_str().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "guide path must be valid UTF-8")
                });
                if let Some(path) = self.capture(&relative, path) {
                    self.guides
                        .insert(path.replace(std::path::MAIN_SEPARATOR, "/"));
                }
            }
            let Some(kind) = self.capture(&relative, entry.file_type()) else {
                continue;
            };
            if kind.is_dir()
                && let Some(child) = self.capture(&relative, directory.open_dir_nofollow(&name))
            {
                // A nested repository's guides belong to that repository.
                if is_nested_repository(&child) {
                    continue;
                }
                self.collect(&child, &relative);
            }
        }
    }
}

/// A directory with a real `.git` entry: a gitdir with `HEAD`, or the file a
/// worktree or submodule keeps there. A bare `.git` directory, such as a
/// fixture, is not a repository.
fn is_nested_repository(directory: &Dir) -> bool {
    match directory.symlink_metadata(".git") {
        Ok(metadata) if metadata.is_dir() => directory.symlink_metadata(".git/HEAD").is_ok(),
        Ok(metadata) => metadata.is_file(),
        Err(_) => false,
    }
}

impl GuideFiles {
    /// Existing guides are independent of Git ignore rules. Directory handles
    /// keep discovery beneath the same pinned root used for guide reads, and
    /// discovery does not descend into nested repositories: their guides
    /// belong to them.
    pub fn discover(&self) -> GuideDiscovery {
        let mut discovery = GuideDiscovery::default();
        discovery.collect(&self.root, Path::new(""));
        discovery
    }
}
