use super::*;

#[cfg_attr(not(feature = "dev-proxy"), allow(dead_code))]
pub(super) fn find_optional_repo_root() -> Result<Option<PathBuf>> {
    find_optional_repo_root_from(&std::env::current_dir()?)
}

pub(crate) fn find_repo_root_from(start: &Path) -> Result<PathBuf> {
    find_optional_repo_root_from(start)?.context(REPO_CONTEXT_NOT_FOUND)
}

pub(crate) fn find_repo_root_from_or_env(start: &Path) -> Result<PathBuf> {
    if let Some(root) = repo_root_from_env()? {
        Ok(root)
    } else {
        find_repo_root_from(start)
    }
}

pub(super) fn repo_root_from_env() -> Result<Option<PathBuf>> {
    let Some(root) = std::env::var_os(JIG_REPO_ROOT_ENV) else {
        return Ok(None);
    };
    let root = PathBuf::from(root);
    if root.as_os_str().is_empty() {
        return Ok(None);
    }
    if !root.join(".jig.toml").exists() {
        bail!(
            "{JIG_REPO_ROOT_ENV} does not contain .jig.toml: {}",
            root.display()
        );
    }
    fs::canonicalize(&root)
        .with_context(|| {
            format!(
                "Failed to canonicalize {JIG_REPO_ROOT_ENV}: {}",
                root.display()
            )
        })
        .map(Some)
}

fn find_optional_repo_root_from(start: &Path) -> Result<Option<PathBuf>> {
    let mut current = start.to_path_buf();
    loop {
        if current.join(".jig.toml").exists() {
            return Ok(Some(current));
        }
        if !current.pop() {
            return Ok(None);
        }
    }
}

#[cfg(test)]
impl RepoContext {
    pub(crate) fn load_from(root: &Path) -> Result<Self> {
        let config_path = root.join(".jig.toml");
        let loaded_config = load_config_snapshot(&config_path)?;
        let manifest_text = fs::read_to_string(root.join(".agent/jig-contract.json"))?;
        let manifest_authority: serde_json::Value = serde_json::from_str(&manifest_text)?;
        let manifest: ContractManifest = serde_json::from_value(manifest_authority.clone())?;
        let contract_digest = contract_source_digest(&loaded_config.config, &manifest_authority)?;
        Ok(Self {
            root: root.to_path_buf(),
            config: loaded_config.config,
            manifest,
            contract_digest,
            configuration_content_digests: [
                loaded_config.content_digest,
                format!("sha256:{:x}", Sha256::digest(manifest_text.as_bytes())),
            ],
        })
    }
}
