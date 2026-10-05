//! The crate roots a repository's Cargo manifests define.
//!
//! Cargo compiles each of these as its own crate, so the file is production
//! code however else a module declaration describes it: neither its own calls
//! nor those of its descendants can be reclassified by a `#[cfg(test)]` claim
//! elsewhere. Manifests are read as source, like everything else the SQLx
//! inventory looks at; Cargo itself is never invoked.

use std::collections::{BTreeMap, BTreeSet};

use super::modules::{join_relative, parent_dir, split_path};

/// The directory each kind of target table is discovered and named under.
const TARGET_DIRECTORIES: [(&str, &str); 4] = [
    ("bin", "src/bin"),
    ("example", "examples"),
    ("bench", "benches"),
    ("test", "tests"),
];

#[derive(Default)]
pub(super) struct CargoTargets {
    /// Paths a manifest names, either directly or through a target's name.
    named: BTreeSet<String>,
    /// Package directories and what each discovers automatically.
    packages: BTreeMap<String, Discovery>,
}

/// What a manifest says about Cargo's automatic target discovery. Each kind
/// is discovered unless the package turns it off.
#[derive(Clone, Copy)]
struct Discovery {
    library: bool,
    binaries: bool,
    examples: bool,
    benches: bool,
    tests: bool,
    /// A build script is discovered at `build.rs` only when `package.build`
    /// neither disables it nor names a different file.
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
        let build = value
            .get("package")
            .and_then(|package| package.get("build"));
        named(build.and_then(toml::Value::as_str));
        // A target table without a path is found at the conventional
        // locations for its name, which auto-discovery settings do not
        // govern. Both shapes are recorded: only one of them exists.
        if let Some(library) = value.get("lib") {
            named(configured_path(Some(library)).or(Some("src/lib.rs")));
        }
        let package_name = value
            .get("package")
            .and_then(|package| package.get("name"))
            .and_then(toml::Value::as_str);
        for (kind, directory) in TARGET_DIRECTORIES {
            let tables = value.get(kind).and_then(toml::Value::as_array);
            for table in tables.into_iter().flatten() {
                if let Some(path) = configured_path(Some(table)) {
                    named(Some(path));
                    continue;
                }
                let Some(name) = table.get("name").and_then(toml::Value::as_str) else {
                    continue;
                };
                named(Some(&format!("{directory}/{name}.rs")));
                named(Some(&format!("{directory}/{name}/main.rs")));
                // A binary carrying the package's own name is also found at
                // the package's default entrypoint.
                if kind == "bin" && package_name == Some(name) {
                    named(Some("src/main.rs"));
                }
            }
        }
        if value.get("package").is_some() {
            self.packages.insert(dir, Discovery::read(value, build));
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
            if let Some(discovery) = self.packages.get(dir) {
                let relative = path.strip_prefix(dir).unwrap_or(path);
                return discovery.discovers(relative.trim_start_matches('/'));
            }
            if dir.is_empty() {
                return false;
            }
            dir = split_path(dir).0;
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
            // An explicit `[lib]` table defines the library target itself,
            // including the path it is read from, so `src/lib.rs` is only the
            // default when no such table replaces it.
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

    /// The package-relative locations this package discovers a target at.
    fn discovers(self, relative: &str) -> bool {
        match relative {
            "src/lib.rs" => return self.library,
            "src/main.rs" => return self.binaries,
            "build.rs" => return self.build,
            _ => {}
        }
        let Some((parent, basename)) = relative.rsplit_once('/') else {
            return false;
        };
        // `src/bin/tool.rs` and `src/bin/tool/main.rs`, and the same two
        // shapes under `benches`, `examples` and `tests`. A target directly in
        // one of those directories counts whatever it is named, including
        // `main.rs`, before the nested shape is considered.
        self.enabled(parent) || (basename == "main.rs" && self.enabled(split_path(parent).0))
    }

    fn enabled(self, directory: &str) -> bool {
        match directory {
            "src/bin" => self.binaries,
            "examples" => self.examples,
            "benches" => self.benches,
            "tests" => self.tests,
            _ => false,
        }
    }
}

fn configured_path(table: Option<&toml::Value>) -> Option<&str> {
    table
        .and_then(|table| table.get("path"))
        .and_then(toml::Value::as_str)
}
