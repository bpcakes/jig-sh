use super::*;

/// Frontend checks read their declared inputs, which include the API contract
/// and generated clients but not backend sources, so a frontend never depends
/// on the backend component. A frontend whose scripts compile or run backend
/// code must declare those files as inputs.
pub(super) fn frontend_component(app: &FrontendApp) -> Result<ComponentSpec> {
    let id = frontend_component_id(&app.name)?;
    let mut component = ComponentSpec::new(id, &app.dir);
    component.description = Some(format!("Frontend application '{}'.", app.name));
    component.tags = vec!["frontend".into(), app.role.clone()];
    component.adapters = vec!["typescript".into()];
    component.provenance = provenance(&[
        ("id", FieldProvenance::Inferred),
        ("root", FieldProvenance::Declared),
        ("adapters", FieldProvenance::Inferred),
    ]);
    Ok(component)
}

/// Earlier generators made every frontend depend on the backend, so any
/// backend change selected every frontend check, and made frontend typecheck
/// and build depend on the contract checks, so any frontend change ran them.
/// Drop those saved dependencies, including under an adopted backend id, but
/// only in their generated shape so an authored dependency survives.
pub(super) fn retire_saved_frontend_dependencies(
    components: &mut [ComponentSpec],
    actions: &mut [ActionSpec],
) {
    let frontends = components
        .iter()
        .filter(|component| component.tags.iter().any(|tag| tag == "frontend"))
        .map(|component| component.id.clone())
        .collect::<BTreeSet<_>>();
    let backends = components
        .iter()
        .filter(|component| {
            component.root == "."
                && component
                    .adapters
                    .iter()
                    .any(|adapter| matches!(adapter.as_str(), "rust" | "go"))
        })
        .map(|component| component.id.clone())
        .collect::<BTreeSet<_>>();
    for component in components {
        if frontends.contains(&component.id)
            && matches!(
                component.depends_on.as_slice(),
                [dependency] if backends.contains(dependency)
            )
            && component.provenance.get("depends_on") == Some(&FieldProvenance::Inferred)
        {
            component.depends_on.clear();
            component.provenance.remove("depends_on");
        }
    }
    for action in actions {
        // The only shapes generators wrote: typecheck on both contract checks
        // in this order, and build on the boundary check.
        let generated: &[&str] = match action.target.action.as_str() {
            "typecheck" => &[
                FRONTEND_CONTRACT_DRIFT_ACTION,
                FRONTEND_PUBLIC_BOUNDARY_ACTION,
            ],
            "build" => &[FRONTEND_PUBLIC_BOUNDARY_ACTION],
            _ => continue,
        };
        if frontends.contains(&action.target.component)
            && action.depends_on.len() == generated.len()
            && action
                .depends_on
                .iter()
                .zip(generated)
                .all(|(dependency, expected)| {
                    dependency.component.as_str() == REPO_COMPONENT
                        && dependency.action.as_str() == *expected
                })
            && action.provenance.get("depends_on") == Some(&FieldProvenance::Inferred)
        {
            action.depends_on.clear();
        }
    }
}

pub fn frontend_component_id(name: &str) -> Result<ComponentId> {
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

pub(super) fn frontend_inputs(
    root: &str,
    inputs: &[&str],
    workspace_roots: &[String],
) -> Vec<String> {
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

pub(super) fn aggregate_frontend_inputs(
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

/// The backend tests compare the committed OpenAPI documents with the backend.
pub(super) const OPENAPI_INPUT: &str = "openapi/**";

/// Backend sources, manifests, lockfiles, and workspace files. The boundary
/// check inspects the backend dependency graph, so it reads them.
pub(super) const BACKEND_CONTRACT_INPUTS: &[&str] = &[
    "Cargo.toml",
    "Cargo.lock",
    "**/Cargo.toml",
    "**/Cargo.lock",
    "**/*.rs",
    "go.mod",
    "go.sum",
    "go.work",
    "go.work.sum",
    "**/go.mod",
    "**/go.sum",
    "**/go.work",
    "**/go.work.sum",
    "**/*.go",
    "vendor/modules.txt",
    "**/vendor/modules.txt",
];

/// The drift check regenerates the clients from the committed documents, so
/// it reads only the contract surface; the backend tests own document
/// freshness. The boundary check also reads the backend.
pub(super) fn frontend_contract_inputs(
    public_boundary: bool,
    workspace_roots: &[String],
) -> Vec<String> {
    let mut inputs = FRONTEND_SHARED_INPUTS
        .iter()
        .map(|input| (*input).to_owned())
        .collect::<Vec<_>>();
    inputs.extend(
        workspace_roots
            .iter()
            .map(|root| component_root_input(root)),
    );
    if public_boundary {
        inputs.extend(BACKEND_CONTRACT_INPUTS.iter().map(|input| (*input).into()));
        inputs.extend(["docs/public/**".into(), "public-docs/**".into()]);
    }
    inputs.sort();
    inputs.dedup();
    inputs
}

/// Backend tests assert that the committed OpenAPI documents match the
/// backend, so a document edit must select them.
pub(super) fn add_backend_contract_test_inputs(
    actions: &mut BTreeMap<TargetId, ActionSpec>,
) -> Result<()> {
    for id in ["test", "test-locked"] {
        if let Some(action) = actions.get_mut(&target_id(BACKEND_COMPONENT, id)?) {
            action.inputs.push(OPENAPI_INPUT.into());
            action
                .provenance
                .insert("inputs".into(), FieldProvenance::Inferred);
        }
    }
    Ok(())
}

pub(super) fn provenance(entries: &[(&str, FieldProvenance)]) -> BTreeMap<String, FieldProvenance> {
    entries
        .iter()
        .map(|(field, source)| ((*field).into(), *source))
        .collect()
}
