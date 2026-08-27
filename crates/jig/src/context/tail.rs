pub(crate) fn is_reserved_git_metadata_component(component: &str) -> bool {
    is_hfs_component_alias(component, ['.', 'g', 'i', 't'])
}

fn is_reserved_agent_state_component(component: &str) -> bool {
    is_hfs_component_alias(component, ['.', 'a', 'g', 'e', 'n', 't'])
}

// Behavioral reference only; this is an independent Rust implementation of Git's
// HFS protection pinned at f60db8d575adb79761d363e026fb49bddf330c73:
// https://github.com/git/git/blob/f60db8d575adb79761d363e026fb49bddf330c73/utf8.c#L698-L787
fn is_hfs_component_alias<const N: usize>(component: &str, expected: [char; N]) -> bool {
    let mut normalized = component
        .chars()
        .filter(|character| !is_hfs_ignored(*character));
    expected.into_iter().all(|expected| {
        normalized
            .next()
            .is_some_and(|actual| actual.eq_ignore_ascii_case(&expected))
    }) && normalized.next().is_none()
}

const fn is_hfs_ignored(character: char) -> bool {
    matches!(
        character,
        '\u{200c}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{206a}'..='\u{206f}' | '\u{feff}'
    )
}

fn is_supported_frontend_kind(kind: &str) -> bool {
    matches!(kind, "vite" | "env-port")
}

fn validate_dev_app_env_prefixes<'a>(
    names: impl IntoIterator<Item = &'a str>,
    section: &str,
) -> Result<()> {
    let mut prefixes = BTreeMap::new();
    for name in names {
        let prefix = jig_core::dev_app_env_prefix(name);
        if let Some(previous) = prefixes.insert(prefix.clone(), name) {
            bail!(
                "{section} entries '{previous}' and '{name}' share derived dev environment prefix {prefix}; rename one app so punctuation-normalized names are unique"
            );
        }
    }
    Ok(())
}

#[cfg_attr(not(feature = "dev-proxy"), allow(dead_code))]
fn find_optional_repo_root() -> Result<Option<PathBuf>> {
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

fn repo_root_from_env() -> Result<Option<PathBuf>> {
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

fn resolve_current_session_path(root: &Path) -> PathBuf {
    let output = Command::new("git")
        .current_dir(root)
        .args(["rev-parse", "--git-path", CURRENT_SESSION_FILE])
        .output();

    if let Ok(output) = output
        && output.status.success()
    {
        let resolved = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !resolved.is_empty() {
            let path = PathBuf::from(&resolved);
            return if path.is_absolute() {
                path
            } else {
                root.join(path)
            };
        }
    }

    root.join(".agent/.cache").join(CURRENT_SESSION_FILE)
}

#[cfg(test)]
impl RepoContext {
    pub(crate) fn load_from(root: &Path) -> Result<Self> {
        let config_text = fs::read_to_string(root.join(".jig.toml"))?;
        let config: RepoConfig = toml::from_str(&config_text)?;
        validate_config(&config)?;
        let manifest_text = fs::read_to_string(root.join(".agent/jig-contract.json"))?;
        let manifest: ContractManifest = serde_json::from_str(&manifest_text)?;
        Ok(Self {
            root: root.to_path_buf(),
            current_session_path: root.join(".agent/.cache").join(CURRENT_SESSION_FILE),
            config,
            manifest,
        })
    }
}
