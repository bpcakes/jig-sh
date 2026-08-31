// agentic-loc-exception: staged rendering and contract validation share one transactional boundary.
use std::collections::BTreeSet;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use minijinja::{Environment, UndefinedBehavior, syntax::SyntaxConfig};
use serde_json::{Value as JsonValue, json};
use tempfile::TempDir;

use super::ANSWERS_FILE;
use super::answers::RenderAnswers;
use super::embedded_templates::EMBEDDED_TEMPLATE_FILES;
use super::managed_paths;
use super::path::{
    validate_no_reserved_git_metadata_components, validate_portable_planned_file_collisions,
};
use super::preview_seed::seed_preview_workspace;
use super::repository_model::RepositoryRenderModel;
use super::staged_render::StagedRender;
use super::template_source::{PreparedTemplateSource, TemplateRenderSource};
use crate::progress::CliProgress;

const TEMPLATE_SUBDIRECTORY: &str = "templates/project";
const TEMPLATE_SUFFIX: &str = ".jinja";
pub(super) const RUST_GATE_COMMAND_AUTHORITY_PATHS: &[&str] =
    &["Cargo.toml", "Cargo.lock", "rust-toolchain*", ".cargo/**"];
pub(super) const FRONTEND_GATE_SHARED_PATHS: &[&str] = &[
    "packages/**",
    "package.json",
    "package-lock.json",
    "npm-shrinkwrap.json",
    "pnpm-lock.yaml",
    "pnpm-workspace.yaml",
    "yarn.lock",
    "bun.lock",
    "bun.lockb",
    ".node-version",
    "**/.node-version",
    ".npmrc",
    "**/.npmrc",
    ".yarnrc",
    ".yarnrc.yml",
    "**/.yarnrc",
    "**/.yarnrc.yml",
    ".pnpmfile.cjs",
    "pnpmfile.cjs",
    "**/.pnpmfile.cjs",
    "**/pnpmfile.cjs",
    "bunfig.toml",
    "**/bunfig.toml",
    "patches/**",
    "**/patches/**",
    ".yarn/patches/**",
    ".yarn/plugins/**",
    ".yarn/releases/**",
    "**/.yarn/patches/**",
    "**/.yarn/plugins/**",
    "**/.yarn/releases/**",
    "tsconfig*.json",
    "eslint.config.*",
    "vite.config.*",
    "vitest.config.*",
    "scripts/check-webapps.sh",
    "scripts/check-webapp-scripts.mjs",
    "scripts/enforce-coverage.cjs",
    "scripts/web-node.cjs",
];

pub(super) struct RenderStageRequest<'a> {
    pub(super) template: &'a PreparedTemplateSource,
    pub(super) answers: &'a RenderAnswers,
    pub(super) seed_repo_path: Option<&'a Path>,
    pub(super) prior_managed_paths: Option<&'a BTreeSet<PathBuf>>,
    pub(super) reconcile_runtime_config: bool,
    pub(super) preferred_rendered_commands: BTreeSet<String>,
    pub(super) contract_version: Option<u32>,
    pub(super) progress: CliProgress,
}

