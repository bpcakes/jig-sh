use serde_json::{Value as JsonValue, json};

use super::super::AnswerOpts;
use super::super::answers::AnswerInputShape;
use super::AdoptInference;
use super::rust_sqlx::RustCrateRootSourceKind;

const ASSUMPTION_SIGNAL_TOKEN: &str = "assumes";

impl AdoptInference {
    pub(in crate::bootstrap) fn adoption_profile_report(
        &self,
        generated_gates: &[String],
        managed_files: &[String],
        retired_managed_files: &[String],
        explicit_answers: &AnswerOpts,
        answer_shape: &AnswerInputShape,
    ) -> JsonValue {
        json!({
            "detected_stack": self.detected_stack(),
            "generated_gates": generated_gates,
            "managed_files": managed_files,
            "retired_managed_files": retired_managed_files,
            "frontend_profiles": self.frontend_profiles,
            "repo_topology": self.repo_topology.report(),
            "command_profile": self.command_profile.report(),
            "ci_shape": self.ci_shape.report(),
            "assumptions": self.assumptions(),
            "overrides": self.overrides(explicit_answers, answer_shape),
        })
    }

    pub(in crate::bootstrap) fn detected_stack_label(&self) -> String {
        let stack = self.detected_stack();
        if stack.is_empty() {
            "no application stack detected".into()
        } else {
            stack.join(", ")
        }
    }

    pub(super) fn rust_stack_label(&self) -> &'static str {
        if matches!(
            self.rust_crate_root_source_kind,
            RustCrateRootSourceKind::WorkspaceMembers | RustCrateRootSourceKind::WorkspaceFallback
        ) {
            "Rust workspace"
        } else {
            "Rust crate"
        }
    }

    fn detected_stack(&self) -> Vec<String> {
        let mut stack = Vec::new();
        if !self.rust_crate_roots.is_empty() {
            stack.push(self.rust_stack_label().into());
        }
        if self.sqlx_enabled == Some(true) {
            stack.push("SQLx".into());
        }
        if !self.frontend_apps.is_empty() {
            if let Some(package_manager) = self.web_package_manager.as_deref() {
                stack.push(package_manager.to_string());
            }
        }
        if self.frontend_apps.iter().any(|app| app.kind == "vite") {
            stack.push("Vite".into());
        } else if !self.frontend_apps.is_empty() {
            stack.push("frontend apps".into());
        }
        if self.ci_shape.has_workflows() || self.ci_github_runner.is_some() {
            stack.push("GitHub Actions".into());
        }
        stack
    }

    fn assumptions(&self) -> Vec<String> {
        let mut assumptions = self
            .signals
            .iter()
            .filter(|signal| signal.contains(ASSUMPTION_SIGNAL_TOKEN))
            .cloned()
            .collect::<Vec<_>>();
        assumptions.extend(self.warnings.iter().cloned());
        assumptions.push(
            "Generated command strings are starting points; review .jig.toml for repo-owned wrappers before relying on CI."
                .into(),
        );
        assumptions.sort();
        assumptions.dedup();
        assumptions
    }

    pub(super) fn overrides(
        &self,
        explicit_answers: &AnswerOpts,
        answer_shape: &AnswerInputShape,
    ) -> Vec<String> {
        let mut overrides = Vec::new();
        for (key, inferred, cli_value) in [
            (
                "repo_name",
                self.repo_name.as_deref(),
                explicit_answers.repo_name.as_ref(),
            ),
            (
                "default_branch",
                self.default_branch.as_deref(),
                explicit_answers.default_branch.as_ref(),
            ),
            (
                "ci_github_runner",
                self.ci_github_runner.as_deref(),
                explicit_answers.ci_github_runner.as_ref(),
            ),
            (
                "web_package_manager",
                self.web_package_manager.as_deref(),
                explicit_answers.web_package_manager.as_ref(),
            ),
            (
                "rust_migration_dir",
                self.rust_migration_dir.as_deref(),
                explicit_answers.rust_migration_dir.as_ref(),
            ),
            (
                "rust_sqlx_metadata_dir",
                self.rust_sqlx_metadata_dir.as_deref(),
                explicit_answers.rust_sqlx_metadata_dir.as_ref(),
            ),
            (
                "sqlx_check_command",
                self.sqlx_check_command.as_deref(),
                explicit_answers.sqlx_check_command.as_ref(),
            ),
            (
                "rust_fmt_check_command",
                self.rust_fmt_check_command.as_deref(),
                explicit_answers.rust_fmt_check_command.as_ref(),
            ),
            (
                "rust_clippy_command",
                self.rust_clippy_command.as_deref(),
                explicit_answers.rust_clippy_command.as_ref(),
            ),
            (
                "rust_test_command",
                self.rust_test_command.as_deref(),
                explicit_answers.rust_test_command.as_ref(),
            ),
            (
                "rust_test_locked_command",
                self.rust_test_locked_command.as_deref(),
                explicit_answers.rust_test_locked_command.as_ref(),
            ),
        ] {
            self.push_option_override(
                &mut overrides,
                key,
                inferred,
                explicit_string_source(cli_value, answer_shape, key),
            );
        }
        if !self.rust_crate_roots.is_empty()
            && explicit_vec_source(
                &explicit_answers.rust_crate_roots,
                answer_shape,
                "rust_crate_roots",
            )
            .is_some()
        {
            overrides.push(format!(
                "rust_crate_roots: inferred {} ignored because an explicit answer was supplied",
                self.rust_crate_roots.join(", ")
            ));
        }
        if !self.frontend_apps.is_empty()
            && explicit_vec_source(
                &explicit_answers.frontend_apps,
                answer_shape,
                "frontend_apps",
            )
            .is_some()
        {
            let dirs = self
                .frontend_apps
                .iter()
                .map(|app| app.dir.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            overrides.push(format!(
                "frontend_apps: inferred {dirs} ignored because an explicit answer was supplied"
            ));
        }
        if let Some(source) =
            explicit_bool_source(explicit_answers.sqlx_enabled, answer_shape, "sqlx_enabled")
        {
            if let Some(value) = self.sqlx_enabled {
                overrides.push(format!(
                    "sqlx_enabled: inferred {value} ignored because {source} supplied an explicit answer"
                ));
            }
        }
        overrides
    }

    fn push_option_override(
        &self,
        overrides: &mut Vec<String>,
        key: &str,
        inferred: Option<&str>,
        explicit_source: Option<&'static str>,
    ) {
        if let (Some(value), Some(source)) = (inferred, explicit_source) {
            overrides.push(format!(
                "{key}: inferred {value} ignored because {source} supplied an explicit answer"
            ));
        }
    }
}

fn explicit_string_source(
    cli_value: Option<&String>,
    answer_shape: &AnswerInputShape,
    key: &str,
) -> Option<&'static str> {
    explicit_source(cli_value.is_some(), answer_shape, key)
}

fn explicit_bool_source(
    cli_value: Option<bool>,
    answer_shape: &AnswerInputShape,
    key: &str,
) -> Option<&'static str> {
    explicit_source(cli_value.is_some(), answer_shape, key)
}

fn explicit_vec_source<T>(
    cli_value: &[T],
    answer_shape: &AnswerInputShape,
    key: &str,
) -> Option<&'static str> {
    explicit_source(!cli_value.is_empty(), answer_shape, key)
}

fn explicit_source(
    supplied_by_cli: bool,
    answer_shape: &AnswerInputShape,
    key: &str,
) -> Option<&'static str> {
    if supplied_by_cli {
        Some("CLI")
    } else if answer_shape.contains_key(key) {
        Some("answers file")
    } else {
        None
    }
}
