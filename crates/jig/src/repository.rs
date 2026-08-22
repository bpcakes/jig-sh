use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, bail};
use jig_contract::{
    ActionEffect, ActionId, ActionIntent, ActionRunner, ActionSpec, ComponentId, ComponentSpec,
    ManifestTool, ProfileId, ProfileSpec, TargetId, kind, tool,
};
use sha2::{Digest, Sha256};

use crate::context::{RepoContext, WorkEvidenceSelector};

mod affected;
mod inspect;
mod planner;

pub(crate) use inspect::{
    CatalogInspection, InspectRequest, inspect_repository, inspect_repository_data,
};
pub(crate) use planner::{
    PlanRunRequest, plan_action_run, plan_run, target_input_digest, validate_run_plan,
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
    match selector {
        WorkEvidenceSelector::Target(target) => {
            if catalog.action(target).is_none() {
                bail!("work evidence gate references unknown target '{target}'");
            }
            Ok(BTreeSet::from([target.clone()]))
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
            Ok(profile.targets.iter().cloned().collect())
        }
    }
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
    aliases: BTreeMap<String, TargetId>,
    target_aliases: BTreeMap<TargetId, Vec<String>>,
}

impl RepositoryCatalog {
    pub(crate) fn from_context(ctx: &RepoContext) -> Result<Self> {
        if ctx.contract_version() >= NATIVE_REPOSITORY_CONTRACT_VERSION {
            Self::from_native(
                ctx.contract_version(),
                ctx.contract_digest(),
                ctx.component_specs(),
                ctx.action_specs(),
                ctx.profile_specs(),
                ctx.default_check_profile(),
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

    fn from_native(
        contract_version: u32,
        config_digest: &str,
        component_specs: &[ComponentSpec],
        action_specs: &[ActionSpec],
        profile_specs: &[ProfileSpec],
        default_check_profile: Option<&ProfileId>,
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
        affected::validate_native_path_policy(&components, &actions)?;

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
            aliases,
            target_aliases,
        })
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

    pub(crate) fn target_for_alias(&self, alias: &str) -> Option<&TargetId> {
        self.aliases.get(alias)
    }

    pub(crate) fn aliases_for_target(&self, target: &TargetId) -> &[String] {
        self.target_aliases.get(target).map_or(&[], Vec::as_slice)
    }
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
    Ok(())
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
        bail!("contract version 6 requires default_check_profile");
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
                bail!("configured legacy check tool '{alias}' is not a read-only check");
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

    let suffix = short_digest(tool_name);
    let max_base_len = 64 - suffix.len() - 1;
    let shortened = base
        .get(..base.len().min(max_base_len))
        .unwrap_or(&base)
        .trim_end_matches(['-', '_']);
    Ok(ActionId::parse(format!("{shortened}-{suffix}"))?)
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
    let digest = Sha256::digest(value.as_bytes());
    format!("{digest:x}")[..12].to_owned()
}

#[cfg(test)]
mod tests {
    use jig_contract::{
        ActionEffect, ActionIntent, ActionRunner, ActionSpec, ComponentId, ComponentSpec,
        ManifestTool, ProfileId, ProfileSpec, TargetId, kind,
    };
    use tempfile::tempdir;

    use crate::{context::RepoContext, test_env::TestRepoBuilder};

    use super::RepositoryCatalog;

    fn command_tool(name: &str, command: &str) -> ManifestTool {
        ManifestTool::new(name, kind::COMMAND, format!("Run {name}.")).with_command(command)
    }

    #[test]
    fn legacy_tools_are_repo_targets_with_bidirectional_aliases() {
        let temp = tempdir().unwrap();
        TestRepoBuilder::new(temp.path())
            .contract_version(5)
            .config(
                r#"
[commands]
go_test_command = "go test ./..."

[work]
checks = ["jig.test"]
"#,
            )
            .tool(serde_json::to_value(command_tool("jig.test", "go_test_command")).unwrap())
            .write();
        let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

        let catalog = RepositoryCatalog::from_context(&ctx).unwrap();
        let target: TargetId = "repo:test".parse().unwrap();
        assert_eq!(catalog.contract_version(), 5);
        assert_eq!(catalog.components().count(), 1);
        assert_eq!(catalog.target_for_alias("jig.test"), Some(&target));
        assert_eq!(catalog.aliases_for_target(&target), ["jig.test"]);
        assert_eq!(catalog.default_check_profile().unwrap().as_str(), "verify");
        assert_eq!(
            catalog
                .profile(catalog.default_check_profile().unwrap())
                .unwrap()
                .targets,
            [target]
        );
        assert!(catalog.config_digest().starts_with("sha256:"));
    }

    #[test]
    fn legacy_alias_collisions_get_order_independent_target_ids() {
        let tools = [
            command_tool("jig.foo_bar", "foo_command"),
            command_tool("jig.foo-bar", "bar_command"),
        ];
        let reversed = [tools[1].clone(), tools[0].clone()];

        let first = RepositoryCatalog::from_legacy(5, "digest", &tools, &[]).unwrap();
        let second = RepositoryCatalog::from_legacy(5, "digest", &reversed, &[]).unwrap();
        assert_eq!(
            first.target_for_alias("jig.foo_bar"),
            second.target_for_alias("jig.foo_bar")
        );
        assert_ne!(
            first.target_for_alias("jig.foo_bar"),
            first.target_for_alias("jig.foo-bar")
        );
    }

    #[test]
    fn native_catalog_keeps_same_action_distinct_per_component() {
        let api = ComponentSpec::new(ComponentId::parse("api").unwrap(), "api");
        let web = ComponentSpec::new(ComponentId::parse("web").unwrap(), "web");
        let api_target: TargetId = "api:test".parse().unwrap();
        let web_target: TargetId = "web:test".parse().unwrap();
        let mut api_test = ActionSpec::new(
            api_target.clone(),
            ActionIntent::Check,
            ActionRunner::command("go_test_command"),
        );
        api_test.effects = vec![ActionEffect::ReadOnly, ActionEffect::Process];
        api_test.legacy_aliases.push("jig.test".into());
        let mut web_test = ActionSpec::new(
            web_target.clone(),
            ActionIntent::Check,
            ActionRunner::command("typescript_test_command"),
        );
        web_test.effects = vec![ActionEffect::ReadOnly, ActionEffect::Process];
        let profile_id = ProfileId::parse("verify").unwrap();
        let profile = ProfileSpec::new(
            profile_id.clone(),
            vec![api_target.clone(), web_target.clone()],
        );

        let catalog = RepositoryCatalog::from_native(
            6,
            "sha256:config",
            &[api, web],
            &[api_test, web_test],
            &[profile],
            Some(&profile_id),
        )
        .unwrap();

        assert!(catalog.action(&api_target).is_some());
        assert!(catalog.action(&web_target).is_some());
        assert_eq!(catalog.actions().count(), 2);
        assert_eq!(catalog.target_for_alias("jig.test"), Some(&api_target));
    }

    #[test]
    fn native_catalog_rejects_aliases_that_are_canonical_selectors() {
        for alias in ["test", "web:test", "*:test", "web:*"] {
            let component = ComponentSpec::new(ComponentId::parse("web").unwrap(), "web");
            let target: TargetId = "web:lint".parse().unwrap();
            let mut action = ActionSpec::new(
                target.clone(),
                ActionIntent::Check,
                ActionRunner::command("typescript_lint_command"),
            );
            action.effects = vec![ActionEffect::ReadOnly, ActionEffect::Process];
            action.legacy_aliases.push(alias.into());
            let profile_id = ProfileId::parse("verify").unwrap();
            let profile = ProfileSpec::new(profile_id.clone(), vec![target]);

            let error = RepositoryCatalog::from_native(
                6,
                "sha256:config",
                &[component],
                &[action],
                &[profile],
                Some(&profile_id),
            )
            .unwrap_err()
            .to_string();

            assert!(
                error.contains("reserved for canonical repository selectors"),
                "alias {alias:?} was accepted: {error}"
            );
        }
    }

    #[test]
    fn native_catalog_rejects_cross_record_drift() {
        let repo = ComponentSpec::new(ComponentId::parse("repo").unwrap(), ".");
        let missing_target: TargetId = "missing:test".parse().unwrap();
        let action = ActionSpec::new(
            missing_target,
            ActionIntent::Check,
            ActionRunner::native("contract"),
        );
        let profile = ProfileSpec::new(
            ProfileId::parse("verify").unwrap(),
            vec!["missing:test".parse().unwrap()],
        );

        let error = RepositoryCatalog::from_native(
            6,
            "digest",
            &[repo],
            &[action],
            &[profile],
            Some(&ProfileId::parse("verify").unwrap()),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("unknown component 'missing'"));
    }
}
