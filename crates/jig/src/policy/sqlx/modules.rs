//! External `mod` resolution for SQLx inventory test classification.
//!
//! The inventory scans each source file on its own, so a `#[cfg(test)]`
//! attribute in a parent file says nothing about the child file it declares.
//! Resolving straightforward `mod` declarations recovers that relationship:
//! a file whose only module parents are test-only belongs to the test
//! inventory, exactly as the equivalent inline module already does.
//!
//! Resolution follows the directory rules Rust itself uses. A file does not
//! determine its own module directory: that depends on the declaration that
//! loaded it, so the loading context travels with each file rather than being
//! recomputed from its name.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use super::scanner::{has_cfg_test, unraw};

/// How a `mod` item names what it loads. A `#[path]` value extends the
/// directory itself and leaves any pending module name unused, so the two
/// spellings cannot be collapsed.
#[derive(Clone)]
enum ModuleName {
    Named(String),
    Path(String),
}

/// One external `mod` declaration, with the inline module blocks enclosing it.
pub(super) struct ModuleDecl {
    inline: Vec<ModuleName>,
    name: ModuleName,
    /// Whether an exact `#[cfg(test)]` attribute covers this declaration,
    /// either on the declaration itself or on an enclosing inline module.
    cfg_test: bool,
}

/// What one scanned file contributes to the module graph.
pub(super) struct FileModules {
    /// Whether the file is already test code on its own: a conventional test
    /// path, or an exact `#[cfg(test)]` attribute on the file itself.
    pub(super) self_test: bool,
    pub(super) declarations: Vec<ModuleDecl>,
}

/// Where a loaded file resolves its declarations: the directory a `#[path]`
/// value extends, and the module name a conventional declaration appends
/// first. Rust keeps that name pending until a declaration consumes it, which
/// is why `src/a/b.rs` loads `mod c;` from `src/a/b/` but `#[path = "c.rs"]`
/// from `src/a/`.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Dir {
    path: String,
    pending: Option<String>,
}

impl Dir {
    fn owned(path: String) -> Self {
        Self {
            path,
            pending: None,
        }
    }

    /// The directory a conventional `mod name;` resolves against.
    fn anchored(&self) -> Option<String> {
        match &self.pending {
            Some(name) => join_relative(&self.path, name),
            None => Some(self.path.clone()),
        }
    }
}

impl ModuleName {
    /// The directory an enclosing inline module block establishes.
    fn directory(&self, dir: &Dir) -> Option<String> {
        match self {
            Self::Path(value) => join_relative(&dir.path, value),
            Self::Named(name) => join_relative(&dir.anchored()?, name),
        }
    }
}

/// Collects the external `mod` declarations of a parsed file. Only items are
/// walked, so a `mod` inside a block expression is left unresolved along with
/// the macro-generated and `include!`-introduced declarations the inventory
/// already documents as out of reach.
pub(super) fn collect_declarations(items: &[syn::Item]) -> Vec<ModuleDecl> {
    let mut declarations = Vec::new();
    collect_items(items, &mut Vec::new(), false, &mut declarations);
    declarations
}

fn collect_items(
    items: &[syn::Item],
    inline: &mut Vec<ModuleName>,
    cfg_test: bool,
    out: &mut Vec<ModuleDecl>,
) {
    for item in items {
        let syn::Item::Mod(module) = item else {
            continue;
        };
        let cfg_test = cfg_test || has_cfg_test(&module.attrs);
        let name = match path_attribute(&module.attrs) {
            Some(value) => ModuleName::Path(value),
            None => ModuleName::Named(unraw(&module.ident)),
        };
        match &module.content {
            Some((_, items)) => {
                inline.push(name);
                collect_items(items, inline, cfg_test, out);
                inline.pop();
            }
            None => out.push(ModuleDecl {
                inline: inline.clone(),
                name,
                cfg_test,
            }),
        }
    }
}