pub(super) fn stage_render(request: RenderStageRequest<'_>) -> Result<StagedRender> {
    let root = request
        .progress
        .log_blocked_on_err(TempDir::new().context("Failed to create staging directory"))?;
    let destination = root.path().join("render");
    if let Some(seed_repo_path) = request.seed_repo_path {
        request
            .progress
            .step("seed agent guides", "scan existing repo for AGENTS.md");
        request
            .progress
            .log_blocked_on_err(seed_preview_workspace(seed_repo_path, &destination))?;
    }

    request
        .progress
        .step("render templates", "managed files, scripts, and workflows");
    let mut active_paths = request.progress.log_blocked_on_err(render_template_files(
        request.template,
        request.answers,
        &destination,
        None,
        request.contract_version,
    ))?;
    if !request.answers.is_minimal_footprint() {
        request
            .progress
            .step("generate agent map", "native renderer");
        request
            .progress
            .log_blocked_on_err(run_post_render_tasks(&destination))?;
    } else {
        request
            .progress
            .log_blocked_on_err(set_scripts_executable(&destination))?;
    }
    merge_existing_managed_blocks(
        request.seed_repo_path,
        &destination,
        &active_paths,
        request.progress,
    )?;
    let mut retirement_paths = BTreeSet::new();
    if let Some(prior_paths) = request.prior_managed_paths {
        retire_inactive_managed_blocks(
            request.seed_repo_path,
            &destination,
            &active_paths,
            prior_paths,
            &mut retirement_paths,
            request.progress,
        )?;
    }
    if request.reconcile_runtime_config {
        super::runtime_config::reconcile_runtime_config(
            request.seed_repo_path,
            &destination,
            &request.preferred_rendered_commands,
        )?;
    }

    active_paths.insert(PathBuf::from(managed_paths::MANIFEST_PATH));
    request
        .progress
        .log_blocked_on_err(validate_portable_planned_file_collisions(&active_paths))?;
    request
        .progress
        .log_blocked_on_err(managed_paths::write_manifest(&destination, &active_paths))?;
    if let Some(prior_paths) = request.prior_managed_paths {
        retirement_paths.extend(
            prior_paths
                .difference(&active_paths)
                .filter(|path| managed_paths::managed_block_spec(path).is_none())
                .cloned(),
        );
    }

    let answers_path = destination.join(ANSWERS_FILE);
    if !answers_path.exists() {
        request
            .progress
            .blocked(format!("staging render did not produce {ANSWERS_FILE}"));
        bail!(
            "Staging render did not produce {} in {}",
            ANSWERS_FILE,
            destination.display()
        );
    }
    let staged_context = crate::context::RepoContext::load_from_root(destination.clone())
        .with_context(|| {
            format!(
                "Staged render produced an invalid Jig config or contract in {}",
                destination.display()
            )
        })?;
    validate_staged_runtime_contract(&destination, staged_context.contract_version())?;
    crate::policy::validate_contract(&staged_context).with_context(|| {
        format!(
            "Staged render produced an invalid Jig config or contract in {}",
            destination.display()
        )
    })?;

    Ok(StagedRender {
        _root: root,
        destination,
        active_paths,
        retirement_paths,
    })
}

pub(super) fn validate_staged_runtime_contract(
    destination: &Path,
    manifest_contract_version: u32,
) -> Result<()> {
    let requires_repository_scoped_runtime =
        manifest_contract_version > crate::context::LAST_VERSION_LOCKED_CONTRACT_VERSION;
    let launcher_path = destination.join("scripts/jig");
    let launcher = match fs::read_to_string(&launcher_path) {
        Ok(launcher) => Some(launcher),
        Err(error) if error.kind() == ErrorKind::NotFound => None,
        Err(error) => {
            return Err(error)
                .with_context(|| format!("Failed to read {}", launcher_path.display()));
        }
    };

    if let Some(launcher) = launcher {
        let inspection = crate::runtime_artifacts::inspect_launcher(&launcher);
        match inspection.declared_contract_version() {
            crate::runtime_artifacts::ParsedField::Value(launcher_contract_version)
                if launcher_contract_version != manifest_contract_version =>
            {
                bail!(
                    "Staged launcher {} declares contract {}, but the staged manifest declares contract {}",
                    launcher_path.display(),
                    launcher_contract_version,
                    manifest_contract_version
                );
            }
            crate::runtime_artifacts::ParsedField::Malformed => {
                bail!(
                    "Staged launcher {} has an unreadable CONTRACT_VERSION",
                    launcher_path.display()
                );
            }
            crate::runtime_artifacts::ParsedField::Missing
                if requires_repository_scoped_runtime =>
            {
                bail!(
                    "Staged contract-v{} launcher {} does not declare CONTRACT_VERSION",
                    manifest_contract_version,
                    launcher_path.display()
                );
            }
            crate::runtime_artifacts::ParsedField::Missing
            | crate::runtime_artifacts::ParsedField::Value(_) => {}
        }

        if requires_repository_scoped_runtime && !inspection.uses_repository_scope_protocol() {
            bail!(
                "Staged contract-v{} launcher {} does not implement the repository-scoped runtime protocol",
                manifest_contract_version,
                launcher_path.display()
            );
        }
    }

    if requires_repository_scoped_runtime {
        let installer_path = destination.join("scripts/install-jig.sh");
        let installer = match fs::read_to_string(&installer_path) {
            Ok(installer) => Some(installer),
            Err(error) if error.kind() == ErrorKind::NotFound => None,
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("Failed to read {}", installer_path.display()));
            }
        };
        if let Some(installer) = installer
            && !crate::runtime_artifacts::inspect_installer(&installer)
                .uses_repository_scope_protocol()
        {
            bail!(
                "Staged contract-v{} installer {} does not implement the repository-scoped runtime protocol",
                manifest_contract_version,
                installer_path.display()
            );
        }
    }
    Ok(())
}

