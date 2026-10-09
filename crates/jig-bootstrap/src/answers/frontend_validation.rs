use super::*;

pub(crate) fn validate_frontend_apps(apps: &[FrontendApp]) -> Result<()> {
    let mut names = HashSet::new();
    for app in apps {
        if !is_safe_frontend_app_name(&app.name) {
            bail!(
                "Invalid frontend app name '{}'. Use ASCII letters, numbers, '-' or '_'.",
                app.name
            );
        }
        frontend_component_id(&app.name)?;
        if !names.insert(app.name.as_str()) {
            bail!("Duplicate frontend app name '{}'", app.name);
        }
        if !is_supported_frontend_app_kind(&app.kind) {
            bail!(
                "Invalid frontend app kind '{}'. Expected 'vite' or 'env-port'.",
                app.kind
            );
        }
        if !is_supported_frontend_app_role(&app.role) {
            bail!(
                "Invalid frontend app role '{}'. Expected 'spa', 'admin', or 'astro'.",
                app.role
            );
        }
        validate_frontend_app_dir(&app.name, &app.dir)?;
    }
    Ok(())
}

pub(super) fn is_safe_frontend_app_name(value: &str) -> bool {
    !value.is_empty()
        && value.trim() == value
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
}

pub(super) fn is_supported_frontend_app_kind(value: &str) -> bool {
    matches!(value, "vite" | "env-port")
}

pub(super) fn is_supported_frontend_app_role(value: &str) -> bool {
    matches!(value, "spa" | "admin" | "astro")
}

pub(super) fn validate_frontend_app_dir(app_name: &str, value: &str) -> Result<()> {
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
