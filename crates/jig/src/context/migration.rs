use clap::ValueEnum;
use serde::{Deserialize, Serialize};

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
