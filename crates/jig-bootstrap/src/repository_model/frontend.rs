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

/// The committed OpenAPI documents.
pub(super) const OPENAPI_INPUT: &str = "openapi/**";

/// Committed public documentation that the boundary check scans.
pub(super) const PUBLIC_DOCUMENT_INPUTS: &[&str] = &["docs/public/**", "public-docs/**"];

/// The boundary check reads the Cargo dependency graph, not backend sources.
const BOUNDARY_MANIFEST_INPUTS: &[&str] =
    &["Cargo.toml", "Cargo.lock", "**/Cargo.toml", "**/Cargo.lock"];

/// The backend check that the committed OpenAPI documents match the backend.
pub(super) const OPENAPI_DOCUMENT_CHECK_ACTION: &str = "openapi";

/// The drift check beside the client-only contract script, which a scaffold
/// writes together with the document check: it owns only client freshness.
/// The description records that mode durably in the repository model.
pub(super) const CLIENT_DRIFT_DESCRIPTION: &str =
    "Check that the generated API clients match the committed OpenAPI documents.";

/// The drift check of a repository whose own contract script exports the
/// documents from the backend, as scripts did before the document check.
pub(super) const EXPORTING_DRIFT_DESCRIPTION: &str =
    "Check that the OpenAPI documents and generated clients match the backend.";

/// Backend sources, manifests, lockfiles, and workspace files, which a drift
/// check whose script exports from the backend reads.
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

/// The client-only drift check regenerates the clients from the committed
/// documents, so it reads only the contract surface; the backend document
/// check owns document freshness. A drift check whose script exports from the
/// backend also reads the backend. The boundary check reads the Cargo
/// dependency graph and committed public documentation.
pub(super) fn frontend_contract_inputs(
    public_boundary: bool,
    drift_reads_backend: bool,
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
        inputs.extend(
            BOUNDARY_MANIFEST_INPUTS
                .iter()
                .chain(PUBLIC_DOCUMENT_INPUTS)
                .map(|input| (*input).into()),
        );
    } else if drift_reads_backend {
        inputs.extend(BACKEND_CONTRACT_INPUTS.iter().map(|input| (*input).into()));
    }
    inputs.sort();
    inputs.dedup();
    inputs
}

pub(super) const BOUNDARY_DESCRIPTION: &str =
    "Check the repository-wide public/private dependency boundary.";

/// A generated contract check; its runner names the component command key.
pub(super) fn contract_check_action(
    action_id: &str,
    description: &str,
    inputs: Vec<String>,
) -> Result<ActionSpec> {
    let mut action = ActionSpec::new(
        target_id(REPO_COMPONENT, action_id)?,
        ActionIntent::Check,
        ActionRunner::command(CommandScope::Component.command_key(REPO_COMPONENT, action_id)?),
    );
    action.description = Some(description.into());
    action.effects = vec![ActionEffect::ReadOnly, ActionEffect::Process];
    action.inputs = inputs;
    action.provenance = provenance(&[
        ("target", FieldProvenance::Inferred),
        ("intent", FieldProvenance::Inherited),
        ("effects", FieldProvenance::Inherited),
        ("runner", FieldProvenance::Inferred),
        ("inputs", FieldProvenance::Inferred),
    ]);
    Ok(action)
}

/// The public boundary check guards a Rust admin API. A Rust scaffold gets
/// one with an admin frontend, and keeps a saved one: its repository may
/// still have the admin API after the admin app moved elsewhere.
pub(super) fn generates_public_boundary(answers: &RenderAnswers) -> bool {
    !answers.backend_language().is_go()
        && (answers.scaffolded_public_boundary()
            || answers
                .frontend_apps()
                .iter()
                .any(|app| app.role == "admin"))
}

