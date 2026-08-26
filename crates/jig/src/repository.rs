// agentic-loc-exception: repository catalog normalization and validation remain together as one contract-authority boundary.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{Context, Result, bail};
use jig_contract::{
    ActionEffect, ActionId, ActionIntent, ActionRunner, ActionSpec, ComponentId, ComponentSpec,
    ManifestTool, ProfileId, ProfileSpec, TargetId, kind, tool,
};
use sha2::{Digest, Sha256};

use crate::context::{
    CommandTimeout, MAX_COMMAND_TIMEOUT_SECONDS, RepoContext, WorkEvidenceSelector,
};

pub(crate) use inspect::{
    CatalogInspection, InspectRequest, inspect_repository, inspect_repository_data,
};
pub(crate) use planner::{
    PlanRunRequest, plan_action_run, plan_run, target_input_digest,
    validate_current_repository_authority, validate_run_plan, validate_run_plan_source,
};

const NATIVE_REPOSITORY_CONTRACT_VERSION: u32 = 6;

#[derive(Clone, Debug, Eq, PartialEq)]
enum SelectorPart<T> {
    Any,
    Exact(T),
}

impl<T: AsRef<str>> SelectorPart<T> {
    fn matches(&self, value: &str) -> bool {
        match self {
            Self::Any => true,
            Self::Exact(expected) => expected.as_ref() == value,
        }
    }
}

/// A canonical repository selector. Legacy aliases deliberately live outside
/// this grammar so a raw string can never change canonical selection meaning.
#[derive(Clone, Debug, Eq, PartialEq)]
enum RepositorySelector {
    Action(SelectorPart<ActionId>),
    Target {
        component: SelectorPart<ComponentId>,
        action: SelectorPart<ActionId>,
    },
}

impl RepositorySelector {
    fn parse(value: &str) -> Result<Self> {
        if let Some((component, action)) = value.split_once(':') {
            if action.contains(':') {
                bail!("invalid target selector '{value}': expected component:action");
            }
            return Ok(Self::Target {
                component: parse_selector_part(component, |value| {
                    ComponentId::parse(value).map_err(Into::into)
                })?,
                action: parse_selector_part(action, |value| {
                    ActionId::parse(value).map_err(Into::into)
                })?,
            });
        }
        Ok(Self::Action(parse_selector_part(value, |value| {
            ActionId::parse(value).map_err(Into::into)
        })?))
    }

    fn matches(&self, target: &TargetId) -> bool {
        match self {
            Self::Action(action) => action.matches(target.action.as_str()),
            Self::Target { component, action } => {
                component.matches(target.component.as_str())
                    && action.matches(target.action.as_str())
            }
        }
    }
}

fn parse_selector_part<T>(
    value: &str,
    parse: impl FnOnce(String) -> Result<T>,
) -> Result<SelectorPart<T>> {
    if value == "*" {
        Ok(SelectorPart::Any)
    } else {
        Ok(SelectorPart::Exact(parse(value.to_owned())?))
    }
}

pub(crate) fn resolve_evidence_targets(
    catalog: &RepositoryCatalog,
    selector: &WorkEvidenceSelector,
) -> Result<BTreeSet<TargetId>> {
    let targets = match selector {
        WorkEvidenceSelector::Target(target) => {
            if catalog.action(target).is_none() {
                bail!("work evidence gate references unknown target '{target}'");
            }
            BTreeSet::from([target.clone()])
        }
        WorkEvidenceSelector::Profile(profile) => {
            let profile = catalog.profile(profile).ok_or_else(|| {
                anyhow::anyhow!("work evidence gate references unknown profile '{profile}'")
            })?;
            if profile.targets.is_empty() {
                bail!(
                    "work evidence gate profile '{}' contains no targets",
                    profile.id
                );
            }
            profile.targets.iter().cloned().collect()
        }
    };
    planner::validate_check_actions(catalog, targets.iter())?;
    Ok(targets)
}

