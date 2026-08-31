use anyhow::{Result, bail};
use clap::ValueEnum;
use jig_contract::{ActionEffect, ActionIntent, ActionRunner, ActionSpec, ComponentSpec, tool};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MigrationBackend {
    Sqlx,
    Goose,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
#[value(rename_all = "snake_case")]
pub(crate) enum RustMigrationLayout {
    #[default]
    FlatMigrations,
    VersionedArtifacts,
}

impl RustMigrationLayout {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::FlatMigrations => "flat_migrations",
            Self::VersionedArtifacts => "versioned_artifacts",
        }
    }

    pub(crate) const fn allows_migration_add(self) -> bool {
        matches!(self, Self::FlatMigrations)
    }
}

pub(crate) fn native_migration_backend(
    components: &[ComponentSpec],
    actions: &[ActionSpec],
) -> Result<Option<MigrationBackend>> {
    let migration_actions = actions
        .iter()
        .filter(|action| {
            matches!(
                &action.runner,
                ActionRunner::Native { operation, .. } if operation == tool::MIGRATION_ADD
            )
        })
        .collect::<Vec<_>>();
    let [action] = migration_actions.as_slice() else {
        if migration_actions.is_empty() {
            return Ok(None);
        }
        bail!(
            "repository declares multiple migration authoring targets ({}), but the repository-wide migration_dir requires a single authoring owner; keep one native [[repository.actions]] migration-add entry",
            migration_actions
                .iter()
                .map(|action| action.target.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );
    };
    if action.intent != ActionIntent::Generate {
        bail!(
            "native migration authoring target '{}' must declare intent 'generate'",
            action.target
        );
    }
    if !action.effects.contains(&ActionEffect::Worktree) {
        bail!(
            "native migration authoring target '{}' must declare the 'worktree' effect",
            action.target
        );
    }
    let component = components
        .iter()
        .find(|component| component.id == action.target.component)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "migration authoring target '{}' references unknown component '{}'",
                action.target,
                action.target.component
            )
        })?;
    let sqlx = component.adapters.iter().any(|adapter| adapter == "sqlx");
    let goose = component
        .adapters
        .iter()
        .any(|adapter| adapter == "go-postgres");
    match (sqlx, goose) {
        (true, false) => Ok(Some(MigrationBackend::Sqlx)),
        (false, true) => Ok(Some(MigrationBackend::Goose)),
        (true, true) => bail!(
            "migration authoring target '{}' belongs to component '{}' with both sqlx and go-postgres adapters; choose one migration format owner",
            action.target,
            component.id
        ),
        (false, false) => bail!(
            "migration authoring target '{}' belongs to component '{}' without a sqlx or go-postgres adapter",
            action.target,
            component.id
        ),
    }
}