/// Runs only the scaffold's committed-document tests, uncached, and fails
/// unless each one passed: Go would otherwise replay a pass when only a
/// document outside the module changed, and a filter that matches no test
/// passes. A customized general test command therefore cannot skip them.
/// Rust pins the per-test format, which quiet Cargo would print as dots,
/// keeps the lockfile as a read-only check must, and always requires the
/// public test, which every Rust scaffold has. Go builds the backend module
/// as a release does, outside any workspace, whose checksum file it would
/// update even in read-only mode, and refuses to update go.sum, even under an
/// inherited -mod=mod, so the check writes no checksum file.
pub(super) fn openapi_document_check_command(go: bool) -> &'static str {
    if go {
        concat!(
            "output=$(GOWORK=off go test -mod=readonly -count=1 -run '^TestOpenAPIIsCurrent$' -v ./internal/httpapi 2>&1); ",
            "status=$?; printf '%s\\n' \"$output\"; if [ \"$status\" -ne 0 ]; then ",
            "case \"$(go env GOWORK 2>/dev/null)\" in \"\" | off) ;; ",
            "*) printf 'api:openapi builds the backend module outside its go.work workspace, as releases do: ",
            "go.mod and go.sum must resolve every dependency, or set api_openapi_command.\\n' >&2 ;; esac; ",
            "exit \"$status\"; fi; ",
            "case \"$output\" in *'--- PASS: TestOpenAPIIsCurrent '*) ;; ",
            "*) printf 'TestOpenAPIIsCurrent did not pass.\\n' >&2; exit 1 ;; esac",
        )
    } else {
        concat!(
            "output=$(cargo test --workspace --locked openapi_is_current -- --format pretty 2>&1); ",
            "status=$?; printf '%s\\n' \"$output\"; [ \"$status\" -eq 0 ] || exit \"$status\"; ",
            "for name in public admin; do ",
            "[ \"$name\" = public ] || [ -f \"openapi/$name.json\" ] || continue; ",
            "case \"$output\" in *\"committed_${name}_openapi_is_current ... ok\"*) ;; ",
            "*) printf 'committed_%s_openapi_is_current did not pass.\\n' \"$name\" >&2; exit 1 ;; ",
            "esac; done",
        )
    }
}

impl ModelBuilder<'_> {
    /// The backend owns document freshness through a check that reads what
    /// the backend tests read, plus the documents.
    pub(super) fn add_openapi_document_check(&mut self) -> Result<()> {
        let mut inputs = self
            .actions
            .get(&target_id(BACKEND_COMPONENT, "test")?)
            .ok_or_else(|| anyhow::anyhow!("frontend contracts require the backend tests"))?
            .inputs
            .clone();
        inputs.push(OPENAPI_INPUT.into());
        inputs.sort();
        inputs.dedup();
        let command_key = CommandScope::Component
            .command_key(BACKEND_COMPONENT, OPENAPI_DOCUMENT_CHECK_ACTION)?;
        self.insert_command(
            &command_key,
            openapi_document_check_command(self.answers.backend_language().is_go()),
        )?;
        let mut action = ActionSpec::new(
            target_id(BACKEND_COMPONENT, OPENAPI_DOCUMENT_CHECK_ACTION)?,
            ActionIntent::Check,
            ActionRunner::command(command_key),
        );
        action.description =
            Some("Check that the committed OpenAPI documents match the backend.".into());
        action.effects = vec![ActionEffect::ReadOnly, ActionEffect::Process];
        action.inputs = inputs;
        action.provenance = provenance(&[
            ("target", FieldProvenance::Inferred),
            ("intent", FieldProvenance::Inherited),
            ("effects", FieldProvenance::Inherited),
            ("runner", FieldProvenance::Inferred),
            ("inputs", FieldProvenance::Inferred),
        ]);
        self.insert_action(action)
    }
}

impl RepositoryRenderModel {
    /// The document check that the backend workflow for `adapter` runs: a
    /// read-only check owned by a component with that adapter, so an authored
    /// action of that name joins CI only as a check.
    pub(super) fn openapi_document_check(&self, adapter: &str) -> Option<&ActionSpec> {
        let check = self.actions.iter().find(|action| {
            action.target.component.as_str() == BACKEND_COMPONENT
                && action.target.action.as_str() == OPENAPI_DOCUMENT_CHECK_ACTION
        })?;
        let owned = self
            .components
            .iter()
            .find(|component| component.id == check.target.component)?
            .adapters
            .iter()
            .any(|item| item == adapter);
        (owned
            && jig_repository::validate_read_only_check_closure(
                &self.actions,
                std::iter::once(&check.target),
            )
            .is_ok())
        .then_some(check)
    }

    pub(crate) fn openapi_document_check_target(&self, adapter: &str) -> Option<String> {
        self.openapi_document_check(adapter)
            .map(|check| check.target.to_string())
    }
}

pub(super) fn provenance(entries: &[(&str, FieldProvenance)]) -> BTreeMap<String, FieldProvenance> {
    entries
        .iter()
        .map(|(field, source)| ((*field).into(), *source))
        .collect()
}
