use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{Value as JsonValue, json};

use super::answers::AnswerInputShape;
use super::{AnswerOpts, FrontendApp};

mod commands;
mod frontend;
mod github;
mod metadata;
mod package_manager;
mod profile;
mod repo;
mod rust_sqlx;
mod scan;
mod topology;

use self::commands::{CommandCandidate, CommandInference, infer_commands};
use self::frontend::{
    FrontendAppProfile, FrontendAppsInference, infer_frontend_apps_with_metadata,
};
use self::github::{
    GithubCiInference, GithubCiShapeInference, infer_ci_github_runner_with_metadata,
};
use self::metadata::{Confidence, InferenceMetadata};
use self::package_manager::{PackageManagerInference, infer_package_manager_with_metadata};
use self::repo::{
    RepoValueInference, infer_default_branch_with_metadata, infer_repo_name_with_metadata,
};
use self::rust_sqlx::{
    RustCrateRootSourceKind, RustCrateRootsInference, SqlxInference,
    infer_rust_crate_roots_from_scan, infer_rust_crate_roots_with_metadata, infer_sqlx,
};
use self::scan::{RepoScan, read_limited_text};
use self::topology::{RepoTopology, infer_repo_topology};

const APPLICATION_CONTRACT_CHECKER_MARKER: &str =
    "// jig-application-contract-checker: v1 modes=check,public-check";

#[cfg(test)]
use self::frontend::segment_matches;
#[cfg(test)]
use self::github::select_github_runner;
#[cfg(test)]
use self::repo::{
    infer_default_branch, infer_repo_name, repo_name_from_remote_url, safe_repo_name,
};
#[cfg(test)]
use self::rust_sqlx::{crate_root_from_workspace_member, infer_rust_crate_roots};
#[cfg(test)]
use self::scan::MAX_SCAN_FILE_BYTES;

#[derive(Clone, Debug, Default)]
pub(super) struct AdoptInference {
    repo_name: Option<String>,
    default_branch: Option<String>,
    rust_crate_roots: Vec<String>,
    rust_crate_root_source_kind: RustCrateRootSourceKind,
    sqlx_enabled: Option<bool>,
    rust_migration_dir: Option<String>,
    rust_migration_dirs: Vec<String>,
    rust_sqlx_metadata_dir: Option<String>,
    sqlx_check_command: Option<String>,
    rust_fmt_check_command: Option<String>,
    rust_clippy_command: Option<String>,
    rust_test_command: Option<String>,
    rust_test_locked_command: Option<String>,
    command_profile: CommandInference,
    web_package_manager: Option<String>,
    application_contracts_enabled: Option<bool>,
    frontend_apps: Vec<FrontendApp>,
    frontend_workspace_roots: Vec<String>,
    frontend_profiles: Vec<FrontendAppProfile>,
    ci_github_runner: Option<String>,
    ci_shape: GithubCiShapeInference,
    repo_topology: RepoTopology,
    signals: Vec<String>,
    warnings: Vec<String>,
    metadata: BTreeMap<String, InferenceMetadata>,
}

pub(super) struct AdoptionReview {
    pub(super) items: Vec<String>,
}

