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

use std::collections::BTreeSet;

use syn::visit::{self, Visit};

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

/// What encloses a declaration and so decides the directory it resolves
/// against.
#[derive(Clone)]
enum Step {
    /// An inline module block, which extends the directory by its own name.
    Inline(ModuleName),
    /// A block expression, which drops the module name the file keeps
    /// pending without extending the directory.
    Block,
}

/// One external `mod` declaration, with whatever encloses it.
pub(super) struct ModuleDecl {
    steps: Vec<Step>,
    name: ModuleName,
    /// Whether an exact `#[cfg(test)]` attribute covers this declaration,
    /// on the declaration itself or on anything enclosing it.
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

/// Collects the external `mod` declarations of a parsed file, including the
/// ones inside a block, which Rust requires to carry a `#[path]`. Macro input
/// is not expanded, so declarations a macro produces stay out of reach, as
/// the inventory documents.
pub(super) fn collect_declarations(items: &[syn::Item]) -> Vec<ModuleDecl> {
    let mut collector = Declarations {
        steps: Vec::new(),
        cfg_test: false,
        out: Vec::new(),
    };
    for item in items {
        visit::visit_item(&mut collector, item);
    }
    collector.out
}

struct Declarations {
    steps: Vec<Step>,
    cfg_test: bool,
    out: Vec<ModuleDecl>,
}

impl Declarations {
    /// Collects with an exact `#[cfg(test)]` on these attributes in force, so
    /// a declaration inside a test-only item inherits it.
    fn within(&mut self, attrs: &[syn::Attribute], collect: impl FnOnce(&mut Self)) {
        let enclosing = self.cfg_test;
        self.cfg_test |= has_cfg_test(attrs);
        collect(self);
        self.cfg_test = enclosing;
    }

    fn in_block(&self) -> bool {
        self.steps.iter().any(|step| matches!(step, Step::Block))
    }
}

impl<'ast> Visit<'ast> for Declarations {
    fn visit_item_mod(&mut self, module: &'ast syn::ItemMod) {
        let name = match path_attribute(&module.attrs) {
            Some(value) => ModuleName::Path(value),
            None => ModuleName::Named(unraw(&module.ident)),
        };
        self.within(&module.attrs, |collector| match &module.content {
            Some(_) => {
                collector.steps.push(Step::Inline(name));
                visit::visit_item_mod(collector, module);
                collector.steps.pop();
            }
            // Rust rejects a file module inside a block that carries no
            // `#[path]`, so there is no such file to resolve.
            None if collector.in_block() && matches!(&name, ModuleName::Named(_)) => {}
            None => collector.out.push(ModuleDecl {
                steps: collector.steps.clone(),
                name,
                cfg_test: collector.cfg_test,
            }),
        });
    }

    fn visit_block(&mut self, block: &'ast syn::Block) {
        self.steps.push(Step::Block);
        visit::visit_block(self, block);
        self.steps.pop();
    }

    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        self.within(&item.attrs, |collector| {
            visit::visit_item_fn(collector, item)
        });
    }

    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        self.within(&item.attrs, |collector| {
            visit::visit_item_impl(collector, item);
        });
    }

    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        self.within(&item.attrs, |collector| {
            visit::visit_impl_item_fn(collector, item);
        });
    }

    fn visit_trait_item_fn(&mut self, item: &'ast syn::TraitItemFn) {
        self.within(&item.attrs, |collector| {
            visit::visit_trait_item_fn(collector, item);
        });
    }
}

/// Every file one declaration can load from a file loaded with `dir`, with the
/// directory each candidate would itself be loaded with.
pub(super) fn resolve(declaration: &ModuleDecl, dir: &Dir) -> Option<Vec<(String, Dir)>> {
    let mut dir = dir.clone();
    for step in &declaration.steps {
        dir = match step {
            Step::Inline(name) => Dir::owned(name.directory(&dir)?),
            Step::Block => Dir::owned(dir.path.clone()),
        };
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

/// Every repository path a file's declarations could name, under any of the
/// directories that file could be loaded with. This finds the sources whose
/// relationships the graph needs even though their own call sites are out of
/// the configured crate roots, so it deliberately over-approximates: the
/// graph itself then resolves each declaration against one directory only.
pub(super) fn candidate_targets(
    path: &str,
    modules: &FileModules,
    targets: &CargoTargets,
) -> BTreeSet<String> {
    let loadings = [
        Some(root_dir(path)),
        Some(Dir::owned(parent_dir(path))),
        cargo_target_dir(path, targets),
    ];
    let mut candidates = BTreeSet::new();
    for dir in loadings.iter().flatten() {
        for declaration in &modules.declarations {
            for (target, _) in resolve(declaration, dir).into_iter().flatten() {
                candidates.insert(target);
            }
        }
    }
    candidates
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
