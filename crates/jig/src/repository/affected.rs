use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use globset::{GlobBuilder, GlobMatcher};
use jig_contract::{ActionSpec, ComponentId, ComponentSpec, SelectionReason, TargetId};
use sha2::{Digest, Sha256};

use super::RepositoryCatalog;

pub(super) const MAX_SELECTION_REASONS: usize = 100;
const SELECTION_REASON_ITEM_DIGEST_DOMAIN: &[u8] = b"jig-selection-reason-item-v2\0";

#[derive(Debug, Default, Eq, PartialEq)]
pub(super) struct TargetSelectionReasons {
    pub(super) explicit: BTreeSet<SelectionReason>,
    pub(super) batches: Vec<Arc<SelectionReasonBatch>>,
}

impl TargetSelectionReasons {
    pub(super) fn insert(&mut self, reason: SelectionReason) -> bool {
        self.explicit.insert(reason)
    }

    fn attach(&mut self, batch: &Arc<SelectionReasonBatch>) {
        self.batches.push(Arc::clone(batch));
    }

    #[cfg(test)]
    fn contains(&self, reason: &SelectionReason) -> bool {
        self.explicit.contains(reason)
            || self
                .batches
                .iter()
                .any(|batch| batch.preview.contains(reason))
    }

    #[cfg(test)]
    fn preview(&self) -> BTreeSet<SelectionReason> {
        self.explicit
            .iter()
            .cloned()
            .chain(
                self.batches
                    .iter()
                    .flat_map(|batch| batch.preview.iter().cloned()),
            )
            .collect()
    }
}

#[derive(Debug, Eq, PartialEq)]
pub(super) struct SelectionReasonBatch {
    pub(super) preview: Vec<SelectionReason>,
    pub(super) count: usize,
    pub(super) sum_digest: [u8; 32],
}

impl SelectionReasonBatch {
    fn from_reasons(reasons: impl IntoIterator<Item = SelectionReason>) -> Result<Arc<Self>> {
        let mut preview = Vec::new();
        let mut count = 0usize;
        let mut sum_digest = [0u8; 32];
        for reason in reasons {
            add_selection_reason_digest(&mut sum_digest, selection_reason_item_digest(&reason)?);
            count = count.saturating_add(1);
            if preview.len() < MAX_SELECTION_REASONS {
                preview.push(reason);
            }
        }
        Ok(Arc::new(Self {
            preview,
            count,
            sum_digest,
        }))
    }
}

/// Adds a reason digest modulo 2^256. This keeps independently built batches
/// composable and traversal-order independent while preserving duplicate
/// multiplicity, unlike XOR aggregation.
pub(super) fn add_selection_reason_digest(aggregate: &mut [u8; 32], digest: [u8; 32]) {
    let mut carry = false;
    for (aggregate_byte, digest_byte) in aggregate.iter_mut().zip(digest).rev() {
        let (value, first_carry) = aggregate_byte.overflowing_add(digest_byte);
        let (value, second_carry) = value.overflowing_add(u8::from(carry));
        *aggregate_byte = value;
        carry = first_carry || second_carry;
    }
}

pub(super) fn selection_reason_item_digest(reason: &SelectionReason) -> Result<[u8; 32]> {
    let encoded = serde_json::to_vec(reason)
        .context("Failed to canonicalize an affected-selection reason")?;
    let mut hasher = Sha256::new();
    hasher.update(SELECTION_REASON_ITEM_DIGEST_DOMAIN);
    hasher.update((encoded.len() as u64).to_be_bytes());
    hasher.update(encoded);
    Ok(hasher.finalize().into())
}

pub(super) type TargetSelection = BTreeMap<TargetId, TargetSelectionReasons>;

/// Checked-in source outside `.agent/**` that defines the repository catalog.
/// The generated manifest is tracked separately by the canonical contract
/// digest and is intentionally outside the affected-source projection.
const REPOSITORY_AUTHORITY_INPUTS: &[&str] = &[".jig.toml"];
// The launcher does not define catalog records, but ignoring it would let an
// affected check skip validation after its execution-routing policy changed.
const AFFECTED_IGNORE_PROTECTED_INPUTS: &[&str] = &[".jig.toml", "scripts/jig"];

