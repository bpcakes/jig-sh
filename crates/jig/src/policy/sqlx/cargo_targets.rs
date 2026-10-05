//! The crate roots a repository's Cargo manifests define.
//!
//! Cargo compiles each of these as its own crate, so the file is production
//! code however else a module declaration describes it: neither its own calls
//! nor those of its descendants can be reclassified by a `#[cfg(test)]` claim
//! elsewhere. Manifests are read as source, like everything else the SQLx
//! inventory looks at; Cargo itself is never invoked.

use std::collections::{BTreeMap, BTreeSet};

use super::modules::{join_relative, parent_dir, split_path};

/// The kinds of target table, with the directory each is discovered under.
const TARGET_KINDS: [(&str, &str); 4] = [
    ("bin", "src/bin"),
    ("example", "examples"),
    ("bench", "benches"),
    ("test", "tests"),
];

#[derive(Default)]
pub(super) struct CargoTargets {
    /// Paths a manifest names, either directly or through a target's name.
    named: BTreeSet<String>,
    /// Package directories and what each compiles beyond those paths.
    packages: BTreeMap<String, Package>,
}

/// One package's automatic target discovery.
#[derive(Default)]
struct Package {
    /// Whether each kind of target is discovered at all.
    discovery: Discovery,
    /// The package's own name, which is the name of a discovered `src/main.rs`.
    name: Option<String>,
    /// Target names the manifest declares itself, by target directory. Cargo
    /// resolves each from its table instead of discovering a file for it.
    declared: BTreeMap<&'static str, BTreeSet<String>>,
}

/// Whether a package discovers each kind of target. Each kind is discovered
/// unless the package turns it off.
struct Discovery {
    library: bool,
    binaries: bool,
    examples: bool,
    benches: bool,
    tests: bool,
    /// A build script is discovered at `build.rs` unless `package.build`
    /// disables it or names a different file.
    build: bool,
}

impl CargoTargets {
    pub(super) fn add_manifest(&mut self, manifest: &str, value: &toml::Value) {
        let dir = parent_dir(manifest);
        let mut named = |path: Option<&str>| {
            if let Some(path) = path
                && let Some(target) = join_relative(&dir, path)
            {
                self.named.insert(target);
            }
        };
        let package = value.get("package");
        let build = package.and_then(|package| package.get("build"));
        named(build.and_then(toml::Value::as_str));
        // A target table without a path is resolved from the conventional
        // locations for its name. Both shapes are recorded: only one exists.
        if let Some(library) = value.get("lib") {
            named(configured_path(library).or(Some("src/lib.rs")));
        }
        let package_name = package
            .and_then(|package| package.get("name"))
            .and_then(toml::Value::as_str);
        let mut declared: BTreeMap<&'static str, BTreeSet<String>> = BTreeMap::new();
        for (kind, directory) in TARGET_KINDS {
            let tables = value.get(kind).and_then(toml::Value::as_array);
            for table in tables.into_iter().flatten() {
                let path = configured_path(table);
                let name = table.get("name").and_then(toml::Value::as_str);
                if let Some(name) = name {
                    declared.entry(directory).or_default().insert(name.into());
                }
                if let Some(path) = path {
                    named(Some(path));
                    continue;
                }
                let Some(name) = name else { continue };
                named(Some(&format!("{directory}/{name}.rs")));
                named(Some(&format!("{directory}/{name}/main.rs")));
                // A binary carrying the package's own name is also resolved
                // from the package's default entrypoint.
                if kind == "bin" && package_name == Some(name) {
                    named(Some("src/main.rs"));
                }
            }
        }
        if package.is_some() {
            self.packages.insert(
                dir,
                Package {
                    discovery: Discovery::read(value, build),
                    name: package_name.map(String::from),
                    declared,
                },
            );
        }
    }

    pub(super) fn contains(&self, path: &str) -> bool {
        self.named.contains(path) || self.discovers(path)
    }