pub(super) fn stage_selected_render(
    template: &PreparedTemplateSource,
    answers: &RenderAnswers,
    selected_paths: &BTreeSet<PathBuf>,
    contract_version: u32,
    progress: CliProgress,
) -> Result<StagedRender> {
    let root = progress
        .log_blocked_on_err(TempDir::new().context("Failed to create staging directory"))?;
    let destination = root.path().join("render");
    progress.step("render templates", "selected managed files");
    let active_paths = progress.log_blocked_on_err(render_template_files(
        template,
        answers,
        &destination,
        Some(selected_paths),
        Some(contract_version),
    ))?;
    let missing_paths = selected_paths
        .difference(&active_paths)
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>();
    if !missing_paths.is_empty() {
        bail!(
            "Embedded template is missing selected managed paths: {}",
            missing_paths.join(", ")
        );
    }
    progress.log_blocked_on_err(set_scripts_executable(&destination))?;
    progress.log_blocked_on_err(validate_staged_runtime_contract(
        &destination,
        contract_version,
    ))?;
    progress.log_blocked_on_err(validate_portable_planned_file_collisions(&active_paths))?;

    Ok(StagedRender {
        _root: root,
        destination,
        active_paths,
        retirement_paths: BTreeSet::new(),
    })
}

fn render_template_files(
    template: &PreparedTemplateSource,
    answers: &RenderAnswers,
    destination: &Path,
    selected_paths: Option<&BTreeSet<PathBuf>>,
    contract_version: Option<u32>,
) -> Result<BTreeSet<PathBuf>> {
    let context = render_context(template, answers, contract_version)?;
    let mut environment = Environment::new();
    environment.set_syntax(
        SyntaxConfig::builder()
            .block_delimiters("[%", "%]")
            .variable_delimiters("<<[", "]>>")
            .comment_delimiters("<#", "#>")
            .build()?,
    );
    environment.set_undefined_behavior(UndefinedBehavior::Strict);

    let mut render = TemplateRender {
        environment: &mut environment,
        context: &context,
        answers,
        destination,
        selected_paths,
        managed_paths: BTreeSet::new(),
    };
    match template.render_source() {
        TemplateRenderSource::Filesystem(render_root) => {
            let template_root = render_root.join(TEMPLATE_SUBDIRECTORY);
            if !template_root.is_dir() {
                bail!(
                    "Template source does not contain {}: {}",
                    TEMPLATE_SUBDIRECTORY,
                    render_root.display()
                );
            }
            for template_path in collect_template_paths(&template_root)? {
                let relative_template =
                    template_path
                        .strip_prefix(&template_root)
                        .with_context(|| {
                            format!(
                                "{} is not under {}",
                                template_path.display(),
                                template_root.display()
                            )
                        })?;
                let source = fs::read_to_string(&template_path)
                    .with_context(|| format!("Failed to read {}", template_path.display()))?;
                render.entry(
                    relative_template,
                    &template_path.display().to_string(),
                    &source,
                )?;
            }
        }
        TemplateRenderSource::Embedded => {
            for template_file in EMBEDDED_TEMPLATE_FILES {
                render.entry(
                    Path::new(template_file.relative_path),
                    template_file.relative_path,
                    template_file.contents,
                )?;
            }
        }
    }

    Ok(render.managed_paths)
}

struct TemplateRender<'a, 'env> {
    environment: &'a mut Environment<'env>,
    context: &'a JsonValue,
    answers: &'a RenderAnswers,
    destination: &'a Path,
    selected_paths: Option<&'a BTreeSet<PathBuf>>,
    managed_paths: BTreeSet<PathBuf>,
}