/// Validates the path policy needed by affected selection when a native
/// repository catalog is constructed. Legacy projections do not declare this
/// policy and continue to use their compatibility behavior.
pub(super) fn validate_native_path_policy(
    components: &BTreeMap<ComponentId, ComponentSpec>,
    actions: &BTreeMap<TargetId, ActionSpec>,
    affected_ignore: &[String],
) -> Result<()> {
    for component in components.values() {
        validate_component_root(component)?;
    }
    for action in actions.values() {
        for input in &action.inputs {
            compile_input(&action.target, input)?;
        }
    }
    for pattern in affected_ignore {
        let matcher = compile_path_pattern("affected_ignore", pattern)?;
        if AFFECTED_IGNORE_PROTECTED_INPUTS
            .iter()
            .any(|path| matcher.is_match(path))
        {
            bail!(
                "affected_ignore pattern {pattern:?} must not match repository execution authority"
            );
        }
    }
    Ok(())
}

/// Narrows an already resolved selector/profile candidate set to targets whose
/// components are affected by the supplied repository-relative changed paths.
/// Action dependencies deliberately expand later in the planner.
pub(super) fn select_affected_targets(
    catalog: &RepositoryCatalog,
    candidates: TargetSelection,
    changed_paths: &[String],
    observed_input_paths: &[String],
) -> Result<TargetSelection> {
    let mut paths = normalized_changed_paths(changed_paths)?;
    let matchers = component_input_matchers(catalog)?;
    // Ignored dotenv files have no committed baseline, so their presence is
    // not a Git change. Keep a presence-only observation only when checked-in
    // action policy explicitly declares it as an input; never let it become an
    // unclaimed change or a component-root fallback.
    paths.extend(
        normalized_changed_paths(observed_input_paths)?
            .into_iter()
            .filter(|path| {
                matchers
                    .values()
                    .any(|inputs| inputs.iter().any(|input| input.is_match(path)))
            }),
    );
    let ignored_matchers = catalog
        .affected_ignore()
        .iter()
        .map(|pattern| compile_path_pattern("affected_ignore", pattern))
        .collect::<Result<Vec<_>>>()?;
    paths.retain(|path| {
        REPOSITORY_AUTHORITY_INPUTS.contains(&path.as_str())
            || matchers
                .values()
                .any(|inputs| inputs.iter().any(|input| input.is_match(path)))
            || !ignored_matchers
                .iter()
                .any(|matcher| matcher.is_match(path))
    });
    let authority_paths = paths
        .iter()
        .filter(|path| REPOSITORY_AUTHORITY_INPUTS.contains(&path.as_str()))
        .cloned()
        .collect::<BTreeSet<_>>();
    let (mut direct, mut unclaimed_paths) =
        directly_affected_components(catalog, &matchers, &paths);
    for path in &authority_paths {
        unclaimed_paths.remove(path);
        for direct_paths in direct.values_mut() {
            direct_paths.remove(path);
        }
    }
    direct.retain(|_, direct_paths| !direct_paths.is_empty());
    let unclaimed_batch = (!unclaimed_paths.is_empty())
        .then(|| {
            SelectionReasonBatch::from_reasons(
                unclaimed_paths
                    .into_iter()
                    .map(|path| SelectionReason::UnclaimedInput { path }),
            )
        })
        .transpose()?;
    let batches = affected_reason_batches(catalog, direct)?;

    let mut selected = TargetSelection::new();
    for (target, mut reasons) in candidates {
        let mut affected = !authority_paths.is_empty() || unclaimed_batch.is_some();
        for path in &authority_paths {
            reasons.insert(SelectionReason::DirectInput { path: path.clone() });
        }
        if let Some(batch) = &unclaimed_batch {
            reasons.attach(batch);
        }
        if let Some(batch) = batches.direct.get(&target.component) {
            affected = true;
            reasons.attach(batch);
        }
        if let Some(batches) = batches.propagated.get(&target.component) {
            affected = true;
            for batch in batches {
                reasons.attach(batch);
            }
        }
        if affected {
            selected.insert(target, reasons);
        }
    }
    Ok(selected)
}

