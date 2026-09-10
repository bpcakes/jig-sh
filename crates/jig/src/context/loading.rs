use super::*;

impl RepoContext {
    pub(crate) fn reload_execution_authority(&self) -> Result<Self> {
        Self::load_from_root_with_development_epoch(
            self.root().to_path_buf(),
            cfg!(test)
                && self.contract_version()
                    == jig_contract::freshness::TARGET_FRESHNESS_CONTRACT_VERSION,
        )
    }
    pub(crate) fn load_from_root(root: PathBuf) -> Result<Self> {
        Self::load_from_root_with_development_epoch(root, false)
    }

    #[cfg(test)]
    pub(crate) fn load_freshness_fixture(root: PathBuf) -> Result<Self> {
        Self::load_from_root_with_development_epoch(root, true)
    }

    fn load_from_root_with_development_epoch(
        root: PathBuf,
        development_fixture: bool,
    ) -> Result<Self> {
        let config_path = root.join(".jig.toml");
        let loaded_config = load_config_snapshot(&config_path)?;

        let manifest_path = root.join(".agent/jig-contract.json");
        let manifest_text = fs::read_to_string(&manifest_path)
            .with_context(|| format!("Failed to read {}", manifest_path.display()))?;
        let manifest_authority: serde_json::Value =
            crate::strict_json::from_slice(manifest_text.as_bytes())
                .with_context(|| format!("Failed to parse {}", manifest_path.display()))?;
        let manifest: ContractManifest = serde_json::from_value(manifest_authority.clone())
            .with_context(|| format!("Failed to parse {}", manifest_path.display()))?;
        let config = loaded_config.config;
        let contract_digest = contract_source_digest(&config, &manifest_authority)?;
        let configuration_content_digests = [
            loaded_config.content_digest,
            format!("sha256:{:x}", Sha256::digest(manifest_text.as_bytes())),
        ];

        if !is_supported_contract_version(manifest.contract_version)
            && !(development_fixture
                && manifest.contract_version
                    == jig_contract::freshness::TARGET_FRESHNESS_CONTRACT_VERSION)
        {
            bail!(
                "Unsupported jig contract version: {}",
                manifest.contract_version
            );
        }
        if manifest.tool_namespace != "jig" {
            bail!("Unsupported tool namespace: {}", manifest.tool_namespace);
        }
        validate_repository_source(&config, &manifest)?;
        // Legacy contracts are command-backed. A native-only v6 repository is
        // valid because action runners, rather than a global command list,
        // define its executable surface.
        if manifest.contract_version <= 5 && manifest.required_commands.is_empty() {
            bail!("jig contract manifest does not declare required commands");
        }
        if manifest.contract_version <= LAST_VERSION_LOCKED_CONTRACT_VERSION {
            let config_version =
                non_empty_legacy_jig_version(config.jig_version.as_deref(), ".jig.toml")?;
            let manifest_version = non_empty_legacy_jig_version(
                manifest.jig_version.as_deref(),
                ".agent/jig-contract.json",
            )?;
            if config_version != manifest_version {
                bail!(
                    "jig version mismatch between .jig.toml ({config_version}) and manifest ({manifest_version})"
                );
            }
        }

        let current_session_path = resolve_current_session_path(&root);

        Ok(Self {
            root,
            current_session_path,
            config,
            manifest,
            contract_digest,
            configuration_content_digests,
        })
    }

    pub(crate) fn supported_contract_version_from_root(root: &Path) -> Result<u32> {
        let contract_version = Self::declared_contract_version_from_root(root)?;
        if !is_supported_contract_version(contract_version) {
            bail!("Unsupported jig contract version: {contract_version}");
        }
        Ok(contract_version)
    }

    pub(crate) fn validate_config_file(root: &Path) -> Result<RepoConfigProbe> {
        let config = load_config(&root.join(".jig.toml"))?;
        Ok(RepoConfigProbe {
            repo_name: config.repo_name,
            jig_version: config.jig_version,
        })
    }
}