pub(super) fn infer_adopt_answers(root: &Path) -> AdoptInference {
    let mut warnings = Vec::new();
    let scan = RepoScan::collect(root, &mut warnings);
    let repo_name = infer_repo_name_with_metadata(root);
    let default_branch = infer_default_branch_with_metadata(root, &mut warnings);
    let mut rust_crate_roots = infer_rust_crate_roots_with_metadata(root, &mut warnings);
    if rust_crate_roots.roots.is_empty() && !root.join("Cargo.toml").is_file() {
        rust_crate_roots = infer_rust_crate_roots_from_scan(root, &scan, &mut warnings);
    }
    let repo_topology = infer_repo_topology(root, &scan, &rust_crate_roots.roots, &mut warnings);
    let mut package_manager_warnings = Vec::new();
    let package_manager =
        infer_package_manager_with_metadata(root, &scan, &mut package_manager_warnings);
    let scanned_rust_packages =
        rust_crate_roots.source_kind == RustCrateRootSourceKind::ScannedPackages;
    let nested_manifest_paths =
        scanned_rust_packages.then_some(rust_crate_roots.scanned_manifest_paths.as_slice());
    let commands = infer_commands(root, &scan, nested_manifest_paths, &mut warnings);
    if scanned_rust_packages && commands.rust_test_locked_command.is_none() {
        warnings.push(
            "nested Rust manifest scan did not infer rust_test_locked_command; add a project-owned locked command once lockfiles are committed"
                .into(),
        );
    }
    let frontend_apps =
        infer_frontend_apps_with_metadata(root, repo_name.value.as_deref(), &mut warnings);
    if !frontend_apps.apps.is_empty() {
        warnings.extend(package_manager_warnings);
    }
    let github_ci = infer_ci_github_runner_with_metadata(root, &scan, &mut warnings);
    let mut inference = AdoptInference {
        repo_name: repo_name.value.clone(),
        default_branch: default_branch.value.clone(),
        rust_crate_roots: rust_crate_roots.roots.clone(),
        rust_crate_root_source_kind: rust_crate_roots.source_kind,
        rust_fmt_check_command: commands
            .rust_fmt_check_command
            .as_ref()
            .map(CommandCandidate::command),
        rust_clippy_command: commands
            .rust_clippy_command
            .as_ref()
            .map(CommandCandidate::command),
        rust_test_command: commands
            .rust_test_command
            .as_ref()
            .map(CommandCandidate::command),
        rust_test_locked_command: commands
            .rust_test_locked_command
            .as_ref()
            .map(CommandCandidate::command),
        command_profile: commands.clone(),
        web_package_manager: package_manager.value.clone(),
        application_contracts_enabled: Some(infer_application_contracts_enabled(
            root,
            &scan,
            !frontend_apps.apps.is_empty(),
            &mut warnings,
        )),
        frontend_apps: frontend_apps.apps.clone(),
        frontend_workspace_roots: frontend_apps.workspace_roots.clone(),
        frontend_profiles: frontend_apps.profiles.clone(),
        ci_github_runner: github_ci.runner.clone(),
        ci_shape: github_ci.shape.clone(),
        repo_topology,
        warnings,
        ..AdoptInference::default()
    };
    record_repository_metadata(
        &mut inference,
        &repo_name,
        &default_branch,
        &rust_crate_roots,
        &commands,
    );
    record_frontend_and_ci_metadata(&mut inference, &package_manager, &frontend_apps, &github_ci);

    let sqlx = infer_sqlx(root, &scan, &mut inference.warnings);
    apply_sqlx_inference(&mut inference, &sqlx);
    record_inference_signals(&mut inference, &github_ci);

    inference
}

fn record_repository_metadata(
    inference: &mut AdoptInference,
    repo_name: &RepoValueInference,
    default_branch: &RepoValueInference,
    rust_crate_roots: &RustCrateRootsInference,
    commands: &CommandInference,
) {
    if let Some(value) = inference.repo_name.clone() {
        let confidence = if repo_name
            .source
            .as_deref()
            .is_some_and(|source| source.starts_with("git "))
        {
            Confidence::High
        } else {
            Confidence::Medium
        };
        inference.record_metadata(
            "repo_name",
            json!(value),
            option_source(repo_name.source.clone()),
            confidence,
            Vec::new(),
        );
    }
    if let Some(value) = inference.default_branch.clone() {
        let confidence = if default_branch
            .source
            .as_deref()
            .is_some_and(|source| source.contains("origin"))
        {
            Confidence::High
        } else {
            Confidence::Medium
        };
        inference.record_metadata(
            "default_branch",
            json!(value),
            option_source(default_branch.source.clone()),
            confidence,
            Vec::new(),
        );
    }
    if !inference.rust_crate_roots.is_empty() {
        let confidence = match rust_crate_roots.source_kind {
            RustCrateRootSourceKind::WorkspaceFallback => Confidence::Low,
            _ => Confidence::High,
        };
        inference.record_metadata(
            "rust_crate_roots",
            json!(inference.rust_crate_roots.clone()),
            rust_crate_roots.sources.clone(),
            confidence,
            Vec::new(),
        );
    }
    for (key, candidate) in [
        (
            "rust_fmt_check_command",
            commands.rust_fmt_check_command.as_ref(),
        ),
        ("rust_clippy_command", commands.rust_clippy_command.as_ref()),
        ("rust_test_command", commands.rust_test_command.as_ref()),
        (
            "rust_test_locked_command",
            commands.rust_test_locked_command.as_ref(),
        ),
    ] {
        if let Some(candidate) = candidate {
            inference.record_command_metadata(key, candidate);
        }
    }
}

