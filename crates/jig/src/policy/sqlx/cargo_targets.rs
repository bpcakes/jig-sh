//! The crate roots a repository's Cargo manifests define.
//!
//! Cargo compiles each of these as its own crate, so the file is production
//! code however else a module declaration describes it: neither its own calls
//! nor those of its descendants can be reclassified by a `#[cfg(test)]` claim
//! elsewhere. Manifests are read as source, like everything else the SQLx
//! inventory looks at; Cargo itself is never invoked.

use std::collections::BTreeSet;

use super::modules::{join_relative, parent_dir, split_path};

/// The crate roots Cargo compiles. Each is production code however else a
/// module declaration describes it, so neither its own calls nor those of its
/// descendants can be reclassified by a `#[cfg(test)]` claim elsewhere.
#[derive(Default)]
pub(super) struct CargoTargets {
    /// Paths a manifest configures explicitly.
    configured: BTreeSet<String>,
    /// Directories of manifests that define a package, which is where Cargo
    /// discovers the conventional target locations.
    packages: BTreeSet<String>,
}

impl CargoTargets {
    /// Reads one manifest's target paths, resolved relative to the manifest.
    pub(super) fn add_manifest(&mut self, manifest: &str, value: &toml::Value) {
        let dir = parent_dir(manifest);
        let mut record = |configured: Option<&toml::Value>| {
            if let Some(path) = configured.and_then(toml::Value::as_str)
                && let Some(target) = join_relative(&dir, path)
            {
                self.configured.insert(target);
            }
        };
        let package = value.get("package");
        record(package.and_then(|package| package.get("build")));
        record(value.get("lib").and_then(|library| library.get("path")));
        for kind in ["bin", "example", "bench", "test"] {
            let configured = value.get(kind).and_then(toml::Value::as_array);
            for target in configured.into_iter().flatten() {
                record(target.get("path"));
            }
        }
        if package.is_some() {
            self.packages.insert(dir);
        }
    }

    pub(super) fn contains(&self, path: &str) -> bool {
        self.configured.contains(path) || self.discovers(path)
    }

    /// Whether a file sits where Cargo discovers a target of the package that
    /// governs it, which is the nearest ancestor directory holding a manifest
    /// that defines a package.
    fn discovers(&self, path: &str) -> bool {
        let mut dir = split_path(path).0;
        loop {
            if self.packages.contains(dir) {
                let relative = path.strip_prefix(dir).unwrap_or(path);
                return is_discovered_target(relative.trim_start_matches('/'));
            }
            if dir.is_empty() {
                return false;
            }
            dir = split_path(dir).0;
        }
    }
}

/// The package-relative locations Cargo discovers a target at. A build script
/// counts whether or not `package.build` names it, since treating one as a
/// crate root can only keep production calls visible.
fn is_discovered_target(relative: &str) -> bool {
    if matches!(relative, "src/lib.rs" | "src/main.rs" | "build.rs") {
        return true;
    }
    let Some((parent, basename)) = relative.rsplit_once('/') else {
        return false;
    };
    // `src/bin/tool.rs` and `src/bin/tool/main.rs`, and the same two shapes
    // under `benches`, `examples` and `tests`. A target directly in one of
    // those directories counts whatever it is named, including `main.rs`,
    // before the nested shape is considered.
    is_target_directory(parent)
        || (basename == "main.rs" && is_target_directory(split_path(parent).0))
}

fn is_target_directory(directory: &str) -> bool {
    matches!(directory, "src/bin" | "benches" | "examples" | "tests")
}
