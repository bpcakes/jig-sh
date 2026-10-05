//! Which files only `#[cfg(test)]` module declarations reach.
//!
//! A file is classified through the declarations that load it, and the same
//! file can be loaded more than once: a library can declare `mod shared;` for
//! production and `#[cfg(test)] #[path = "shared.rs"] mod cases;` beside it,
//! and `mod helper;` inside that file then names a different file under each.
//! The graph therefore has one node per file and loading directory, and a
//! file is test code only when every way of loading it is.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use super::cargo_targets::CargoTargets;
use super::modules::{Dir, FileModules, cargo_target_dir, resolve, root_dir};

/// How many times the set of files treated as roots of their own is revised
/// before the inventory declines to reclassify anything.
const MAX_ROOT_REVISIONS: usize = 16;

/// One file loaded with one directory, and what that loading declares.
struct Node<'a> {
    file: &'a str,
    /// Nodes this one loads through a declaration not under `#[cfg(test)]`.
    production_children: Vec<usize>,
    /// Outstanding such declarations loading this node.
    production_claims: usize,
    /// Whether any declaration loads this node at all.
    claimed: bool,
}

struct Graph<'a> {
    nodes: Vec<Node<'a>>,
    /// The nodes of each file, in discovery order.
    by_file: BTreeMap<&'a str, Vec<usize>>,
    /// Files some declaration loads, other than through themselves.
    loaded: BTreeSet<&'a str>,
}

/// Returns the scanned files that only `#[cfg(test)]` module declarations
/// reach, so every call site they contain belongs to the test inventory.
pub(super) fn test_only_files(
    files: &BTreeMap<String, FileModules>,
    targets: &CargoTargets,
) -> BTreeSet<String> {
    let Some(mut graph) = resolve_graph(files, targets) else {
        // Declarations that only load one another cannot compile, so a root
        // set that will not settle is pathological. Reclassifying nothing
        // leaves every call site where it already was.
        return BTreeSet::new();
    };
    // A loading is test-only when an attribute compiles the whole file for
    // tests alone, or when every declaration reaching it is under
    // `#[cfg(test)]` or comes from a test-only loading. A Cargo target, a
    // loading production code still reaches, and one no declaration reaches
    // keep their own classification, so no production call site is lost.
    let mut test_only = vec![false; graph.nodes.len()];
    let mut pending: VecDeque<usize> = VecDeque::new();
    for (index, node) in graph.nodes.iter().enumerate() {
        let attributed = files[node.file].cfg_test;
        let claimed_for_test = node.claimed
            && node.production_claims == 0
            && cargo_target_dir(node.file, targets).is_none();
        if attributed || claimed_for_test {
            test_only[index] = true;
            pending.push_back(index);
        }
    }
    while let Some(index) = pending.pop_front() {
        for child in std::mem::take(&mut graph.nodes[index].production_children) {
            let node = &mut graph.nodes[child];
            if test_only[child] || cargo_target_dir(node.file, targets).is_some() {
                continue;
            }
            // Each node leaves the queue once, so a declaration is shed once.
            node.production_claims -= 1;
            if node.production_claims == 0 {
                test_only[child] = true;
                pending.push_back(child);
            }
        }
    }
    // A file is test code only when every way of loading it is test-only.
    graph
        .by_file
        .iter()
        .filter(|(_, nodes)| nodes.iter().all(|index| test_only[*index]))
        .map(|(file, _)| (*file).to_string())
        .collect()
}

/// Resolves the graph, revising which files are roots of their own until the
/// answer agrees with itself. A file's module directory depends on the
/// declaration that loaded it, so the directory its own name implies is only
/// a guess: a file some declaration turns out to load stops being a root, and
/// a file left unloaded becomes one, each of which changes the next pass.
fn resolve_graph<'a>(
    files: &'a BTreeMap<String, FileModules>,
    targets: &CargoTargets,
) -> Option<Graph<'a>> {
    let mut roots: BTreeSet<&'a str> = BTreeSet::new();
    for _ in 0..MAX_ROOT_REVISIONS {
        let graph = resolve_pass(files, targets, &roots);
        // A file another declaration loads is not a root of its own, and the
        // directory its name implied is replaced by the resolved one.
        let loaded = roots
            .iter()
            .copied()
            .filter(|path| graph.loaded.contains(path))
            .collect::<Vec<_>>();
        if !loaded.is_empty() {
            for path in loaded {
                roots.remove(path);
            }
            continue;
        }
        // A file no declaration loads still resolves declarations of its own,
        // against the conventional directory its name implies.
        let unloaded = files
            .keys()
            .map(String::as_str)
            .filter(|path| {
                !graph.loaded.contains(path)
                    && !roots.contains(path)
                    && cargo_target_dir(path, targets).is_none()
            })
            .collect::<Vec<_>>();
        if unloaded.is_empty() {
            return Some(graph);
        }
        roots.extend(unloaded);
    }
    None
}

/// One resolution pass, seeding Cargo's own crate roots and each file in
/// `roots` with the directory its name implies.
fn resolve_pass<'a>(
    files: &'a BTreeMap<String, FileModules>,
    targets: &CargoTargets,
    roots: &BTreeSet<&'a str>,
) -> Graph<'a> {
    let mut graph = Graph {
        nodes: Vec::new(),
        by_file: BTreeMap::new(),
        loaded: BTreeSet::new(),
    };
    let mut indices: BTreeMap<(&'a str, Dir), usize> = BTreeMap::new();
    let mut queue: VecDeque<(&'a str, Dir, usize)> = VecDeque::new();
    // Cargo compiles a crate root wherever it sits, so start from each one;
    // a file no declaration reaches is seeded as its own root.
    for (path, dir) in files
        .keys()
        .filter_map(|path| cargo_target_dir(path, targets).map(|dir| (path.as_str(), dir)))
        .chain(roots.iter().map(|path| (*path, root_dir(path))))
    {
        graph.seed(path, dir, &mut indices, &mut queue);
    }
    while let Some((path, dir, index)) = queue.pop_front() {
        for declaration in &files[path].declarations {
            let Some(candidates) = resolve(declaration, &dir) else {
                continue;
            };
            for (target, child_dir) in candidates {
                // A declaration naming no inventoried file resolves nothing,
                // so that file keeps its own classification.
                let Some((child, _)) = files.get_key_value(target.as_str()) else {
                    continue;
                };
                let child = child.as_str();
                let child_index = graph.seed(child, child_dir, &mut indices, &mut queue);
                // A file that only loads itself is no less a root for it.
                if child != path {
                    graph.loaded.insert(child);
                }
                graph.nodes[child_index].claimed = true;
                if !declaration.cfg_test {
                    graph.nodes[index].production_children.push(child_index);
                    graph.nodes[child_index].production_claims += 1;
                }
            }
        }
    }
    graph
}

impl<'a> Graph<'a> {
    /// The node for one file and directory, queueing it when it is new.
    fn seed(
        &mut self,
        path: &'a str,
        dir: Dir,
        indices: &mut BTreeMap<(&'a str, Dir), usize>,
        queue: &mut VecDeque<(&'a str, Dir, usize)>,
    ) -> usize {
        if let Some(index) = indices.get(&(path, dir.clone())) {
            return *index;
        }
        let index = self.nodes.len();
        self.nodes.push(Node {
            file: path,
            production_children: Vec::new(),
            production_claims: 0,
            claimed: false,
        });
        self.by_file.entry(path).or_default().push(index);
        indices.insert((path, dir.clone()), index);
        queue.push_back((path, dir, index));
        index
    }
}