fn validate_component_root(component: &ComponentSpec) -> Result<()> {
    let root = component.root.as_str();
    if root == "." {
        return Ok(());
    }
    validate_authored_repository_relative_text("component root", root)?;
    if let Err(error) = validate_observable_source_declaration("component root", root) {
        bail!(
            "component '{}' has invalid root {:?}: {error}",
            component.id,
            root
        )
    }
    if root.contains(['*', '?', '[', ']', '{', '}']) {
        bail!(
            "component '{}' has invalid root {:?}: component roots must be literal repository-relative directories",
            component.id,
            root
        );
    }
    Ok(())
}

fn compile_input(target: &TargetId, input: &str) -> Result<GlobMatcher> {
    if let Err(error) = validate_observable_source_declaration("action input", input) {
        bail!("target '{target}' has invalid input pattern {input:?}: {error}")
    }
    match compile_path_pattern("action input", input) {
        Ok(matcher) => Ok(matcher),
        Err(error) => {
            bail!("target '{target}' has invalid input pattern {input:?}: {error}")
        }
    }
}

fn validate_observable_source_declaration(kind: &str, value: &str) -> Result<()> {
    if value.split('/').next() == Some(".agent") {
        bail!(
            "{kind} must not be inside .agent/** because that runtime and harness tree is excluded from source identity and affected selection"
        );
    }
    Ok(())
}

fn compile_path_pattern(kind: &str, input: &str) -> Result<GlobMatcher> {
    if let Err(error) = validate_authored_repository_relative_text(kind, input) {
        bail!("invalid {kind} pattern {input:?}: {error}");
    }
    let mut builder = GlobBuilder::new(input);
    builder.literal_separator(true).backslash_escape(false);
    match builder.build() {
        Ok(glob) => Ok(glob.compile_matcher()),
        Err(error) => {
            bail!("invalid {kind} pattern {input:?}: {error}")
        }
    }
}

fn validate_authored_repository_relative_text(kind: &str, value: &str) -> Result<()> {
    if value.is_empty() {
        bail!("{kind} must not be empty");
    }
    if value != value.trim() {
        bail!("{kind} must not have surrounding whitespace");
    }
    if value.starts_with('/') || value.contains('\\') {
        bail!("{kind} must use repository-relative forward-slash syntax");
    }
    if value.as_bytes().get(1) == Some(&b':') {
        bail!("{kind} must not use an absolute drive path");
    }
    if value.contains('\0') {
        bail!("{kind} must not contain a NUL byte");
    }
    for segment in value.split('/') {
        if segment.is_empty() {
            bail!("{kind} must not contain an empty path segment");
        }
        if matches!(segment, "." | "..") {
            bail!("{kind} must not contain '.' or '..' path segments");
        }
    }
    Ok(())
}

fn normalized_changed_paths(changed_paths: &[String]) -> Result<BTreeSet<String>> {
    let mut normalized = BTreeSet::new();
    for path in changed_paths {
        validate_git_changed_path(path)
            .with_context(|| format!("Git reported invalid changed path {path:?}"))?;
        normalized.insert(path.clone());
    }
    Ok(normalized)
}

fn validate_git_changed_path(path: &str) -> Result<()> {
    if path.is_empty() {
        bail!("changed path must not be empty");
    }
    if path.starts_with('/') {
        bail!("changed path must remain repository-relative");
    }
    if path.contains('\0') {
        bail!("changed path must not contain a NUL byte");
    }
    for segment in path.split('/') {
        if segment.is_empty() {
            bail!("changed path must not contain an empty path segment");
        }
        if matches!(segment, "." | "..") {
            bail!("changed path must not contain '.' or '..' path segments");
        }
    }
    Ok(())
}

fn component_input_matchers(
    catalog: &RepositoryCatalog,
) -> Result<BTreeMap<ComponentId, Vec<GlobMatcher>>> {
    let mut matchers = catalog
        .components()
        .map(|component| (component.id.clone(), Vec::new()))
        .collect::<BTreeMap<_, _>>();
    for action in catalog.actions() {
        let component_matchers = matchers
            .get_mut(&action.target.component)
            .expect("catalog actions must reference defined components");
        for input in &action.inputs {
            component_matchers.push(compile_input(&action.target, input)?);
        }
    }
    Ok(matchers)
}

