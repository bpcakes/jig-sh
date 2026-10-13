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

/// The backend check that the committed OpenAPI documents match the backend.
pub(super) const OPENAPI_DOCUMENT_CHECK_ACTION: &str = "openapi";

/// The document check's command key, which adoption keeps when it renames the
/// backend.
pub(super) const OPENAPI_DOCUMENT_COMMAND_KEY: &str = "api_openapi_command";

/// The drift check beside the client-only contract script, which a scaffold
/// writes together with the document check: it owns only client freshness.
/// The description records that mode durably in the repository model.
pub(super) const CLIENT_DRIFT_DESCRIPTION: &str =
    "Check that the generated API clients match the committed OpenAPI documents.";

/// The drift check's description before generators wrote the document check.
/// A repository that has not gained the check keeps it, which marks its model
/// as one that the next update may give the check.
pub(super) const HISTORICAL_DRIFT_DESCRIPTION: &str =
    "Check repository-wide OpenAPI and generated-client drift.";

/// The drift check of a repository whose own contract script exports the
/// documents from the backend, as scripts did before the document check.
pub(super) const EXPORTING_DRIFT_DESCRIPTION: &str =
    "Check that the OpenAPI documents and generated clients match the backend.";

/// Backend sources, manifests, lockfiles, and workspace files. The boundary
/// check inspects the backend dependency graph, so it reads them, as does a
/// drift check whose script exports from the backend.
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
/// backend also reads the backend, as the boundary check always does.
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
    if public_boundary || drift_reads_backend {
        inputs.extend(BACKEND_CONTRACT_INPUTS.iter().map(|input| (*input).into()));
    }
    if public_boundary {
        inputs.extend(["docs/public/**".into(), "public-docs/**".into()]);
    }
    inputs.sort();
    inputs.dedup();
    inputs
}

/// Runs only the scaffold's committed-document tests, uncached, and fails
/// unless each one passed: Go would otherwise replay a pass when only a
/// document outside the module changed, and a filter that matches no test
/// passes. A customized general test command therefore cannot skip them.
/// Rust pins the per-test format, which quiet Cargo would print as dots,
/// keeps the lockfile as a read-only check must, always requires the public
/// test, which every Rust scaffold has, and accepts each test only by its whole
/// name, not one that merely ends with it, and only from the package that
/// every Rust scaffold puts it in, named after the repository, so a test of
/// that name in another crate cannot stand in for it. Go builds the backend
/// module as a release does, outside any workspace, whose checksum file it
/// would update even in read-only mode, and refuses to update go.sum, even
/// under an inherited -mod=mod, so the check writes no checksum file.
pub(super) fn openapi_document_check_command(go: bool, repo_name: &str) -> String {
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
        .into()
    } else {
        let package = |suffix: &str| jig_repository::shell::quote(&format!("{repo_name}-{suffix}"));
        format!(
            concat!(
                "for name in public admin; do package={public}; ",
                "if [ \"$name\" = admin ]; then [ -f openapi/admin.json ] || continue; package={admin}; fi; ",
                "output=$(cargo test --locked -p \"$package\" \"committed_${{name}}_openapi_is_current\" -- --format pretty 2>&1); ",
                "status=$?; printf '%s\\n' \"$output\"; [ \"$status\" -eq 0 ] || exit \"$status\"; ",
                "case \"$output\" in *\"::committed_${{name}}_openapi_is_current ... ok\"* ",
                "| *\"test committed_${{name}}_openapi_is_current ... ok\"*) ;; ",
                "*) printf 'committed_%s_openapi_is_current did not pass in %s.\\n' \"$name\" \"$package\" >&2; exit 1 ;; ",
                "esac; done",
            ),
            public = package("http"),
            admin = package("admin-http"),
        )
    }
}

impl ModelBuilder<'_> {
    /// The backend owns document freshness through a check that reads what
    /// the backend tests read, plus the documents. Adoption generates the
    /// backend only at the repository root, so a nested backend has no
    /// generated tests to own the check, and gets none. Nor does a model from
    /// before the check until its owner asks for it: the check runs the
    /// scaffold's document tests by name, which the application may have
    /// renamed or disabled, and the model's drift check still exports from
    /// the backend. A saved check keeps its command, like the other backend
    /// commands.
    pub(super) fn add_openapi_document_check(&mut self) -> Result<()> {
        if self.answers.document_check_deferred() {
            // A command that its owner set for the check stays theirs.
            if let Some(command) = self.answers.openapi_check_command() {
                self.insert_command(OPENAPI_DOCUMENT_COMMAND_KEY, command)?;
            }
            return Ok(());
        }
        let Some(tests) = self.actions.get(&target_id(BACKEND_COMPONENT, "test")?) else {
            return Ok(());
        };
        let mut inputs = tests.inputs.clone();
        inputs.push(OPENAPI_INPUT.into());
        inputs.sort();
        inputs.dedup();
        let command = self.answers.openapi_check_command().map_or_else(
            || {
                openapi_document_check_command(
                    self.answers.backend_language().is_go(),
                    self.answers.repo_name(),
                )
            },
            str::to_owned,
        );
        self.insert_command(OPENAPI_DOCUMENT_COMMAND_KEY, &command)?;
        let mut action = ActionSpec::new(
            target_id(BACKEND_COMPONENT, OPENAPI_DOCUMENT_CHECK_ACTION)?,
            ActionIntent::Check,
            ActionRunner::command(OPENAPI_DOCUMENT_COMMAND_KEY),
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
    /// The document checks that the backend workflow for `adapter` runs:
    /// each read-only check of that name owned by a component with that
    /// adapter, whatever adoption or the owner named the component, so an
    /// authored action of that name joins CI only as a check.
    pub(super) fn openapi_document_checks(&self, adapter: &str) -> Vec<&ActionSpec> {
        self.actions
            .iter()
            .filter(|check| {
                is_document_action(&self.components, check, adapter)
                    && jig_repository::validate_read_only_check_closure(
                        &self.actions,
                        std::iter::once(&check.target),
                    )
                    .is_ok()
            })
            .collect()
    }

    pub(crate) fn openapi_document_check_targets(&self, adapter: &str) -> Vec<String> {
        self.openapi_document_checks(adapter)
            .into_iter()
            .map(|check| check.target.to_string())
            .collect()
    }
}

/// Whether `action` has the document check's name and belongs to a component
/// with `adapter`, whatever adoption or the owner named the component.
pub(super) fn is_document_action(
    components: &[ComponentSpec],
    action: &ActionSpec,
    adapter: &str,
) -> bool {
    action.target.action.as_str() == OPENAPI_DOCUMENT_CHECK_ACTION
        && components.iter().any(|component| {
            component.id == action.target.component
                && component.adapters.iter().any(|item| item == adapter)
        })
}

pub(super) fn provenance(entries: &[(&str, FieldProvenance)]) -> BTreeMap<String, FieldProvenance> {
    entries
        .iter()
        .map(|(field, source)| ((*field).into(), *source))
        .collect()
}
