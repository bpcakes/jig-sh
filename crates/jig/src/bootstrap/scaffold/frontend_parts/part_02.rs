
fn frontend_workspace_template_files(
    package_manager: &str,
    frontends: &[FrontendScaffold],
) -> Vec<ScaffoldTemplateFile> {
    if frontends.is_empty() {
        return Vec::new();
    }
    let has_spa = frontends
        .iter()
        .any(|frontend| frontend.kind == ScaffoldFrontendKind::Spa);
    let has_admin = frontends
        .iter()
        .any(|frontend| frontend.kind == ScaffoldFrontendKind::Admin);
    let has_react = frontends.iter().any(|frontend| {
        matches!(
            frontend.kind,
            ScaffoldFrontendKind::Spa | ScaffoldFrontendKind::Admin
        )
    });
    FRONTEND_WORKSPACE_TEMPLATES
        .iter()
        .copied()
        .chain(has_react.then_some(REACT_ESLINT_TEMPLATE))
        .chain((package_manager == "pnpm").then_some(PNPM_WORKSPACE_TEMPLATE))
        .chain((package_manager == "yarn").then_some(YARN_WORKSPACE_TEMPLATE))
        .chain(has_spa.then_some(E2E_WORKFLOW_TEMPLATE))
        .chain(PUBLIC_API_CLIENT_TEMPLATES.iter().copied())
        .chain(
            has_admin
                .then_some(ADMIN_API_CLIENT_TEMPLATES)
                .into_iter()
                .flatten()
                .copied(),
        )
        .collect()
}

fn optional_cargo_command(command: &str, action: &str) -> String {
    format!(
        "if [ -f Cargo.toml ]; then {command}; else printf '%s\\n' 'No Cargo.toml found; skipping cargo {action}.'; fi"
    )
}

fn title_case(value: &str) -> String {
    value
        .split(['-', '_'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn repo_root_relative(frontend_dir: &str) -> String {
    let depth = PathBuf::from(frontend_dir).components().count();
    (0..depth).map(|_| "..").collect::<Vec<_>>().join("/")
}

fn e2e_database_name(module_name: &str, frontend_package_name: &str) -> String {
    let name = format!(
        "{module_name}_{}_e2e",
        frontend_package_name.replace('-', "_")
    );
    bounded_postgres_identifier(&name)
}
