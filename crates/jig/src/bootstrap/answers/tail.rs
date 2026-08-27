pub(super) fn frontend_gate_key(name: &str) -> String {
    name.to_ascii_lowercase().replace('-', "_")
}

fn is_safe_frontend_app_name(value: &str) -> bool {
    !value.is_empty()
        && value.trim() == value
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
}

fn is_supported_frontend_app_kind(value: &str) -> bool {
    matches!(value, "vite" | "env-port")
}

fn is_supported_frontend_app_role(value: &str) -> bool {
    matches!(value, "spa" | "admin" | "astro")
}

fn validate_frontend_app_dir(app_name: &str, value: &str) -> Result<()> {
    if value.is_empty() || value.trim() != value {
        bail!("frontend app '{app_name}' dir must be a non-empty relative path");
    }
    if !value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '-' | '_'))
    {
        bail!(
            "frontend app '{app_name}' dir '{value}' contains unsupported characters. Use a repo-relative path with ASCII letters, numbers, '/', '.', '-' or '_'; use forward slashes on every platform."
        );
    }

    let path = Path::new(value);
    if path.is_absolute() {
        bail!("frontend app '{app_name}' dir '{value}' must be relative");
    }
    if value.split('/').any(str::is_empty) {
        bail!("frontend app '{app_name}' dir '{value}' must not contain empty path components");
    }
    if value == "." {
        return Ok(());
    }
    if value.split('/').any(|segment| segment == ".") {
        bail!("frontend app '{app_name}' dir '{value}' must not contain '.' path components");
    }

    for component in path.components() {
        match component {
            Component::Normal(_) => {}
            Component::CurDir => {
                bail!(
                    "frontend app '{app_name}' dir '{value}' must not contain '.' path components"
                );
            }
            Component::ParentDir => {
                bail!("frontend app '{app_name}' dir '{value}' must not contain '..'");
            }
            Component::RootDir | Component::Prefix(_) => {
                bail!("frontend app '{app_name}' dir '{value}' must be relative");
            }
        }
    }
    Ok(())
}

pub(super) fn web_install_command(package_manager: &str) -> &'static str {
    match package_manager {
        "bun" => "bun install --frozen-lockfile",
        "pnpm" => "pnpm install --frozen-lockfile",
        "npm" => {
            "npm ci --include=dev --include=optional --include=peer --bin-links=true --dry-run=false --package-lock-only=false --package-lock=true --global=false"
        }
        "yarn" => "yarn install --frozen-lockfile",
        _ => unreachable!("web package manager was already validated"),
    }
}

pub(super) fn web_run_command(package_manager: &str) -> &'static str {
    match package_manager {
        "bun" => "bun run",
        "pnpm" => "pnpm run",
        "npm" => "npm run",
        "yarn" => "yarn run",
        _ => unreachable!("web package manager was already validated"),
    }
}

fn serialize_harness_footprint<S>(
    value: &HarnessFootprint,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(value.as_str())
}

fn default_codex_marketplaces() -> Vec<CodexMarketplaceAnswers> {
    vec![CodexMarketplaceAnswers {
        id: DEFAULT_CODEX_MARKETPLACE_ID.into(),
        source: DEFAULT_CODEX_MARKETPLACE_SOURCE.into(),
        plugins: default_codex_marketplace_plugins(),
    }]
}

fn merge_option<T>(target: &mut Option<T>, value: Option<T>) {
    if let Some(value) = value {
        *target = Some(value);
    }
}

fn default_repo_name(destination: &Path) -> Option<String> {
    destination
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
        .filter(|value| !value.is_empty())
}
