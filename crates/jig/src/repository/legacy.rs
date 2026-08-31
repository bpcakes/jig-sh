use super::*;

pub(super) fn legacy_action(tool: &ManifestTool, target: TargetId) -> Result<ActionSpec> {
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

pub(super) fn legacy_default_targets(
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

pub(super) fn unique_legacy_action_id(
    tool_name: &str,
    occupied: &BTreeSet<ActionId>,
) -> Result<ActionId> {
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

pub(super) fn known_legacy_action_id(tool_name: &str) -> Option<&'static str> {
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

pub(super) fn sanitize_legacy_action_id(tool_name: &str) -> String {
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

pub(super) fn short_digest(value: &str) -> String {
    full_digest(value)[..12].to_owned()
}

pub(super) fn full_digest(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}
