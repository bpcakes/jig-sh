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
                self.collect(&child, &relative);
            }
        }
    }
}

impl GuideFiles {
    /// Existing guides are independent of Git ignore rules. Directory handles
    /// keep discovery beneath the same pinned root used for guide reads.
    pub fn discover(&self) -> GuideDiscovery {
        let mut discovery = GuideDiscovery::default();
        discovery.collect(&self.root, Path::new(""));
        discovery
    }
}
