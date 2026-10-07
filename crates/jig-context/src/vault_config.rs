use serde::Deserialize;

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VaultConfig {
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
    pub fn repo_scope_id(&self) -> Option<&str> {
        if self.scope == VaultScopeConfig::Repo {
            self.scope_id.as_deref()
        } else {
            None
        }
    }

    pub const fn allow_global(&self) -> bool {
        self.allow_global
    }
}

pub fn is_valid_vault_scope_id(scope_id: &str) -> bool {
    !scope_id.is_empty()
        && scope_id.len() <= 128
        && scope_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

#[cfg(test)]
mod tests {
    use super::is_valid_vault_scope_id;

    #[test]
    fn vault_scope_id_validator_rejects_path_and_length_boundaries() {
        assert!(is_valid_vault_scope_id("abc_123-XYZ"));
        assert!(!is_valid_vault_scope_id(""));
        assert!(!is_valid_vault_scope_id("../shared"));
        assert!(!is_valid_vault_scope_id("scope/child"));
        assert!(!is_valid_vault_scope_id(&"a".repeat(129)));
    }
}
