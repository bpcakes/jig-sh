pub(crate) const DEFAULT_CODEX_MARKETPLACE_ID: &str = "jig-skills";
// jig.sh generated repos default to the shared Jig skills marketplace; forks can
// override or opt out through agent_tooling.codex.marketplaces in .jig.toml.
pub(crate) const DEFAULT_CODEX_MARKETPLACE_SOURCE: &str = "bpcakes/jig-skills";
pub(crate) const DEFAULT_CODEX_MARKETPLACE_PLUGINS: &[&str] = &[
    "jig-rust@jig-skills",
    "jig-swift@jig-skills",
    "jig-typescript@jig-skills",
    "jig-exec-plans@jig-skills",
];
pub(crate) const SUPPORTED_WEB_PACKAGE_MANAGERS: &[&str] = &["bun", "npm", "pnpm", "yarn"];