fn record_frontend_and_ci_metadata(
    inference: &mut AdoptInference,
    package_manager: &PackageManagerInference,
    frontend_apps: &FrontendAppsInference,
    github_ci: &GithubCiInference,
) {
    if !inference.frontend_apps.is_empty() {
        if let Some(value) = inference.web_package_manager.clone() {
            inference.record_metadata(
                "web_package_manager",
                json!(value),
                package_manager.sources.clone(),
                Confidence::High,
                Vec::new(),
            );
        }
        inference.record_metadata(
            "frontend_apps",
            json!(inference.frontend_apps.clone()),
            frontend_apps.sources.clone(),
            Confidence::High,
            Vec::new(),
        );
    }
    if inference.application_contracts_enabled == Some(true) {
        inference.record_metadata(
            "application_contracts_enabled",
            json!(true),
            vec!["scripts/contracts.mjs".into()],
            Confidence::High,
            Vec::new(),
        );
    }
    if !inference.frontend_profiles.is_empty() {
        let sources = inference
            .frontend_profiles
            .iter()
            .flat_map(|profile| profile.sources.iter().cloned())
            .collect::<Vec<_>>();
        inference.record_metadata(
            "frontend_profiles",
            json!(inference.frontend_profiles.clone()),
            sources,
            Confidence::Medium,
            frontend_apps.warnings.clone(),
        );
    }
    if let Some(value) = inference.ci_github_runner.clone() {
        let confidence = if github_ci.runner_was_synthesized {
            Confidence::Low
        } else {
            Confidence::High
        };
        inference.record_metadata(
            "ci_github_runner",
            json!(value),
            github_ci.sources.clone(),
            confidence,
            github_ci.runner_warnings.clone(),
        );
    }
    if inference.ci_shape.has_workflows() {
        inference.record_metadata(
            "ci_shape",
            inference.ci_shape.report(),
            inference.ci_shape.sources(),
            Confidence::Medium,
            vec![
                "required checks are inferred from workflow job names; GitHub branch protection settings are not available locally"
                    .into(),
            ],
        );
    }
}

fn apply_sqlx_inference(inference: &mut AdoptInference, sqlx: &SqlxInference) {
    inference.sqlx_enabled = Some(sqlx.enabled.value);
    inference.rust_migration_dirs = sqlx.migration_dirs.value.clone();
    inference.signals.extend(sqlx.signals.clone());
    inference.record_metadata(
        "sqlx_enabled",
        json!(sqlx.enabled.value),
        sqlx.enabled.sources.clone(),
        if sqlx.enabled.value {
            Confidence::High
        } else {
            Confidence::Medium
        },
        Vec::new(),
    );
    record_sqlx_paths(inference, sqlx);
}

fn record_sqlx_paths(inference: &mut AdoptInference, sqlx: &SqlxInference) {
    if let Some(migration_dir) = &sqlx.migration_dir {
        inference.rust_migration_dir = Some(migration_dir.value.clone());
        inference.record_metadata(
            "rust_migration_dir",
            json!(migration_dir.value.clone()),
            migration_dir.sources.clone(),
            if sqlx.enabled.value && inference.rust_migration_dirs.is_empty() {
                Confidence::Low
            } else {
                Confidence::High
            },
            migration_dir.warnings.clone(),
        );
    }
    if !inference.rust_migration_dirs.is_empty() {
        inference.record_metadata(
            "rust_migration_dirs",
            json!(inference.rust_migration_dirs.clone()),
            sqlx.migration_dirs.sources.clone(),
            Confidence::High,
            sqlx.migration_dirs.warnings.clone(),
        );
    }
    if let Some(metadata_dir) = &sqlx.metadata_dir {
        inference.rust_sqlx_metadata_dir = Some(metadata_dir.value.clone());
        let synthesized = metadata_dir
            .sources
            .iter()
            .any(|source| source.starts_with("SQLx default"));
        inference.record_metadata(
            "rust_sqlx_metadata_dir",
            json!(metadata_dir.value.clone()),
            metadata_dir.sources.clone(),
            if synthesized {
                Confidence::Low
            } else {
                Confidence::High
            },
            metadata_dir.warnings.clone(),
        );
    }
    if let Some(check_command) = &sqlx.check_command {
        inference.sqlx_check_command = Some(check_command.value.clone());
        inference.record_metadata(
            "sqlx_check_command",
            json!(check_command.value.clone()),
            check_command.sources.clone(),
            Confidence::Medium,
            vec!["assumes online `cargo sqlx prepare --check` in a POSIX-like shell".into()],
        );
    }
}

