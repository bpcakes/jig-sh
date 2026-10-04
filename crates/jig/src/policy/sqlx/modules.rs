//! External `mod` resolution for SQLx inventory test classification.
//!
//! The inventory scans each source file on its own, so a `#[cfg(test)]`
//! attribute in a parent file says nothing about the child file it declares.
//! Resolving straightforward `mod` declarations recovers that relationship:
//! a file whose only module parents are test-only belongs to the test
//! inventory, exactly as the equivalent inline module already does.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use super::scanner::{has_cfg_test, unraw};

/// One external `mod` declaration, with every repository-relative file it can
/// name. A declaration with no resolvable candidate contributes no edge.
pub(super) struct ModuleDecl {
    candidates: Vec<String>,
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

/// Collects the external `mod` declarations of a parsed file. Only items are
/// walked, so a `mod` inside a block expression is left unresolved along with
/// the macro-generated and `include!`-introduced declarations the inventory
/// already documents as out of reach.
pub(super) fn collect_declarations(path: &str, items: &[syn::Item]) -> Vec<ModuleDecl> {
    let mut declarations = Vec::new();
    collect_items(&module_dir(path), items, false, &mut declarations);
    declarations
}

fn collect_items(dir: &str, items: &[syn::Item], cfg_test: bool, out: &mut Vec<ModuleDecl>) {
    for item in items {
        let syn::Item::Mod(module) = item else {
            continue;
        };
        let cfg_test = cfg_test || has_cfg_test(&module.attrs);
        let attribute = path_attribute(&module.attrs);
        match &module.content {
            // A `#[path]` attribute on an inline module names the directory
            // its own children resolve against; otherwise the module name does.
            Some((_, items)) => {
                let name = attribute.unwrap_or_else(|| unraw(&module.ident));
                if let Some(child) = join_relative(dir, &name) {
                    collect_items(&child, items, cfg_test, out);
                }
            }
            None => {
                let candidates = match attribute {
                    Some(value) => join_relative(dir, &value).into_iter().collect(),
                    None => {
                        let name = unraw(&module.ident);
                        [format!("{name}.rs"), format!("{name}/mod.rs")]
                            .iter()
                            .filter_map(|relative| join_relative(dir, relative))
                            .collect()
                    }
                };
                out.push(ModuleDecl {
                    candidates,
                    cfg_test,
                });
            }
        }
    }
}

/// Returns the scanned files that only `#[cfg(test)]` module declarations
/// reach, so every call site they contain belongs to the test inventory.
pub(super) fn test_only_files(files: &BTreeMap<String, FileModules>) -> BTreeSet<String> {
    // Production declarations each file makes, and for every claimed file the
    // number of production declarations it must shed to become test-only.
    let mut production_children: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let mut production_claims: BTreeMap<&str, usize> = BTreeMap::new();
    let mut claimed: BTreeSet<&str> = BTreeSet::new();
    for (path, modules) in files {
        for declaration in &modules.declarations {
            for candidate in &declaration.candidates {
                // A declaration naming no inventoried file resolves nothing,
                // so that file keeps the classification its own markers give.
                let Some((child, _)) = files.get_key_value(candidate.as_str()) else {
                    continue;
                };
                let child = child.as_str();
                claimed.insert(child);
                if !declaration.cfg_test {
                    production_children
                        .entry(path.as_str())
                        .or_default()
                        .push(child);
                    *production_claims.entry(child).or_default() += 1;
                }
            }
        }
    }
    // A file is test-only when it says so itself, or when every declaration
    // claiming it is under `#[cfg(test)]` or made by a test-only file. A file
    // production code still reaches, one no declaration claims, and one in a
    // declaration cycle no production root enters keep their own
    // classification, so the inventory never silently loses production code.
    let mut test_only: BTreeSet<&str> = BTreeSet::new();
    let mut pending: VecDeque<&str> = VecDeque::new();
    for (path, modules) in files {
        let path = path.as_str();
        let claimed_for_test = claimed.contains(path) && !production_claims.contains_key(path);
        if (modules.self_test || claimed_for_test) && test_only.insert(path) {
            pending.push_back(path);
        }
    }
    while let Some(path) = pending.pop_front() {
        for child in production_children.get(path).into_iter().flatten() {
            // Each file leaves the queue once, so a declaration is shed once.
            let Some(remaining) = production_claims.get_mut(child) else {
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

/// The directory a file's child modules resolve against. A crate root or
/// `mod.rs` shares its directory with its children; any other file owns the
/// directory named after it.
fn module_dir(path: &str) -> String {
    let (parent, basename) = path.rsplit_once('/').unwrap_or(("", path));
    match basename {
        "mod.rs" | "lib.rs" | "main.rs" => parent.to_string(),
        _ => {
            let stem = basename.strip_suffix(".rs").unwrap_or(basename);
            join_relative(parent, stem).unwrap_or_default()
        }
    }
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
    (!parts.is_empty()).then(|| parts.join("/"))
}