impl TemplateRender<'_, '_> {
    fn entry(&mut self, relative_template: &Path, source_label: &str, source: &str) -> Result<()> {
        let relative = output_relative_path(relative_template)?;
        if self
            .selected_paths
            .is_some_and(|selected_paths| !selected_paths.contains(&relative))
        {
            return Ok(());
        }
        if managed_paths::should_omit_unmanaged_rendered_path(&relative, self.answers) {
            return Ok(());
        }
        self.managed_paths.insert(relative.clone());

        let rendered = self
            .environment
            .render_str(source, self.context)
            .with_context(|| format!("Failed to render {source_label}"))?;
        write_rendered_file(self.destination, &relative, rendered.as_bytes())
    }
}

fn merge_existing_managed_blocks(
    seed_repo_path: Option<&Path>,
    destination: &Path,
    managed_paths: &BTreeSet<PathBuf>,
    progress: CliProgress,
) -> Result<()> {
    let Some(seed_repo_path) = seed_repo_path else {
        return Ok(());
    };

    for spec in managed_paths::managed_block_specs() {
        let relative = Path::new(spec.path);
        if !managed_paths.contains(relative) {
            continue;
        }
        merge_existing_managed_block(seed_repo_path, destination, relative, progress)?;
    }
    Ok(())
}

fn retire_inactive_managed_blocks(
    seed_repo_path: Option<&Path>,
    destination: &Path,
    active_paths: &BTreeSet<PathBuf>,
    prior_paths: &BTreeSet<PathBuf>,
    retirement_paths: &mut BTreeSet<PathBuf>,
    progress: CliProgress,
) -> Result<()> {
    let Some(seed_repo_path) = seed_repo_path else {
        return Ok(());
    };

    for spec in managed_paths::managed_block_specs() {
        let relative = Path::new(spec.path);
        if active_paths.contains(relative) || !prior_paths.contains(relative) {
            continue;
        }
        retire_managed_block(
            seed_repo_path,
            destination,
            relative,
            *spec,
            retirement_paths,
            progress,
        )?;
    }
    Ok(())
}

fn retire_managed_block(
    seed_repo_path: &Path,
    destination: &Path,
    relative: &Path,
    spec: managed_paths::ManagedBlockSpec,
    retirement_paths: &mut BTreeSet<PathBuf>,
    progress: CliProgress,
) -> Result<()> {
    let existing_path = seed_repo_path.join(relative);
    let metadata = match fs::symlink_metadata(&existing_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("Failed to stat {}", existing_path.display()));
        }
    };
    if !metadata.is_file() {
        return Ok(());
    }

    let existing = fs::read(&existing_path)
        .with_context(|| format!("Failed to read {}", existing_path.display()))?;
    let Some((start, end)) = jig_block_byte_bounds(&existing, &existing_path, spec)? else {
        return Ok(());
    };

    progress.step(
        "retire managed block",
        format!("preserve repo-owned {} content", spec.path),
    );
    let remaining = remove_jig_block_and_separator(&existing, start, end);
    let rendered_path = destination.join(relative);
    fs::write(&rendered_path, remaining)
        .with_context(|| format!("Failed to write {}", rendered_path.display()))?;
    retirement_paths.insert(relative.to_path_buf());
    Ok(())
}

fn remove_jig_block_and_separator(contents: &[u8], start: usize, end: usize) -> Vec<u8> {
    let splice_start = if contents[..start].ends_with(b"\r\n\r\n") {
        start - 2
    } else if contents[..start].ends_with(b"\n\n") {
        start - 1
    } else {
        start
    };
    let splice_end = if contents[end..].starts_with(b"\r\n") {
        end + 2
    } else if contents[end..].starts_with(b"\n") {
        end + 1
    } else {
        end
    };
    let mut remaining = Vec::with_capacity(contents.len() - (splice_end - splice_start));
    remaining.extend_from_slice(&contents[..splice_start]);
    remaining.extend_from_slice(&contents[splice_end..]);
    remaining
}

fn jig_block_byte_bounds(
    contents: &[u8],
    path: &Path,
    spec: managed_paths::ManagedBlockSpec,
) -> Result<Option<(usize, usize)>> {
    let begins = byte_match_indices(contents, spec.begin.as_bytes());
    let ends = byte_match_indices(contents, spec.end.as_bytes());
    match (begins.as_slice(), ends.as_slice()) {
        ([], []) => Ok(None),
        ([begin], [end]) if begin < end => Ok(Some((*begin, end + spec.end.len()))),
        _ => bail!(
            "Malformed Jig managed block in {}. Expected exactly one begin marker before exactly one end marker.",
            path.display()
        ),
    }
}