pub(crate) fn validate_read_only_check_closure<'a, 'b>(
    actions: impl IntoIterator<Item = &'a ActionSpec>,
    targets: impl IntoIterator<Item = &'b TargetId>,
) -> Result<()> {
    let mut actions_by_target = BTreeMap::new();
    for action in actions {
        if actions_by_target
            .insert(action.target.clone(), action)
            .is_some()
        {
            bail!("duplicate target '{}'", action.target);
        }
    }

    let mut pending = targets.into_iter().cloned().collect::<Vec<_>>();
    let mut seen = BTreeSet::new();
    while let Some(target) = pending.pop() {
        if !seen.insert(target.clone()) {
            continue;
        }
        let action = actions_by_target
            .get(&target)
            .ok_or_else(|| anyhow::anyhow!("selected target '{target}' is not defined"))?;
        if action.intent != ActionIntent::Check
            || !action.effects.contains(&ActionEffect::ReadOnly)
            || action.effects.contains(&ActionEffect::Worktree)
            || action.effects.contains(&ActionEffect::External)
        {
            bail!(
                "target '{target}' is not a read-only check; use the action-specific command or plan and execute it through the MCP repository tools for {:?} actions with {:?} effects",
                action.intent,
                action.effects
            );
        }
        pending.extend(action.depends_on.iter().cloned());
    }

    let mut remaining = seen;
    let mut completed = BTreeSet::new();
    while !remaining.is_empty() {
        let ready = remaining
            .iter()
            .filter(|target| {
                actions_by_target
                    .get(*target)
                    .expect("check closure targets must remain defined")
                    .depends_on
                    .iter()
                    .all(|dependency| completed.contains(dependency))
            })
            .cloned()
            .collect::<Vec<_>>();
        if ready.is_empty() {
            bail!(
                "action dependency cycle among: {}",
                remaining
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        for target in ready {
            remaining.remove(&target);
            completed.insert(target);
        }
    }
    Ok(())
}

/// The one version-neutral repository view consumed by planning and inspection.
#[derive(Clone, Debug)]
pub(crate) struct RepositoryCatalog {
    contract_version: u32,
    config_digest: String,
    components: BTreeMap<ComponentId, ComponentSpec>,
    actions: BTreeMap<TargetId, ActionSpec>,
    profiles: BTreeMap<ProfileId, ProfileSpec>,
    default_check_profile: Option<ProfileId>,
    affected_ignore: Vec<String>,
    aliases: BTreeMap<String, TargetId>,
    target_aliases: BTreeMap<TargetId, Vec<String>>,
}

impl RepositoryCatalog {
    pub(crate) fn from_context(ctx: &RepoContext) -> Result<Self> {
        if ctx.contract_version() >= NATIVE_REPOSITORY_CONTRACT_VERSION {
            validate_action_working_directories(ctx.root(), ctx.action_specs())?;
            Self::from_native_with_ignore(
                ctx.contract_version(),
                ctx.contract_digest(),
                ctx.component_specs(),
                ctx.action_specs(),
                ctx.profile_specs(),
                ctx.default_check_profile(),
                ctx.affected_ignore(),
            )
        } else {
            Self::from_legacy(
                ctx.contract_version(),
                ctx.contract_digest(),
                ctx.tool_specs(),
                &ctx.work_check_tools(),
            )
        }
    }

    fn from_native_with_ignore(
        contract_version: u32,
        config_digest: &str,
        component_specs: &[ComponentSpec],
        action_specs: &[ActionSpec],
        profile_specs: &[ProfileSpec],
        default_check_profile: Option<&ProfileId>,
        affected_ignore: &[String],
    ) -> Result<Self> {
        if contract_version < NATIVE_REPOSITORY_CONTRACT_VERSION {
            bail!(
                "component-native repository records require contract version {NATIVE_REPOSITORY_CONTRACT_VERSION} or later"
            );
        }
        if component_specs.is_empty() {
            bail!("contract version {contract_version} requires at least one component");
        }
        if action_specs.is_empty() {
            bail!("contract version {contract_version} requires at least one action");
        }

        let mut components = BTreeMap::new();
        for component in component_specs {
            if component.id.as_str() == "repo" && component.root != "." {
                bail!("the reserved repo component must use root '.'");
            }
            if components
                .insert(component.id.clone(), component.clone())
                .is_some()
            {
                bail!("duplicate component id '{}'", component.id);
            }
        }
        validate_component_dependencies(&components)?;

        let mut actions = BTreeMap::new();
        let mut aliases = BTreeMap::new();
        let mut target_aliases = BTreeMap::new();
        for action in action_specs {
            if !components.contains_key(&action.target.component) {
                bail!(
                    "target '{}' references unknown component '{}'",
                    action.target,
                    action.target.component
                );
            }
            if action.effects.is_empty() {
                bail!(
                    "target '{}' must declare at least one effect so execution isolation and approval remain fail-closed",
                    action.target
                );
            }
            if action
                .timeout_seconds
                .is_some_and(|seconds| CommandTimeout::from_seconds(seconds).is_none())
            {
                bail!(
                    "target '{}' timeout_seconds must be between 1 and {MAX_COMMAND_TIMEOUT_SECONDS}",
                    action.target
                );
            }
            if action.timeout_seconds.is_some()
                && matches!(
                    &action.runner,
                    ActionRunner::Native { operation } if operation != tool::SCHEMA_CHECK
                )
            {
                bail!(
                    "target '{}' sets timeout_seconds for an in-process native runner that cannot be preempted safely; omit the override or use a supervised command/schema runner",
                    action.target
                );
            }
            if actions
                .insert(action.target.clone(), action.clone())
                .is_some()
            {
                bail!("duplicate target '{}'", action.target);
            }
            register_aliases(
                &mut aliases,
                &mut target_aliases,
                &action.target,
                &action.legacy_aliases,
            )?;
        }
        validate_action_dependencies(&actions)?;
        crate::context::native_migration_backend(component_specs, action_specs)?;
        affected::validate_native_path_policy(&components, &actions, affected_ignore)?;

        let profiles = collect_profiles(profile_specs, &actions)?;
        let default_check_profile = default_check_profile.cloned();
        validate_default_profile(default_check_profile.as_ref(), &profiles)?;

        Ok(Self {
            contract_version,
            config_digest: config_digest.to_owned(),
            components,
            actions,
            profiles,
            default_check_profile,
            affected_ignore: affected_ignore.to_vec(),
            aliases,
            target_aliases,
        })
    }

    #[cfg(test)]
    fn from_native(
        contract_version: u32,
        config_digest: &str,
        component_specs: &[ComponentSpec],
        action_specs: &[ActionSpec],
        profile_specs: &[ProfileSpec],
        default_check_profile: Option<&ProfileId>,
    ) -> Result<Self> {
        Self::from_native_with_ignore(
            contract_version,
            config_digest,
            component_specs,
            action_specs,
            profile_specs,
            default_check_profile,
            &[],
        )
    }

    fn from_legacy(
        contract_version: u32,
        config_digest: &str,
        manifest_tools: &[ManifestTool],
        configured_checks: &[String],
    ) -> Result<Self> {
        if contract_version >= NATIVE_REPOSITORY_CONTRACT_VERSION {
            bail!("legacy tool projection is only valid for contracts before version 6");
        }

        let repo_id = ComponentId::parse("repo").expect("repo is a valid static component id");
        let mut repo = ComponentSpec::new(repo_id.clone(), ".");
        repo.description = Some("Compatibility component for legacy repository tools.".into());
        repo.tags.push("legacy".into());
        repo.adapters.push("legacy-tools".into());

        let mut tools = manifest_tools.iter().collect::<Vec<_>>();
        tools.sort_by(|left, right| left.name.cmp(&right.name));
        let mut seen_aliases = BTreeSet::new();
        let mut occupied_ids = BTreeSet::new();
        let mut actions = BTreeMap::new();
        let mut aliases = BTreeMap::new();
        let mut target_aliases = BTreeMap::new();

        for manifest_tool in tools {
            if !seen_aliases.insert(manifest_tool.name.as_str()) {
                bail!("duplicate legacy tool name '{}'", manifest_tool.name);
            }
            let action_id = unique_legacy_action_id(&manifest_tool.name, &occupied_ids)?;
            occupied_ids.insert(action_id.clone());
            let target = TargetId::new(repo_id.clone(), action_id);
            let mut action = legacy_action(manifest_tool, target.clone())?;
            action.legacy_aliases.push(manifest_tool.name.clone());
            // Pre-v6 contracts may persist tool names that now parse as
            // canonical selectors. Preserve that public compatibility shape:
            // the planner resolves a real selector first and falls back to the
            // legacy alias only when it names no native target.
            aliases.insert(manifest_tool.name.clone(), target.clone());
            target_aliases.insert(target.clone(), vec![manifest_tool.name.clone()]);
            actions.insert(target, action);
        }

        let default_targets = legacy_default_targets(&actions, &aliases, configured_checks)?;
        let (profiles, default_check_profile) = if default_targets.is_empty() {
            (BTreeMap::new(), None)
        } else {
            let profile_id =
                ProfileId::parse("verify").expect("verify is a valid static profile id");
            let profile = ProfileSpec::new(profile_id.clone(), default_targets);
            (
                BTreeMap::from([(profile_id.clone(), profile)]),
                Some(profile_id),
            )
        };

        Ok(Self {
            contract_version,
            config_digest: config_digest.to_owned(),
            components: BTreeMap::from([(repo_id, repo)]),
            actions,
            profiles,
            default_check_profile,
            affected_ignore: Vec::new(),
            aliases,
            target_aliases,
        })
    }

    pub(crate) const fn contract_version(&self) -> u32 {
        self.contract_version
    }

    pub(crate) fn config_digest(&self) -> &str {
        &self.config_digest
    }

    pub(crate) fn components(&self) -> impl Iterator<Item = &ComponentSpec> {
        self.components.values()
    }

    pub(crate) fn component(&self, id: &ComponentId) -> Option<&ComponentSpec> {
        self.components.get(id)
    }

    pub(crate) fn actions(&self) -> impl Iterator<Item = &ActionSpec> {
        self.actions.values()
    }

    pub(crate) fn action(&self, target: &TargetId) -> Option<&ActionSpec> {
        self.actions.get(target)
    }

    pub(crate) fn profiles(&self) -> impl Iterator<Item = &ProfileSpec> {
        self.profiles.values()
    }

    pub(crate) fn profile(&self, id: &ProfileId) -> Option<&ProfileSpec> {
        self.profiles.get(id)
    }

    pub(crate) fn default_check_profile(&self) -> Option<&ProfileId> {
        self.default_check_profile.as_ref()
    }

    pub(crate) fn affected_ignore(&self) -> &[String] {
        &self.affected_ignore
    }

    pub(crate) fn target_for_alias(&self, alias: &str) -> Option<&TargetId> {
        self.aliases.get(alias)
    }

    pub(crate) fn action_for_alias(&self, alias: &str) -> Option<&ActionSpec> {
        self.target_for_alias(alias)
            .and_then(|target| self.action(target))
    }

    pub(crate) fn aliases_for_target(&self, target: &TargetId) -> &[String] {
        self.target_aliases.get(target).map_or(&[], Vec::as_slice)
    }
}

fn validate_action_working_directories(root: &Path, actions: &[ActionSpec]) -> Result<()> {
    for action in actions {
        if let ActionRunner::Command {
            working_directory: Some(working_directory),
            ..
        } = &action.runner
        {
            crate::repository_path::resolve_repository_working_directory(
                root,
                Some(working_directory),
            )
            .with_context(|| {
                format!(
                    "target '{}' has invalid working_directory {:?}",
                    action.target, working_directory
                )
            })?;
        }
    }
    Ok(())
}

fn validate_component_dependencies(
    components: &BTreeMap<ComponentId, ComponentSpec>,
) -> Result<()> {
    for component in components.values() {
        let mut seen = BTreeSet::new();
        for dependency in &component.depends_on {
            if dependency == &component.id {
                bail!("component '{}' cannot depend on itself", component.id);
            }
            if !components.contains_key(dependency) {
                bail!(
                    "component '{}' depends on unknown component '{}'",
                    component.id,
                    dependency
                );
            }
            if !seen.insert(dependency) {
                bail!(
                    "component '{}' repeats dependency '{}'",
                    component.id,
                    dependency
                );
            }
        }
    }
    validate_acyclic_dependencies("component", components, |component| &component.depends_on)
}

fn validate_action_dependencies(actions: &BTreeMap<TargetId, ActionSpec>) -> Result<()> {
    for action in actions.values() {
        let mut seen = BTreeSet::new();
        for dependency in &action.depends_on {
            if dependency == &action.target {
                bail!("target '{}' cannot depend on itself", action.target);
            }
            if !actions.contains_key(dependency) {
                bail!(
                    "target '{}' depends on unknown target '{}'",
                    action.target,
                    dependency
                );
            }
            if !seen.insert(dependency) {
                bail!(
                    "target '{}' repeats dependency '{}'",
                    action.target,
                    dependency
                );
            }
        }
    }
    validate_acyclic_dependencies("action", actions, |action| &action.depends_on)
}

fn validate_acyclic_dependencies<K, V>(
    kind: &str,
    records: &BTreeMap<K, V>,
    dependencies: impl Fn(&V) -> &[K],
) -> Result<()>
where
    K: Clone + Ord + ToString,
{
    let mut remaining = records.keys().cloned().collect::<BTreeSet<_>>();
    let mut completed = BTreeSet::new();
    while !remaining.is_empty() {
        let ready = remaining
            .iter()
            .filter(|id| {
                dependencies(
                    records
                        .get(*id)
                        .expect("dependency validation records must remain stable"),
                )
                .iter()
                .all(|dependency| completed.contains(dependency))
            })
            .cloned()
            .collect::<Vec<_>>();
        if ready.is_empty() {
            let mut current = remaining
                .iter()
                .next()
                .expect("non-empty dependency graph must have a first record")
                .clone();
            let mut path = Vec::new();
            let mut positions = BTreeMap::new();
            loop {
                if let Some(start) = positions.get(&current).copied() {
                    let mut cycle = path[start..]
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>();
                    cycle.push(current.to_string());
                    bail!("{kind} dependency cycle: {}", cycle.join(" -> "));
                }
                positions.insert(current.clone(), path.len());
                path.push(current.clone());
                current = dependencies(
                    records
                        .get(&current)
                        .expect("dependency validation records must remain stable"),
                )
                .iter()
                .find(|dependency| remaining.contains(*dependency))
                .expect("unresolved dependency records must point within the remaining graph")
                .clone();
            }
        }
        for id in ready {
            remaining.remove(&id);
            completed.insert(id);
        }
    }
    Ok(())
}

fn collect_profiles(
    profile_specs: &[ProfileSpec],
    actions: &BTreeMap<TargetId, ActionSpec>,
) -> Result<BTreeMap<ProfileId, ProfileSpec>> {
    let mut profiles = BTreeMap::new();
    for profile in profile_specs {
        if profile.targets.is_empty() {
            bail!("profile '{}' must select at least one target", profile.id);
        }
        let mut seen = BTreeSet::new();
        for target in &profile.targets {
            if !actions.contains_key(target) {
                bail!(
                    "profile '{}' references unknown target '{target}'",
                    profile.id
                );
            }
            if !seen.insert(target) {
                bail!("profile '{}' repeats target '{target}'", profile.id);
            }
        }
        if profiles
            .insert(profile.id.clone(), profile.clone())
            .is_some()
        {
            bail!("duplicate profile id '{}'", profile.id);
        }
    }
    Ok(profiles)
}

fn validate_default_profile(
    default_profile: Option<&ProfileId>,
    profiles: &BTreeMap<ProfileId, ProfileSpec>,
) -> Result<()> {
    let Some(default_profile) = default_profile else {
        bail!(
            "native repository contract version {NATIVE_REPOSITORY_CONTRACT_VERSION} or later requires default_check_profile"
        );
    };
    if !profiles.contains_key(default_profile) {
        bail!("default_check_profile '{default_profile}' does not name a profile");
    }
    Ok(())
}

fn register_aliases(
    aliases: &mut BTreeMap<String, TargetId>,
    target_aliases: &mut BTreeMap<TargetId, Vec<String>>,
    target: &TargetId,
    new_aliases: &[String],
) -> Result<()> {
    let mut local = BTreeSet::new();
    for alias in new_aliases {
        if alias.trim().is_empty() {
            bail!("target '{target}' declares an empty legacy alias");
        }
        if RepositorySelector::parse(alias).is_ok() {
            bail!(
                "legacy alias '{alias}' for target '{target}' is reserved for canonical repository selectors"
            );
        }
        if !local.insert(alias) {
            bail!("target '{target}' repeats legacy alias '{alias}'");
        }
        if let Some(existing) = aliases.insert(alias.clone(), target.clone()) {
            bail!("legacy alias '{alias}' maps to both '{existing}' and '{target}'");
        }
    }
    if !new_aliases.is_empty() {
        target_aliases.insert(target.clone(), new_aliases.to_vec());
    }
    Ok(())
}

fn legacy_action(tool: &ManifestTool, target: TargetId) -> Result<ActionSpec> {
    let (intent, effects) = match tool.name.as_str() {
        tool::BOOTSTRAP => (
            ActionIntent::Operate,
            vec![
                ActionEffect::Worktree,
                ActionEffect::Process,
                ActionEffect::External,
            ],
        ),
        tool::MIGRATION_ADD | tool::SCHEMA_DUMP => (
            ActionIntent::Generate,
            vec![ActionEffect::Worktree, ActionEffect::Process],
        ),
        _ => (
            ActionIntent::Check,
            vec![ActionEffect::ReadOnly, ActionEffect::Process],
        ),
    };
    let runner = match tool.kind.as_str() {
        kind::COMMAND => ActionRunner::command(tool.command.as_deref().ok_or_else(|| {
            anyhow::anyhow!("legacy command tool '{}' has no command key", tool.name)
        })?),
        kind::NATIVE => ActionRunner::native(&tool.name),
        other => bail!("legacy tool '{}' has unsupported kind '{other}'", tool.name),
    };
    let mut action = ActionSpec::new(target, intent, runner);
    action.description = Some(tool.description.clone());
    action.effects = effects;
    Ok(action)
}

fn legacy_default_targets(
    actions: &BTreeMap<TargetId, ActionSpec>,
    aliases: &BTreeMap<String, TargetId>,
    configured_checks: &[String],
) -> Result<Vec<TargetId>> {
    if !configured_checks.is_empty() {
        let mut targets = BTreeSet::new();
        for alias in configured_checks {
            let Some(target) = aliases.get(alias) else {
                bail!("configured legacy check tool '{alias}' has no repository target");
            };
            let action = &actions[target];
            if action.intent != ActionIntent::Check
                || !action.effects.contains(&ActionEffect::ReadOnly)
            {
                // Legacy work gates historically included `jig.schema_dump`
                // under kind = "check". Keep that one known compatibility
                // action addressable, but never silently discard a different
                // configured target whose effects cannot run in verification.
                if alias == tool::SCHEMA_DUMP {
                    continue;
                }
                bail!(
                    "configured legacy check tool '{alias}' is not a read-only check and cannot be included in the default verification profile"
                );
            }
            targets.insert(target.clone());
        }
        return Ok(targets.into_iter().collect());
    }

    Ok(actions
        .values()
        .filter(|action| {
            action.intent == ActionIntent::Check
                && action.effects.contains(&ActionEffect::ReadOnly)
                && !action
                    .legacy_aliases
                    .iter()
                    .any(|alias| alias == tool::TEST_LOCKED)
        })
        .map(|action| action.target.clone())
        .collect())
}

fn unique_legacy_action_id(tool_name: &str, occupied: &BTreeSet<ActionId>) -> Result<ActionId> {
    let base = known_legacy_action_id(tool_name)
        .map_or_else(|| sanitize_legacy_action_id(tool_name), str::to_owned);
    let candidate = ActionId::parse(&base)?;
    if !occupied.contains(&candidate) {
        return Ok(candidate);
    }

    let digest = full_digest(tool_name);
    for suffix_len in (12..=60).step_by(4) {
        let suffix = &digest[..suffix_len];
        let max_base_len = 64 - suffix.len() - 1;
        let shortened = base
            .get(..base.len().min(max_base_len))
            .unwrap_or(&base)
            .trim_end_matches(['-', '_']);
        let candidate = ActionId::parse(format!("{shortened}-{suffix}"))?;
        if !occupied.contains(&candidate) {
            return Ok(candidate);
        }
    }

    bail!("legacy tool '{tool_name}' exhausted deterministic action-id collision fallbacks")
}

fn known_legacy_action_id(tool_name: &str) -> Option<&'static str> {
    match tool_name {
        tool::FMT_CHECK => Some("fmt"),
        tool::CONTRACT_CHECK => Some("contract"),
        tool::TEST_LOCKED => Some("test-locked"),
        tool::TYPESCRIPT_BUILD => Some("typescript-build"),
        tool::TYPESCRIPT_COVERAGE => Some("typescript-coverage"),
        tool::TYPESCRIPT_LINT => Some("typescript-lint"),
        tool::TYPESCRIPT_TYPECHECK => Some("typescript-typecheck"),
        tool::SCHEMA_CHECK => Some("schema"),
        tool::SCHEMA_DUMP => Some("schema-dump"),
        tool::SQLX_CHECK => Some("sqlx"),
        tool::SQLC_CHECK => Some("sqlc"),
        tool::MIGRATION_ADD => Some("migration-add"),
        tool::AGENT_DOCTOR => Some("agent-doctor"),
        tool::BOOTSTRAP => Some("bootstrap"),
        tool::CLIPPY => Some("clippy"),
        tool::LINT => Some("lint"),
        tool::TEST => Some("test"),
        _ => None,
    }
}

fn sanitize_legacy_action_id(tool_name: &str) -> String {
    let source = tool_name.strip_prefix("jig.").unwrap_or(tool_name);
    let mut value = String::new();
    let mut last_was_separator = false;
    for character in source.chars() {
        let normalized = character.to_ascii_lowercase();
        if normalized.is_ascii_lowercase() || normalized.is_ascii_digit() {
            value.push(normalized);
            last_was_separator = false;
        } else if !last_was_separator && !value.is_empty() {
            value.push('-');
            last_was_separator = true;
        }
    }
    let value = value.trim_matches('-');
    if value.is_empty() {
        return format!("tool-{}", short_digest(tool_name));
    }
    if value.len() <= 64 {
        return value.to_owned();
    }
    let suffix = short_digest(tool_name);
    let prefix = value
        .get(..64 - suffix.len() - 1)
        .unwrap_or(value)
        .trim_end_matches('-');
    format!("{prefix}-{suffix}")
}

fn short_digest(value: &str) -> String {
    full_digest(value)[..12].to_owned()
}

fn full_digest(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

#[cfg(test)]
mod tests;

mod affected;
mod inspect;
mod planner;
