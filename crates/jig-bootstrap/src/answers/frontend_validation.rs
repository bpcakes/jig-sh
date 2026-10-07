pub(super) fn validate_frontend_apps(apps: &[FrontendApp]) -> Result<()> {
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
