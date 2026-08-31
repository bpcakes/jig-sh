use std::collections::BTreeMap;

use anyhow::{Result, bail};
use jig_contract::{ActionRunner, TargetId, tool};

use crate::context::RepoContext;
use crate::repository_path::validate_runner_environment;

const LEGACY_SCHEMA_DUMP_COMMAND: &str = "schema_dump_command";
const SCHEMA_DUMP_ACTION: &str = "schema-dump";

/// The command-backed action whose behavior the native schema freshness check
/// must reproduce inside its disposable repository snapshot.
pub(super) struct SchemaDumpRunner<'a> {
    pub(super) command_key: &'a str,
    pub(super) command_text: &'a str,
    pub(super) working_directory: Option<&'a str>,
    pub(super) environment: Option<&'a BTreeMap<String, String>>,
}

pub(super) fn resolve<'a>(
    ctx: &'a RepoContext,
    schema_check_target: Option<&TargetId>,
) -> Result<SchemaDumpRunner<'a>> {
    if ctx.contract_version() < 6 {
        return Ok(SchemaDumpRunner {
            command_key: LEGACY_SCHEMA_DUMP_COMMAND,
            command_text: ctx.command_for_key(LEGACY_SCHEMA_DUMP_COMMAND)?,
            working_directory: None,
            environment: None,
        });
    }

    let schema_check = resolve_schema_check_action(ctx, schema_check_target)?;
    let schema_dump = ctx
        .action_specs()
        .iter()
        .find(|action| {
            action.target.component == schema_check.target.component
                && action.target.action.as_str() == SCHEMA_DUMP_ACTION
        })
        .ok_or_else(|| {
            anyhow::anyhow!(
                "native schema target '{}' has no owning '{}:{}' action",
                schema_check.target,
                schema_check.target.component,
                SCHEMA_DUMP_ACTION
            )
        })?;
    let ActionRunner::Command {
        command,
        working_directory,
        environment,
    } = &schema_dump.runner
    else {
        bail!(
            "owning schema-dump target '{}' must use a command runner",
            schema_dump.target
        );
    };
    validate_runner_environment(environment)?;

    Ok(SchemaDumpRunner {
        command_key: command,
        command_text: ctx.command_for_key(command)?,
        working_directory: working_directory.as_deref(),
        environment: Some(environment),
    })
}

fn resolve_schema_check_action<'a>(
    ctx: &'a RepoContext,
    target: Option<&TargetId>,
) -> Result<&'a jig_contract::ActionSpec> {
    if let Some(target) = target {
        let action = ctx
            .action_specs()
            .iter()
            .find(|action| &action.target == target)
            .ok_or_else(|| anyhow::anyhow!("native schema target '{target}' is not declared"))?;
        if !matches!(
            &action.runner,
            ActionRunner::Native { operation, .. } if operation == tool::SCHEMA_CHECK
        ) {
            bail!(
                "target '{}' does not use the native schema-check runner",
                action.target
            );
        }
        return Ok(action);
    }

    let mut matches = ctx.action_specs().iter().filter(|action| {
        matches!(
            &action.runner,
            ActionRunner::Native { operation, .. } if operation == tool::SCHEMA_CHECK
        )
    });
    let Some(action) = matches.next() else {
        bail!("contract v6 repository has no native schema-check target");
    };
    if matches.next().is_some() {
        bail!(
            "contract v6 repository has multiple native schema-check targets; execute a canonical component:action target"
        );
    }
    Ok(action)
}