fn byte_match_indices(contents: &[u8], needle: &[u8]) -> Vec<usize> {
    contents
        .windows(needle.len())
        .enumerate()
        .filter_map(|(index, candidate)| (candidate == needle).then_some(index))
        .collect()
}

fn merge_existing_managed_block(
    seed_repo_path: &Path,
    destination: &Path,
    relative: &Path,
    progress: CliProgress,
) -> Result<()> {
    let Some(spec) = managed_paths::managed_block_spec(relative) else {
        return Ok(());
    };

    let existing_path = seed_repo_path.join(relative);
    if !existing_path.exists() {
        return Ok(());
    }

    let rendered_path = destination.join(relative);
    if !rendered_path.exists() {
        return Ok(());
    }

    let step_label = format!("merge {}", spec.progress_label);
    progress.step(
        &step_label,
        format!("preserve repo-owned {} content", spec.path),
    );
    let existing = progress.log_blocked_on_err(
        fs::read_to_string(&existing_path)
            .with_context(|| format!("Failed to read {}", existing_path.display())),
    )?;
    let rendered = progress.log_blocked_on_err(
        fs::read_to_string(&rendered_path)
            .with_context(|| format!("Failed to read {}", rendered_path.display())),
    )?;
    let block = progress.log_blocked_on_err(extract_jig_block(&rendered, &rendered_path, spec))?;
    let merged =
        progress.log_blocked_on_err(merge_jig_block(&existing, block, &existing_path, spec))?;
    progress.log_blocked_on_err(
        fs::write(&rendered_path, merged)
            .with_context(|| format!("Failed to write {}", rendered_path.display())),
    )
}

fn extract_jig_block<'a>(
    contents: &'a str,
    path: &Path,
    spec: managed_paths::ManagedBlockSpec,
) -> Result<&'a str> {
    let Some((start, end)) = jig_block_bounds(contents, path, spec)? else {
        bail!(
            "Rendered {} does not contain a Jig managed block.",
            path.display()
        );
    };
    Ok(&contents[start..end])
}

fn merge_jig_block(
    existing: &str,
    block: &str,
    path: &Path,
    spec: managed_paths::ManagedBlockSpec,
) -> Result<String> {
    if let Some((start, end)) = jig_block_bounds(existing, path, spec)? {
        return Ok(format!(
            "{}{}{}",
            &existing[..start],
            block,
            &existing[end..]
        ));
    }

    let mut merged = existing.trim_end_matches('\n').to_string();
    if !merged.is_empty() {
        merged.push_str("\n\n");
    }
    merged.push_str(block.trim_end_matches('\n'));
    merged.push('\n');
    Ok(merged)
}

fn jig_block_bounds(
    contents: &str,
    path: &Path,
    spec: managed_paths::ManagedBlockSpec,
) -> Result<Option<(usize, usize)>> {
    let begins = contents.match_indices(spec.begin).collect::<Vec<_>>();
    let ends = contents.match_indices(spec.end).collect::<Vec<_>>();

    match (begins.as_slice(), ends.as_slice()) {
        ([], []) => Ok(None),
        ([(begin, _)], [(end, _)]) if begin < end => Ok(Some((*begin, end + spec.end.len()))),
        _ => bail!(
            "Malformed Jig managed block in {}. Expected exactly one begin marker before exactly one end marker.",
            path.display()
        ),
    }
}