fn record_inference_signals(inference: &mut AdoptInference, github_ci: &GithubCiInference) {
    if !inference.rust_crate_roots.is_empty() {
        inference.signals.push(format!(
            "Rust crate roots: {}",
            inference.rust_crate_roots.join(", ")
        ));
    }
    if !inference.frontend_apps.is_empty() {
        inference.signals.push(format!(
            "frontend apps: {}",
            inference
                .frontend_apps
                .iter()
                .map(|app| format!("{} at {}", app.name, app.dir))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !inference.frontend_apps.is_empty()
        && let Some(package_manager) = inference.web_package_manager.as_deref()
    {
        inference
            .signals
            .push(format!("package manager: {package_manager}"));
    }
    if let Some(runner) = inference.ci_github_runner.as_deref() {
        let suffix = if github_ci.runner_was_synthesized {
            " (synthesized supported-host fallback)"
        } else {
            ""
        };
        inference
            .signals
            .push(format!("GitHub runner: {runner}{suffix}"));
    }
    if inference.ci_shape.has_workflows() {
        inference.signals.push(format!(
            "GitHub workflows: {} file(s); generated Jig checks role: {}",
            inference.ci_shape.workflow_file_count(),
            inference.ci_shape.generated_jig_checks_role()
        ));
    }
}

pub(super) fn adoption_candidate_files(root: &Path) -> (Vec<std::path::PathBuf>, Vec<String>) {
    let mut warnings = Vec::new();
    let scan = RepoScan::collect(root, &mut warnings);
    (scan.files().to_vec(), warnings)
}

fn infer_application_contracts_enabled(
    root: &Path,
    scan: &RepoScan,
    has_frontend_apps: bool,
    warnings: &mut Vec<String>,
) -> bool {
    if !has_frontend_apps {
        return false;
    }
    let checker = root.join("scripts/contracts.mjs");
    if !scan
        .named_files("contracts.mjs")
        .any(|candidate| candidate == &checker)
    {
        return false;
    }
    let text = match read_limited_text(&checker) {
        Ok(text) => text,
        Err(error) => {
            warnings.push(format!(
                "could not validate scripts/contracts.mjs application-contract interface; leaving application contracts disabled: {error:#}"
            ));
            return false;
        }
    };
    if text
        .lines()
        .take(8)
        .any(|line| line.trim() == APPLICATION_CONTRACT_CHECKER_MARKER)
    {
        true
    } else {
        warnings.push(format!(
            "scripts/contracts.mjs does not declare the required `{APPLICATION_CONTRACT_CHECKER_MARKER}` interface marker; leaving application contracts disabled"
        ));
        false
    }
}

impl AdoptInference {
    pub(super) fn apply_to_answers(
        &self,
        answers: &mut AnswerOpts,
        answer_shape: &AnswerInputShape,
    ) {
        fill_string(
            &mut answers.repo_name,
            self.repo_name.as_deref(),
            answer_shape,
            "repo_name",
        );
        fill_string(
            &mut answers.default_branch,
            self.default_branch.as_deref(),
            answer_shape,
            "default_branch",
        );
        fill_string(
            &mut answers.ci_github_runner,
            self.ci_github_runner.as_deref(),
            answer_shape,
            "ci_github_runner",
        );
        fill_vec(
            &mut answers.rust_crate_roots,
            &self.rust_crate_roots,
            answer_shape,
            "rust_crate_roots",
        );
        fill_frontend_apps(
            &mut answers.frontend_apps,
            &self.frontend_apps,
            answer_shape,
        );
        // Workspace ownership is generated adoption policy, not a project
        // command override. Refresh it from the current declarations so added
        // members and exclusions cannot leave stale gate authorities behind.
        answers
            .frontend_workspace_roots
            .clone_from(&self.frontend_workspace_roots);
        if !answers.frontend_apps.is_empty() || answer_shape.contains_key("web_package_manager") {
            fill_string(
                &mut answers.web_package_manager,
                self.web_package_manager.as_deref(),
                answer_shape,
                "web_package_manager",
            );
        }
        if answers.application_contracts_enabled.is_none()
            && !answer_shape.contains_key("application_contracts_enabled")
        {
            answers.application_contracts_enabled = self.application_contracts_enabled;
        }
        fill_string(
            &mut answers.rust_fmt_check_command,
            self.rust_fmt_check_command.as_deref(),
            answer_shape,
            "rust_fmt_check_command",
        );
        fill_string(
            &mut answers.rust_clippy_command,
            self.rust_clippy_command.as_deref(),
            answer_shape,
            "rust_clippy_command",
        );
        fill_string(
            &mut answers.rust_test_command,
            self.rust_test_command.as_deref(),
            answer_shape,
            "rust_test_command",
        );
        fill_string(
            &mut answers.rust_test_locked_command,
            self.rust_test_locked_command.as_deref(),
            answer_shape,
            "rust_test_locked_command",
        );

        let explicit_sqlx_enabled = answer_shape.explicit_sqlx_enabled(answers);
        if answer_shape.should_apply_inferred_sqlx_enabled(answers) {
            answers.sqlx_enabled = self.sqlx_enabled;
        }
        if self.sqlx_enabled == Some(true) && explicit_sqlx_enabled != Some(false) {
            fill_string(
                &mut answers.rust_migration_dir,
                self.rust_migration_dir.as_deref(),
                answer_shape,
                "rust_migration_dir",
            );
            fill_string(
                &mut answers.rust_sqlx_metadata_dir,
                self.rust_sqlx_metadata_dir.as_deref(),
                answer_shape,
                "rust_sqlx_metadata_dir",
            );
            fill_string(
                &mut answers.sqlx_check_command,
                self.sqlx_check_command.as_deref(),
                answer_shape,
                "sqlx_check_command",
            );
        }
    }

    pub(super) fn summary(&self) -> String {
        let rust = if self.rust_crate_roots.is_empty() {
            "no Rust workspace".to_string()
        } else {
            format!(
                "{} ({})",
                self.rust_stack_label(),
                self.rust_crate_roots.join(", ")
            )
        };
        let sqlx = if self.sqlx_enabled == Some(true) {
            match self.rust_migration_dir.as_deref() {
                Some(dir) => format!("SQLx migrations at {dir}"),
                None => "SQLx".into(),
            }
        } else {
            "no SQLx".into()
        };
        let frontend = match self.frontend_apps.as_slice() {
            [] => "no frontend apps".to_string(),
            [app] => format!("one {} app at {}", app.kind, app.dir),
            apps => format!("{} frontend apps", apps.len()),
        };
        if self.frontend_apps.is_empty() {
            return format!("{rust}, {sqlx}, {frontend}");
        }
        let package_manager = self
            .web_package_manager
            .as_deref()
            .map(|value| format!("{value} lockfile"))
            .unwrap_or_else(|| "no web lockfile".into());
        format!("{rust}, {sqlx}, {frontend}, {package_manager}")
    }

    pub(super) fn adoption_review(
        &self,
        resolved_answers: &AnswerOpts,
        explicit_answers: &AnswerOpts,
        answer_shape: &AnswerInputShape,
    ) -> AdoptionReview {
        let mut items = Vec::new();
        items.push(format!("stack: {}", self.detected_stack_label()));
        if self.frontend_apps.is_empty() {
            items.push("frontend: no apps configured; web package-manager lockfiles are ignored until a frontend app is supplied".into());
        } else if let Some(package_manager) = resolved_answers.web_package_manager.as_deref() {
            items.push(format!(
                "frontend: {} app(s), using {package_manager}",
                self.frontend_apps.len()
            ));
            items.push(
                "frontend coverage: test:coverage must write coverage/coverage-summary.json in each app directory"
                    .into(),
            );
        }
        if self.sqlx_enabled == Some(true) {
            match resolved_answers.rust_migration_dir.as_deref() {
                Some(dir) => items.push(format!("SQLx: enabled with migrations at {dir}")),
                None => {
                    items.push(
                        "SQLx: enabled; confirm migration and metadata paths in .jig.toml".into(),
                    );
                }
            }
        }
        if matches!(
            self.rust_crate_root_source_kind,
            RustCrateRootSourceKind::WorkspaceFallback
        ) {
            items.push(
                "Rust: workspace fallback used; confirm rust_crate_roots in .jig.toml".into(),
            );
        } else if matches!(
            self.rust_crate_root_source_kind,
            RustCrateRootSourceKind::ScannedPackages
        ) {
            if self.command_profile.uses_nested_manifest_commands() {
                items.push(
                    "Rust: nested crates detected without a root Cargo.toml; generated Rust commands cover inferred manifests not handled by wrappers"
                        .into(),
                );
            } else {
                items.push(
                    "Rust: nested crates detected without a root Cargo.toml; wrapper Rust commands took precedence, so confirm they cover the inferred manifests"
                        .into(),
                );
            }
        }
        let overrides = self.overrides(explicit_answers, answer_shape);
        if !overrides.is_empty() {
            items.push(format!("overrides: {} explicit answer(s)", overrides.len()));
        }
        if !self.warnings.is_empty() {
            items.push(format!(
                "warnings: {} item(s); review before writing",
                self.warnings.len()
            ));
        }
        AdoptionReview { items }
    }

    pub(super) fn report(&self) -> JsonValue {
        json!({
            "summary": self.summary(),
            "scope": "inferred values before CLI and answers-file precedence is applied",
            "repo_name": self.repo_name,
            "default_branch": self.default_branch,
            "rust_crate_roots": self.rust_crate_roots,
            "sqlx_enabled": self.sqlx_enabled,
            "rust_migration_dir": self.rust_migration_dir,
            "rust_migration_dirs": self.rust_migration_dirs,
            "rust_sqlx_metadata_dir": self.rust_sqlx_metadata_dir,
            "rust_fmt_check_command": self.rust_fmt_check_command,
            "rust_clippy_command": self.rust_clippy_command,
            "rust_test_command": self.rust_test_command,
            "rust_test_locked_command": self.rust_test_locked_command,
            "web_package_manager": if self.frontend_apps.is_empty() {
                JsonValue::Null
            } else {
                json!(self.web_package_manager)
            },
            "application_contracts_enabled": self.application_contracts_enabled,
            "frontend_apps": self.frontend_apps,
            "frontend_workspace_roots": self.frontend_workspace_roots,
            "frontend_profiles": self.frontend_profiles,
            "ci_github_runner": self.ci_github_runner,
            "ci_shape": self.ci_shape.report(),
            "repo_topology": self.repo_topology.report(),
            "command_profile": self.command_profile.report(),
            "signals": self.signals,
            "warnings": self.warnings,
            "metadata": metadata::report(&self.metadata),
        })
    }

    pub(super) fn warnings(&self) -> &[String] {
        &self.warnings
    }

    fn record_metadata(
        &mut self,
        key: &str,
        value: JsonValue,
        sources: Vec<String>,
        confidence: Confidence,
        warnings: Vec<String>,
    ) {
        let previous = self.metadata.insert(
            key.into(),
            InferenceMetadata {
                value,
                sources,
                confidence,
                warnings,
            },
        );
        debug_assert!(
            previous.is_none(),
            "duplicate inference metadata key recorded: {key}"
        );
    }

    fn record_command_metadata(&mut self, key: &str, candidate: &CommandCandidate) {
        self.record_metadata(
            key,
            json!(candidate.command),
            vec![candidate.source.clone()],
            Confidence::from_str(candidate.confidence),
            candidate.warnings.clone(),
        );
    }
}

fn option_source(source: Option<String>) -> Vec<String> {
    source.into_iter().collect()
}

fn fill_string(
    target: &mut Option<String>,
    value: Option<&str>,
    answer_shape: &AnswerInputShape,
    key: &str,
) {
    if target.is_none() && !answer_shape.contains_key(key) {
        *target = value.map(str::to_string);
    }
}

fn fill_vec(
    target: &mut Vec<String>,
    value: &[String],
    answer_shape: &AnswerInputShape,
    key: &str,
) {
    if target.is_empty() && !value.is_empty() && !answer_shape.contains_key(key) {
        target.extend(value.iter().cloned());
    }
}

fn fill_frontend_apps(
    target: &mut Vec<FrontendApp>,
    value: &[FrontendApp],
    answer_shape: &AnswerInputShape,
) {
    if target.is_empty() && !value.is_empty() && !answer_shape.contains_key("frontend_apps") {
        target.extend(value.iter().cloned());
    }
}

#[cfg(test)]
mod tests;
