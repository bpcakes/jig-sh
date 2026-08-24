use serde::Deserialize;

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct VaultConfig {
    #[serde(default)]
    pub(super) scope: VaultScopeConfig,
    #[serde(default)]
    pub(super) scope_id: Option<String>,
    #[serde(default)]
    allow_global: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub(super) enum VaultScopeConfig {
    #[default]
    Legacy,
    Repo,
}

impl VaultConfig {
    pub(crate) fn repo_scope_id(&self) -> Option<&str> {
        if self.scope == VaultScopeConfig::Repo {
            self.scope_id.as_deref()
        } else {
            None
        }
    }

    pub(crate) const fn allow_global(&self) -> bool {
        self.allow_global
    }
}