fn render_context(
    template: &PreparedTemplateSource,
    answers: &RenderAnswers,
    contract_version: Option<u32>,
) -> Result<JsonValue> {
    let contract_version = contract_version.unwrap_or(crate::context::CURRENT_CONTRACT_VERSION);
    let mut context = serde_json::to_value(answers)?
        .as_object()
        .cloned()
        .unwrap_or_default();
    context.insert(
        "frontend_harness_enabled".into(),
        JsonValue::Bool(answers.frontend_harness_enabled()),
    );
    context.insert(
        "frontend_gate_apps".into(),
        JsonValue::Array(
            answers
                .frontend_apps()
                .iter()
                .map(|app| {
                    let paths_ignore = answers
                        .frontend_apps()
                        .iter()
                        .filter(|other| other.name != app.name && other.dir != ".")
                        .filter(|other| {
                            !repo_dirs_intersect(&app.dir, &other.dir)
                                && !answers
                                    .frontend_workspace_roots()
                                    .iter()
                                    .any(|root| repo_dirs_intersect(root, &other.dir))
                        })
                        .map(|other| format!("{}/**", other.dir))
                        .collect::<Vec<_>>();
                    json!({
                        "name": app.name,
                        "dir": app.dir,
                        "coverage_threshold": app.coverage_threshold,
                        "key": super::answers::frontend_gate_key(&app.name),
                        "role": app.role,
                        "paths_ignore": paths_ignore,
                    })
                })
                .collect(),
        ),
    );
    context.insert(
        "rust_gate_command_authority_paths".into(),
        json!(RUST_GATE_COMMAND_AUTHORITY_PATHS),
    );
    let mut frontend_gate_shared_paths = FRONTEND_GATE_SHARED_PATHS
        .iter()
        .map(|path| (*path).to_string())
        .collect::<Vec<_>>();
    for path in answers.frontend_workspace_roots().iter().map(|root| {
        if root == "." {
            "**".into()
        } else {
            format!("{root}/**")
        }
    }) {
        if !frontend_gate_shared_paths.contains(&path) {
            frontend_gate_shared_paths.push(path);
        }
    }
    context.insert(
        "frontend_gate_shared_paths".into(),
        json!(frontend_gate_shared_paths),
    );
    context.insert(
        "go_backend_enabled".into(),
        JsonValue::Bool(answers.go_backend_enabled()),
    );
    context.insert(
        "rust_backend_enabled".into(),
        JsonValue::Bool(answers.rust_backend_enabled()),
    );
    context.insert(
        "go_postgres_enabled".into(),
        JsonValue::Bool(answers.go_postgres_enabled()),
    );
    context.insert(
        "go_ci_workflow_enabled".into(),
        JsonValue::Bool(answers.go_ci_workflow_enabled()),
    );
    context.insert(
        "rust_ci_workflow_enabled".into(),
        JsonValue::Bool(answers.rust_ci_workflow_enabled()),
    );
    context.insert(
        "go_sqlc_ci_enabled".into(),
        JsonValue::Bool(answers.go_sqlc_ci_enabled()),
    );
    for (name, target) in [
        ("go_fmt_ci_target", answers.go_fmt_ci_target()),
        ("go_lint_ci_target", answers.go_lint_ci_target()),
        (
            "go_test_locked_ci_target",
            answers.go_test_locked_ci_target(),
        ),
        ("go_sqlc_ci_target", answers.go_sqlc_ci_target()),
        ("rust_fmt_ci_target", answers.rust_fmt_ci_target()),
        ("rust_clippy_ci_target", answers.rust_clippy_ci_target()),
        (
            "rust_test_locked_ci_target",
            answers.rust_test_locked_ci_target(),
        ),
    ] {
        context.insert(name.into(), target.unwrap_or_default().into());
    }
    context.insert(
        "go_postgres_integration_ci_enabled".into(),
        JsonValue::Bool(answers.go_postgres_integration_ci_enabled()),
    );
    let repository = (contract_version >= 6
        || answers.go_ci_workflow_enabled()
        || answers.rust_ci_workflow_enabled())
    .then(|| RepositoryRenderModel::from_answers(answers))
    .transpose()?;
    context.insert(
        "go_ci_input_paths".into(),
        serde_json::to_value(
            repository
                .as_ref()
                .map(RepositoryRenderModel::go_ci_input_paths)
                .unwrap_or_default(),
        )?,
    );
    context.insert(
        "rust_ci_input_paths".into(),
        serde_json::to_value(
            repository
                .as_ref()
                .map(RepositoryRenderModel::rust_ci_input_paths)
                .unwrap_or_default(),
        )?,
    );
    context.insert(
        "rust_component_input_paths".into(),
        serde_json::to_value(
            repository
                .as_ref()
                .map(RepositoryRenderModel::rust_component_input_paths)
                .unwrap_or_default(),
        )?,
    );
    context.insert(
        "rust_workspace_guidance_enabled".into(),
        JsonValue::Bool(
            repository
                .as_ref()
                .is_some_and(RepositoryRenderModel::rust_workspace_guidance_enabled),
        ),
    );
    if contract_version >= 6 {
        let repository = repository.expect("contract v6 always resolves a repository model");
        let repository_toml = repository.authored_toml()?;
        let repository_commands_toml = repository.commands_toml()?;
        context.insert(
            "frontend_contracts_enabled".into(),
            JsonValue::Bool(repository.frontend_contracts_enabled()),
        );
        context.insert("repository".into(), serde_json::to_value(repository)?);
        context.insert("repository_toml".into(), repository_toml.into());
        context.insert(
            "repository_commands_toml".into(),
            repository_commands_toml.into(),
        );
    }
    context.insert(
        "_jig".into(),
        json!({
            "commit": template.vcs_ref().unwrap_or_default(),
            "src_path": if answers.template_source_url().is_empty() {
                template.source().to_string()
            } else {
                answers.template_source_url().to_string()
            },
            "template_mode": template.template_mode_answer().unwrap_or(""),
            "template_local_path": template.template_local_path_answer().unwrap_or(""),
            "contract_version": contract_version,
        }),
    );
    Ok(JsonValue::Object(context))
}

