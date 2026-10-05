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

use super::cargo_targets::CargoTargets;
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
    pub(super) cfg_test: bool,
}

/// What one scanned file contributes to the module graph.
pub(super) struct FileModules {
    /// Whether an exact `#[cfg(test)]` attribute covers the whole file, which
    /// keeps everything it declares out of a production build too. A
    /// conventional test path is not recorded here: it names the file's own
    /// calls as test code without saying anything about what reaches it.
    pub(super) cfg_test: bool,
    pub(super) declarations: Vec<ModuleDecl>,
}

/// Where a loaded file resolves its declarations: the directory a `#[path]`
/// value extends, and the module name a conventional declaration appends
/// first. Rust keeps that name pending until a declaration consumes it, which
/// is why `src/a/b.rs` loads `mod c;` from `src/a/b/` but `#[path = "c.rs"]`
/// from `src/a/`.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Dir {
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
pub(super) fn resolve(declaration: &ModuleDecl, dir: &Dir) -> Option<Vec<(String, Dir)>> {
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

/// The directory a Cargo target resolves its declarations against, when the
/// file is one. A crate root resolves its children beside itself.
pub(super) fn cargo_target_dir(path: &str, targets: &CargoTargets) -> Option<Dir> {
    targets.contains(path).then(|| Dir::owned(parent_dir(path)))
}

/// The directory a file resolves its declarations against when it is a root
/// of its own: the conventional layout its name implies. A crate root and a
/// `mod.rs` share their directory with their children; any other module file
/// owns a directory named after it.
pub(super) fn root_dir(path: &str) -> Dir {
    let (parent, basename) = split_path(path);
    match basename.strip_suffix(".rs") {
        Some(stem) if !matches!(basename, "mod.rs" | "lib.rs" | "main.rs") => Dir {
            path: parent.to_string(),
            pending: Some(stem.to_string()),
        },
        _ => Dir::owned(parent.to_string()),
    }
}

pub(super) fn split_path(path: &str) -> (&str, &str) {
    path.rsplit_once('/').unwrap_or(("", path))
}

pub(super) fn parent_dir(path: &str) -> String {
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
pub(super) fn join_relative(dir: &str, value: &str) -> Option<String> {
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