/// Every file one declaration can load from a file loaded with `dir`, with the
/// directory each candidate would itself be loaded with.
fn resolve(declaration: &ModuleDecl, dir: &Dir) -> Option<Vec<(String, Dir)>> {
    let mut dir = dir.clone();
    for step in &declaration.inline {
        dir = Dir::owned(step.directory(&dir)?);
    }
    match &declaration.name {
        // An explicit path names the file directly, and that file owns the
        // directory it sits in rather than one named after it.
        ModuleName::Path(value) => {
            let target = join_relative(&dir.path, value)?;
            let child = Dir::owned(parent_dir(&target));
            Some(vec![(target, child)])
        }
        // A conventional declaration is either `name.rs` beside the anchor or
        // `name/mod.rs` beneath it; both resolve their own children in
        // `anchor/name`.
        ModuleName::Named(name) => {
            let anchor = dir.anchored()?;
            let nested = join_relative(&anchor, name)?;
            Some(vec![
                (
                    format!("{nested}.rs"),
                    Dir {
                        path: anchor,
                        pending: Some(name.clone()),
                    },
                ),
                (format!("{nested}/mod.rs"), Dir::owned(nested)),
            ])
        }
    }
}

/// Which files declare which, after resolving every declaration the inventory
/// can follow.
#[derive(Default)]
struct Claims<'a> {
    /// Files each file loads through a declaration not under `#[cfg(test)]`.
    production_children: BTreeMap<&'a str, Vec<&'a str>>,
    /// Outstanding such declarations per claimed file.
    production_claims: BTreeMap<&'a str, usize>,
    /// Every file some resolved declaration loads.
    claimed: BTreeSet<&'a str>,
}

fn resolve_claims<'a>(
    files: &'a BTreeMap<String, FileModules>,
    targets: &BTreeSet<String>,
) -> Claims<'a> {
    let mut claims = Claims::default();
    let mut directories: BTreeMap<&str, BTreeSet<Dir>> = BTreeMap::new();
    let mut queue: VecDeque<(&str, Dir)> = VecDeque::new();
    // Cargo compiles a crate entrypoint wherever it sits, so start from each
    // one; a file no declaration reaches is seeded below instead.
    for path in files.keys() {
        if let Some(dir) = cargo_target_dir(path, targets) {
            seed(path, dir, &mut directories, &mut queue);
        }
    }
    let mut unreached = files.keys();
    loop {
        while let Some((path, dir)) = queue.pop_front() {
            for declaration in &files[path].declarations {
                let Some(candidates) = resolve(declaration, &dir) else {
                    continue;
                };
                for (target, child_dir) in candidates {
                    // A declaration naming no inventoried file resolves
                    // nothing, so that file keeps its own classification.
                    let Some((child, _)) = files.get_key_value(target.as_str()) else {
                        continue;
                    };
                    let child = child.as_str();
                    claims.claimed.insert(child);
                    if !declaration.cfg_test {
                        claims
                            .production_children
                            .entry(path)
                            .or_default()
                            .push(child);
                        *claims.production_claims.entry(child).or_default() += 1;
                    }
                    seed(child, child_dir, &mut directories, &mut queue);
                }
            }
        }
        // A file no resolved declaration reaches still resolves declarations
        // of its own, against the conventional directory its name implies.
        let Some(path) = unreached.find(|path| !directories.contains_key(path.as_str())) else {
            return claims;
        };
        seed(path, unreached_dir(path), &mut directories, &mut queue);
    }
}

/// Target paths a Cargo manifest configures explicitly, resolved relative to
/// the manifest. Cargo compiles each as its own crate root, so a module
/// declaration elsewhere cannot make one test code. Conventional target
/// locations need no manifest and are recognized by `cargo_target_dir`.
pub(super) fn collect_target_paths(
    manifest: &str,
    value: &toml::Value,
    out: &mut BTreeSet<String>,
) {
    let dir = parent_dir(manifest);
    let mut record = |configured: Option<&toml::Value>| {
        if let Some(path) = configured.and_then(toml::Value::as_str)
            && let Some(target) = join_relative(&dir, path)
        {
            out.insert(target);
        }
    };
    record(
        value
            .get("package")
            .and_then(|package| package.get("build")),
    );
    record(value.get("lib").and_then(|library| library.get("path")));
    for kind in ["bin", "example", "bench", "test"] {
        let configured = value.get(kind).and_then(toml::Value::as_array);
        for target in configured.into_iter().flatten() {
            record(target.get("path"));
        }
    }
}