    /// Whether a file sits where Cargo discovers a target of the package that
    /// governs it, which is the nearest ancestor directory holding a manifest
    /// that defines a package.
    fn discovers(&self, path: &str) -> bool {
        let mut dir = split_path(path).0;
        loop {
            if let Some(package) = self.packages.get(dir) {
                let relative = path.strip_prefix(dir).unwrap_or(path);
                return package.discovers(relative.trim_start_matches('/'));
            }
            if dir.is_empty() {
                return false;
            }
            dir = split_path(dir).0;
        }
    }
}

impl Package {
    /// The package-relative locations this package discovers a target at.
    fn discovers(&self, relative: &str) -> bool {
        match relative {
            "src/lib.rs" => return self.discovery.library,
            // The default entrypoint is a binary named after the package.
            "src/main.rs" => {
                return self.discovery.binaries
                    && !self.declares("src/bin", self.name.as_deref().unwrap_or_default());
            }
            "build.rs" => return self.discovery.build,
            _ => {}
        }
        let Some((parent, basename)) = relative.rsplit_once('/') else {
            return false;
        };
        // `src/bin/tool.rs` and `src/bin/tool/main.rs`, and the same two
        // shapes under `benches`, `examples` and `tests`. A target directly in
        // one of those directories counts whatever it is named, including
        // `main.rs`, before the nested shape is considered.
        let direct = basename.strip_suffix(".rs").unwrap_or(basename);
        self.discovers_named(parent, direct)
            || (basename == "main.rs" && {
                let (directory, name) = split_path(parent);
                self.discovers_named(directory, name)
            })
    }

    /// Whether this package discovers a target of the given name under one of
    /// its target directories. A name its manifest declares is resolved from
    /// that table instead, so no file is discovered for it.
    fn discovers_named(&self, directory: &str, name: &str) -> bool {
        let enabled = match directory {
            "src/bin" => self.discovery.binaries,
            "examples" => self.discovery.examples,
            "benches" => self.discovery.benches,
            "tests" => self.discovery.tests,
            _ => false,
        };
        enabled && !self.declares(directory, name)
    }

    fn declares(&self, directory: &str, name: &str) -> bool {
        self.declared
            .get(directory)
            .is_some_and(|names| names.contains(name))
    }
}

impl Default for Discovery {
    fn default() -> Self {
        Self {
            library: true,
            binaries: true,
            examples: true,
            benches: true,
            tests: true,
            build: true,
        }
    }
}

impl Discovery {
    fn read(value: &toml::Value, build: Option<&toml::Value>) -> Self {
        let package = value.get("package");
        let setting = |key: &str| {
            package
                .and_then(|package| package.get(key))
                .and_then(toml::Value::as_bool)
        };
        // A 2015-edition package stops discovering a kind of target once it
        // declares one manually. An edition the manifest inherits from its
        // workspace cannot be read here, and inheritance postdates that
        // edition by years, so an edition that is present but not a plain
        // string counts as a later one: keeping discovery on is what keeps
        // production call sites visible.
        let legacy_edition = match package.and_then(|package| package.get("edition")) {
            None => true,
            Some(edition) => edition.as_str() == Some("2015"),
        };
        let discovers = |key: &str, kind: &str| {
            setting(key).unwrap_or(!(legacy_edition && value.get(kind).is_some()))
        };
        Self {
            // An explicit `[lib]` table defines the library target, including
            // the path it is read from, so `src/lib.rs` is only the default
            // when no such table replaces it.
            library: value.get("lib").is_none() && setting("autolib").unwrap_or(true),
            binaries: discovers("autobins", "bin"),
            examples: discovers("autoexamples", "example"),
            benches: discovers("autobenches", "bench"),
            tests: discovers("autotests", "test"),
            // `build = true` asks for the default build script, a string names
            // a different file, and `false` has none.
            build: build.is_none_or(|build| build.as_bool() == Some(true)),
        }
    }
}

fn configured_path(table: &toml::Value) -> Option<&str> {
    table.get("path").and_then(toml::Value::as_str)
}
