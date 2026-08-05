macro_rules! define_reason_codes {
    ($($variant:ident => $value:literal),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub(super) enum ReasonCode {
            $($variant),+
        }

        impl ReasonCode {
            pub(super) const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $value),+
                }
            }
        }

        #[cfg(test)]
        pub(super) const ALL: &[ReasonCode] = &[$(ReasonCode::$variant),+];
    };
}

define_reason_codes! {
    AgentReadinessUnknown => "agent_readiness_unknown",
    BootstrapToolInvalid => "bootstrap_tool_invalid",
    BootstrapToolMissing => "bootstrap_tool_missing",
    CodexMarketplaceSupportUnavailable => "codex_marketplace_support_unavailable",
    CodexMarketplaceUnregistered => "codex_marketplace_unregistered",
    DevAppsNotConfigured => "dev_apps_not_configured",
    DevProxyFeatureNotBuilt => "dev_proxy_feature_not_built",
    MigrationAddToolInvalid => "migration_add_tool_invalid",
    MigrationAddToolMissing => "migration_add_tool_missing",
    MigrationDirectoryNotConfigured => "migration_directory_not_configured",
    RepoContextUnavailable => "repo_context_unavailable",
    SqlxDisabled => "sqlx_disabled",
    VaultNotInitialized => "vault_not_initialized",
    VaultStatusUnavailable => "vault_status_unavailable",
}