/// Records a directory a file can be loaded with, queueing it when it is new.
fn seed<'a>(
    path: &'a str,
    dir: Dir,
    directories: &mut BTreeMap<&'a str, BTreeSet<Dir>>,
    queue: &mut VecDeque<(&'a str, Dir)>,
) {
    if directories.entry(path).or_default().insert(dir.clone()) {
        queue.push_back((path, dir));
    }
}

/// Returns the scanned files that only `#[cfg(test)]` module declarations
/// reach, so every call site they contain belongs to the test inventory.
pub(super) fn test_only_files(
    files: &BTreeMap<String, FileModules>,
    targets: &BTreeSet<String>,
) -> BTreeSet<String> {
    let mut claims = resolve_claims(files, targets);
    // A file is test-only when it says so itself, or when every declaration
    // claiming it is under `#[cfg(test)]` or made by a test-only file. A Cargo
    // target, a file production code still reaches, one no declaration claims,
    // and one in a declaration cycle no target enters keep their own
    // classification, so no production call site is quietly lost.
    let mut test_only: BTreeSet<&str> = BTreeSet::new();
    let mut pending: VecDeque<&str> = VecDeque::new();
    for (path, modules) in files {
        let path = path.as_str();
        let test_claimed = claims.claimed.contains(path)
            && !claims.production_claims.contains_key(path)
            && cargo_target_dir(path, targets).is_none();
        if (modules.self_test || test_claimed) && test_only.insert(path) {
            pending.push_back(path);
        }
    }
    while let Some(path) = pending.pop_front() {
        for child in claims.production_children.get(path).into_iter().flatten() {
            if cargo_target_dir(child, targets).is_some() {
                continue;
            }
            // Each file leaves the queue once, so a declaration is shed once.
            let Some(remaining) = claims.production_claims.get_mut(child) else {
                continue;
            };
            *remaining -= 1;
            if *remaining == 0 && test_only.insert(child) {
                pending.push_back(child);
            }
        }
    }
    test_only.into_iter().map(str::to_string).collect()
}

/// The directory a Cargo target resolves its declarations against, when the
/// file is one: a conventional entrypoint location, or a path a manifest
/// configures explicitly. Cargo compiles each as a crate root, so production
/// code reaches it whatever else also declares it as a module.
fn cargo_target_dir(path: &str, targets: &BTreeSet<String>) -> Option<Dir> {
    let (parent, basename) = split_path(path);
    let standalone = basename != "mod.rs"
        && matches!(
            split_path(parent).1,
            "bin" | "benches" | "examples" | "tests"
        );
    let target = matches!(basename, "lib.rs" | "main.rs") || standalone || targets.contains(path);
    target.then(|| Dir::owned(parent.to_string()))
}

/// The directory a file resolves its declarations against when no resolved
/// declaration loads it: the conventional layout its own name implies.
fn unreached_dir(path: &str) -> Dir {
    let (parent, basename) = split_path(path);
    match basename.strip_suffix(".rs") {
        Some(stem) if basename != "mod.rs" => Dir {
            path: parent.to_string(),
            pending: Some(stem.to_string()),
        },
        _ => Dir::owned(parent.to_string()),
    }
}

fn split_path(path: &str) -> (&str, &str) {
    path.rsplit_once('/').unwrap_or(("", path))
}

fn parent_dir(path: &str) -> String {
    split_path(path).0.to_string()
}

fn path_attribute(attrs: &[syn::Attribute]) -> Option<String> {
    attrs.iter().find_map(|attr| {
        if !attr.path().is_ident("path") {
            return None;
        }
        match &attr.meta {
            syn::Meta::NameValue(value) => match &value.value {
                syn::Expr::Lit(literal) => match &literal.lit {
                    syn::Lit::Str(text) => Some(text.value()),
                    _ => None,
                },
                _ => None,
            },
            _ => None,
        }
    })
}

/// Resolves a declaration-relative path against a directory, producing a
/// repository-relative path. An absolute path or one that leaves the
/// repository root resolves to nothing rather than to a guess.
fn join_relative(dir: &str, value: &str) -> Option<String> {
    if value.starts_with('/') {
        return None;
    }
    let mut parts: Vec<&str> = if dir.is_empty() {
        Vec::new()
    } else {
        dir.split('/').collect()
    };
    for part in value.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part => parts.push(part),
        }
    }
    Some(parts.join("/"))
}