fn repo_dirs_intersect(left: &str, right: &str) -> bool {
    left == "."
        || right == "."
        || left == right
        || left
            .strip_prefix(right)
            .is_some_and(|suffix| suffix.starts_with('/'))
        || right
            .strip_prefix(left)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn collect_template_paths(root: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    collect_template_paths_recursive(root, &mut paths)?;
    paths.sort();
    Ok(paths)
}

fn collect_template_paths_recursive(current: &Path, paths: &mut Vec<PathBuf>) -> Result<()> {
    for entry in
        fs::read_dir(current).with_context(|| format!("Failed to read {}", current.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_template_paths_recursive(&path, paths)?;
        } else if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(TEMPLATE_SUFFIX))
        {
            paths.push(path);
        }
    }
    Ok(())
}

fn output_relative_path(relative_template: &Path) -> Result<PathBuf> {
    let file_name = relative_template
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow::anyhow!("Invalid template path: {}", relative_template.display()))?;
    let output_name = file_name.strip_suffix(TEMPLATE_SUFFIX).ok_or_else(|| {
        anyhow::anyhow!(
            "Template path must end with {TEMPLATE_SUFFIX}: {}",
            relative_template.display()
        )
    })?;
    let relative = relative_template.with_file_name(output_name);
    validate_no_reserved_git_metadata_components(&relative)?;
    Ok(relative)
}

fn write_rendered_file(destination: &Path, relative: &Path, contents: &[u8]) -> Result<()> {
    let path = destination.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    remove_existing_symlink(&path)?;
    fs::write(&path, contents).with_context(|| format!("Failed to write {}", path.display()))?;
    set_rendered_permissions(&path, relative)
}

fn remove_existing_symlink(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => fs::remove_file(path)
            .with_context(|| format!("Failed to remove symlink {}", path.display())),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("Failed to stat {}", path.display())),
    }
}

fn run_post_render_tasks(destination: &Path) -> Result<()> {
    set_scripts_executable(destination)?;
    crate::policy::write_agent_map(destination, Path::new(managed_paths::AGENT_MAP_PATH))
}

#[cfg(unix)]
fn set_rendered_permissions(path: &Path, relative: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    if managed_paths::is_executable_script(relative) {
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))
            .with_context(|| format!("Failed to set permissions on {}", path.display()))?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn set_rendered_permissions(_path: &Path, _relative: &Path) -> Result<()> {
    Ok(())
}

fn set_scripts_executable(destination: &Path) -> Result<()> {
    for relative in executable_script_paths(destination)? {
        set_rendered_permissions(&destination.join(&relative), &relative)?;
    }
    Ok(())
}

fn executable_script_paths(destination: &Path) -> Result<Vec<PathBuf>> {
    let scripts_dir = destination.join("scripts");
    if !scripts_dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut paths = Vec::new();
    for entry in fs::read_dir(&scripts_dir)
        .with_context(|| format!("Failed to read {}", scripts_dir.display()))?
    {
        let entry = entry?;
        let relative = PathBuf::from("scripts").join(entry.file_name());
        if managed_paths::is_executable_script(&relative) {
            paths.push(relative);
        }
    }
    Ok(paths)
}

#[cfg(test)]
#[path = "renderer_tests.rs"]
mod tests;
