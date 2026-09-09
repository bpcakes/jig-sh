use super::*;

pub(super) struct LoadedConfig {
    pub(super) config: RepoConfig,
    pub(super) content_digest: String,
}

pub(super) fn load_config_snapshot(config_path: &Path) -> Result<LoadedConfig> {
    let config_text = fs::read_to_string(config_path)
        .with_context(|| format!("Failed to read {}", config_path.display()))?;
    let config: RepoConfig = toml::from_str(&config_text).with_context(|| {
        format!(
            "Failed to parse {}. Jig rejects unknown .jig.toml keys during upgrades; remove typos or experimental keys and retry.",
            config_path.display()
        )
    })?;
    validate_config(&config)?;
    Ok(LoadedConfig {
        config,
        content_digest: format!("sha256:{:x}", Sha256::digest(config_text.as_bytes())),
    })
}

pub(super) fn load_config(config_path: &Path) -> Result<RepoConfig> {
    Ok(load_config_snapshot(config_path)?.config)
}