fn directly_affected_components(
    catalog: &RepositoryCatalog,
    matchers: &BTreeMap<ComponentId, Vec<GlobMatcher>>,
    paths: &BTreeSet<String>,
) -> (BTreeMap<ComponentId, BTreeSet<String>>, BTreeSet<String>) {
    let mut direct = BTreeMap::<ComponentId, BTreeSet<String>>::new();
    let mut unclaimed = BTreeSet::new();
    for path in paths {
        let matched_components = matchers
            .iter()
            .filter(|(_, patterns)| patterns.iter().any(|pattern| pattern.is_match(path)))
            .map(|(component, _)| component.clone())
            .collect::<BTreeSet<_>>();
        let matched_components = if matched_components.is_empty() {
            most_specific_component_roots(catalog, matchers, path)
        } else {
            matched_components
        };
        if matched_components.is_empty() {
            unclaimed.insert(path.clone());
        }
        for component in matched_components {
            direct.entry(component).or_default().insert(path.clone());
        }
    }
    (direct, unclaimed)
}

fn most_specific_component_roots(
    catalog: &RepositoryCatalog,
    matchers: &BTreeMap<ComponentId, Vec<GlobMatcher>>,
    path: &str,
) -> BTreeSet<ComponentId> {
    let mut matches = Vec::new();
    let mut maximum_depth = None;
    for component in catalog.components() {
        if !root_contains(&component.root, path) {
            continue;
        }
        if component.root == "."
            && matchers
                .get(&component.id)
                .is_some_and(|patterns| !patterns.is_empty())
        {
            continue;
        }
        let depth = root_depth(&component.root);
        maximum_depth = Some(maximum_depth.map_or(depth, |current: usize| current.max(depth)));
        matches.push((depth, component.id.clone()));
    }
    matches
        .into_iter()
        .filter(|(depth, _)| Some(*depth) == maximum_depth)
        .map(|(_, component)| component)
        .collect()
}

fn root_contains(root: &str, path: &str) -> bool {
    root == "."
        || path == root
        || path
            .strip_prefix(root)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn root_depth(root: &str) -> usize {
    if root == "." {
        0
    } else {
        root.split('/').count()
    }
}

struct AffectedReasonBatches {
    direct: BTreeMap<ComponentId, Arc<SelectionReasonBatch>>,
    propagated: BTreeMap<ComponentId, Vec<Arc<SelectionReasonBatch>>>,
}

fn affected_reason_batches(
    catalog: &RepositoryCatalog,
    direct: BTreeMap<ComponentId, BTreeSet<String>>,
) -> Result<AffectedReasonBatches> {
    let mut dependents = BTreeMap::<ComponentId, BTreeSet<ComponentId>>::new();
    for component in catalog.components() {
        for dependency in &component.depends_on {
            dependents
                .entry(dependency.clone())
                .or_default()
                .insert(component.id.clone());
        }
    }

    let mut direct_batches = BTreeMap::new();
    let mut propagated_batches = BTreeMap::<ComponentId, Vec<Arc<SelectionReasonBatch>>>::new();
    for (origin, paths) in direct {
        direct_batches.insert(
            origin.clone(),
            SelectionReasonBatch::from_reasons(
                paths
                    .iter()
                    .cloned()
                    .map(|path| SelectionReason::DirectInput { path }),
            )?,
        );
        let dependency_batch = SelectionReasonBatch::from_reasons(paths.into_iter().map(|path| {
            SelectionReason::ComponentDependency {
                component: origin.clone(),
                path: Some(path),
            }
        }))?;
        let mut seen = BTreeSet::from([origin.clone()]);
        let mut pending = vec![origin.clone()];
        while let Some(component_id) = pending.pop() {
            let component = catalog
                .component(&component_id)
                .expect("affected components must exist in the catalog");
            if !component.propagate_affected_to_dependents {
                continue;
            }
            for dependent in dependents.get(&component_id).into_iter().flatten() {
                if seen.insert(dependent.clone()) {
                    propagated_batches
                        .entry(dependent.clone())
                        .or_default()
                        .push(Arc::clone(&dependency_batch));
                    pending.push(dependent.clone());
                }
            }
        }
    }
    Ok(AffectedReasonBatches {
        direct: direct_batches,
        propagated: propagated_batches,
    })
}

#[cfg(test)]
mod tests;
