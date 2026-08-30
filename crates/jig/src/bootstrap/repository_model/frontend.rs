fn frontend_component(app: &FrontendApp) -> Result<ComponentSpec> {
    let id = frontend_component_id(&app.name)?;
    let mut component = ComponentSpec::new(id, &app.dir);
    component.description = Some(format!("Frontend application '{}'.", app.name));
    component.tags = vec!["frontend".into(), app.role.clone()];
    component.depends_on = vec![component_id(BACKEND_COMPONENT)?];
    component.adapters = vec!["typescript".into()];
    component.provenance = provenance(&[
        ("id", FieldProvenance::Inferred),
        ("root", FieldProvenance::Declared),
        ("depends_on", FieldProvenance::Inferred),
        ("adapters", FieldProvenance::Inferred),
    ]);
    Ok(component)
}

pub(super) fn frontend_component_id(name: &str) -> Result<ComponentId> {
    let normalized = name.to_ascii_lowercase();
    if matches!(normalized.as_str(), REPO_COMPONENT | BACKEND_COMPONENT) {
        bail!(
            "Frontend app name '{name}' resolves to reserved repository component id '{normalized}'; choose a different frontend name"
        );
    }
    let value = if normalized.len() <= 64 {
        normalized
    } else {
        let digest = format!("{:x}", Sha256::digest(normalized.as_bytes()));
        let mut end = 51;
        while !normalized.is_char_boundary(end) {
            end -= 1;
        }
        format!(
            "{}-{}",
            normalized[..end].trim_end_matches('-'),
            &digest[..12]
        )
    };
    component_id(&value)
        .with_context(|| format!("Invalid frontend app name '{name}' for repository identity"))
}

fn frontend_inputs(root: &str, inputs: &[&str], workspace_roots: &[String]) -> Vec<String> {
    let mut resolved = inputs
        .iter()
        .map(|input| {
            if root == "." {
                (*input).to_owned()
            } else {
                format!("{root}/{input}")
            }
        })
        .collect::<Vec<_>>();
    resolved.extend(
        FRONTEND_SHARED_INPUTS
            .iter()
            .map(|input| (*input).to_owned()),
    );
    resolved.extend(
        workspace_roots
            .iter()
            .map(|root| component_root_input(root)),
    );
    resolved.sort();
    resolved.dedup();
    resolved
}

fn aggregate_frontend_inputs(
    apps: &[FrontendApp],
    inputs: &[&str],
    workspace_roots: &[String],
) -> Vec<String> {
    apps.iter()
        .flat_map(|app| frontend_inputs(&app.dir, inputs, workspace_roots))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn frontend_contract_inputs(
    include_public_artifacts: bool,
    workspace_roots: &[String],
) -> Vec<String> {
    let mut inputs = FRONTEND_SHARED_INPUTS
        .iter()
        .copied()
        .chain([
            "Cargo.toml",
            "**/Cargo.toml",
            "**/*.rs",
            "go.mod",
            "**/go.mod",
            "**/*.go",
        ])
        .map(str::to_owned)
        .collect::<Vec<_>>();
    inputs.extend(
        workspace_roots
            .iter()
            .map(|root| component_root_input(root)),
    );
    if include_public_artifacts {
        inputs.extend(["docs/public/**".into(), "public-docs/**".into()]);
    }
    inputs.sort();
    inputs.dedup();
    inputs
}

fn provenance(entries: &[(&str, FieldProvenance)]) -> BTreeMap<String, FieldProvenance> {
    entries
        .iter()
        .map(|(field, source)| ((*field).into(), *source))
        .collect()
}
