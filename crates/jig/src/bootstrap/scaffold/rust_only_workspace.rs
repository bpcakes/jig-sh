use std::path::PathBuf;

use anyhow::Result;
use serde_json::json;

use super::templates::{
    ScaffoldTemplateFile, ensure_scaffold_template_paths, render_scaffold_template,
};
use super::write::{ScaffoldFile, scaffold_file};
use super::{InitScaffoldPlan, RustOnlyArtifact, RustOnlyScaffoldPlan};

const RUST_ONLY_RUST_VERSION: &str = env!("CARGO_PKG_RUST_VERSION");

const RUST_ONLY_WORKSPACE_TEMPLATES: &[ScaffoldTemplateFile] = &[
    ScaffoldTemplateFile {
        template: "rust-only/workspace/Cargo.toml.jinja",
        output: "Cargo.toml",
    },
    ScaffoldTemplateFile {
        template: "rust-only/workspace/README.md.jinja",
        output: "README.md",
    },
    ScaffoldTemplateFile {
        template: "rust-common/workspace/clippy.toml.jinja",
        output: "clippy.toml",
    },
    ScaffoldTemplateFile {
        template: "rust-only/workspace/crate/Cargo.toml.jinja",
        output: "crates/{package}/Cargo.toml",
    },
    ScaffoldTemplateFile {
        template: "rust-only/workspace/crate/AGENTS.md.jinja",
        output: "crates/{package}/AGENTS.md",
    },
];

const RUST_LIBRARY_SOURCE_TEMPLATE: ScaffoldTemplateFile = ScaffoldTemplateFile {
    template: "rust-library/crate/src/lib.rs.jinja",
    output: "crates/{package}/src/lib.rs",
};

const RUST_CLI_SOURCE_TEMPLATE: ScaffoldTemplateFile = ScaffoldTemplateFile {
    template: "rust-cli/crate/src/main.rs.jinja",
    output: "crates/{package}/src/main.rs",
};

impl RustOnlyArtifact {
    const fn source_template(self) -> ScaffoldTemplateFile {
        match self {
            Self::Library => RUST_LIBRARY_SOURCE_TEMPLATE,
            Self::Cli => RUST_CLI_SOURCE_TEMPLATE,
        }
    }
}

impl InitScaffoldPlan {
    pub(super) fn render_rust_only_workspace_files(
        &self,
        project: &RustOnlyScaffoldPlan,
    ) -> Result<Vec<ScaffoldFile>> {
        ensure_scaffold_template_paths(RUST_ONLY_WORKSPACE_TEMPLATES)?;
        let source_template = project.artifact.source_template();
        ensure_scaffold_template_paths(&[source_template])?;
        let context = self.rust_only_workspace_template_context(project);
        RUST_ONLY_WORKSPACE_TEMPLATES
            .iter()
            .chain(std::iter::once(&source_template))
            .map(|file| {
                Ok(scaffold_file(
                    self.template_output_path(file),
                    render_scaffold_template(file.template, &context)?,
                ))
            })
            .collect()
    }

    pub(super) fn rust_only_workspace_relative_paths(
        &self,
        project: &RustOnlyScaffoldPlan,
    ) -> Vec<PathBuf> {
        let source_template = project.artifact.source_template();
        RUST_ONLY_WORKSPACE_TEMPLATES
            .iter()
            .chain(std::iter::once(&source_template))
            .map(|file| PathBuf::from(self.template_output_path(file)))
            .collect()
    }

    fn rust_only_workspace_template_context(
        &self,
        project: &RustOnlyScaffoldPlan,
    ) -> serde_json::Value {
        json!({
            "repo_name": self.repo_name,
            "package_name": self.package_name,
            "artifact_kind": project.artifact.as_str(),
            "rust_version": RUST_ONLY_RUST_VERSION,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::process::Command;

    use clap::ValueEnum;
    use serde_json::json;
    use tempfile::tempdir;

    use super::super::embedded_templates::snapshot_scaffold_template_files;
    use super::super::templates::{render_scaffold_template, render_scaffold_template_from_files};
    use super::super::{AnswerOpts, ScaffoldDb, ScaffoldPreset};
    use super::*;

    fn plan(artifact: RustOnlyArtifact, repo_name: &str) -> InitScaffoldPlan {
        let destination = tempdir().unwrap();
        InitScaffoldPlan::rust_only(
            artifact,
            &AnswerOpts {
                repo_name: Some(repo_name.into()),
                ..AnswerOpts::default()
            },
            destination.path(),
        )
        .unwrap()
    }

    fn rendered_by_path(plan: &InitScaffoldPlan) -> BTreeMap<String, String> {
        plan.render_files()
            .unwrap()
            .into_iter()
            .map(|file| (file.relative, file.contents))
            .collect()
    }

    fn snapshot_rendered_by_path(
        plan: &InitScaffoldPlan,
        artifact: RustOnlyArtifact,
    ) -> BTreeMap<String, String> {
        let project = RustOnlyScaffoldPlan { artifact };
        let source_template = artifact.source_template();
        let context = plan.rust_only_workspace_template_context(&project);
        RUST_ONLY_WORKSPACE_TEMPLATES
            .iter()
            .chain(std::iter::once(&source_template))
            .map(|file| {
                (
                    plan.template_output_path(file),
                    render_scaffold_template_from_files(
                        snapshot_scaffold_template_files(),
                        file.template,
                        &context,
                    )
                    .unwrap(),
                )
            })
            .collect()
    }

    fn assert_workspace_manifest(
        root: &toml::Value,
        package: &toml::Value,
        rendered: &BTreeMap<String, String>,
    ) {
        assert_eq!(root["workspace"]["resolver"].as_str(), Some("3"));
        assert_eq!(
            root["workspace"]["members"][0].as_str(),
            Some("crates/exampleproject")
        );
        assert_eq!(
            root["workspace"]["package"]["rust-version"].as_str(),
            Some(env!("CARGO_PKG_RUST_VERSION"))
        );
        assert_eq!(
            root["workspace"]["lints"]["clippy"]["cognitive_complexity"].as_str(),
            Some("warn")
        );
        assert_eq!(package["lints"]["workspace"].as_bool(), Some(true));
        assert_eq!(
            rendered.get("clippy.toml").map(String::as_str),
            Some("cognitive-complexity-threshold = 20\n")
        );
        assert_eq!(package["package"]["publish"].as_bool(), Some(false));
    }

    fn assert_license_neutral(
        root: &toml::Value,
        package: &toml::Value,
        rendered: &BTreeMap<String, String>,
    ) {
        assert!(root.get("license").is_none());
        assert!(root["workspace"]["package"].get("license").is_none());
        assert!(package["package"].get("license").is_none());
        assert!(package["package"].get("license-file").is_none());
        assert!(rendered.keys().all(|path| !path.starts_with("LICENSE")));
    }

    fn assert_artifact_manifest(
        artifact: RustOnlyArtifact,
        package: &toml::Value,
        rendered: &BTreeMap<String, String>,
    ) {
        match artifact {
            RustOnlyArtifact::Library => {
                assert!(package.get("bin").is_none());
                let source = rendered.get("crates/exampleproject/src/lib.rs").unwrap();
                assert!(source.starts_with("//! Library entry point"));
                assert!(!source.contains("pub "));
            }
            RustOnlyArtifact::Cli => {
                let bins = package["bin"].as_array().unwrap();
                assert_eq!(bins.len(), 1);
                assert_eq!(bins[0]["name"].as_str(), Some("exampleproject"));
                assert_eq!(bins[0]["path"].as_str(), Some("src/main.rs"));
                let source = rendered.get("crates/exampleproject/src/main.rs").unwrap();
                assert!(source.contains("env!(\"CARGO_PKG_NAME\")"));
                assert!(source.contains("env!(\"CARGO_PKG_VERSION\")"));
            }
        }
    }

    fn assert_member_manifest_resolves(root: &toml::Value, rendered: &BTreeMap<String, String>) {
        let destination = tempdir().unwrap();
        for (relative, contents) in rendered {
            let output = destination.path().join(relative);
            fs::create_dir_all(output.parent().unwrap()).unwrap();
            fs::write(output, contents).unwrap();
        }
        let member = root["workspace"]["members"][0].as_str().unwrap();
        assert!(destination.path().join(member).join("Cargo.toml").is_file());
    }

    #[test]
    fn rust_only_rendered_paths_and_bytes_match_the_checked_in_snapshot() {
        for artifact in [RustOnlyArtifact::Library, RustOnlyArtifact::Cli] {
            let plan = plan(artifact, "ExampleProject");
            assert_eq!(
                rendered_by_path(&plan),
                snapshot_rendered_by_path(&plan, artifact),
                "{} rendered output drifted from the checked-in snapshot",
                artifact.as_str()
            );
        }
    }

    #[test]
    fn library_and_cli_render_exact_artifact_specific_output_sets() {
        for (artifact, source) in [
            (RustOnlyArtifact::Library, "src/lib.rs"),
            (RustOnlyArtifact::Cli, "src/main.rs"),
        ] {
            let plan = plan(artifact, "ExampleProject");
            let rendered_files = plan.render_files().unwrap();
            let rendered_paths = rendered_files
                .iter()
                .map(|file| PathBuf::from(&file.relative))
                .collect::<Vec<_>>();
            let rendered = rendered_files
                .into_iter()
                .map(|file| (file.relative, file.contents))
                .collect::<BTreeMap<_, _>>();
            assert_eq!(
                rendered.keys().map(String::as_str).collect::<Vec<_>>(),
                [
                    "Cargo.toml",
                    "README.md",
                    "clippy.toml",
                    "crates/exampleproject/AGENTS.md",
                    "crates/exampleproject/Cargo.toml",
                    &format!("crates/exampleproject/{source}"),
                ]
            );
            assert_eq!(plan.output_paths(), rendered_paths);
            let unterminated = rendered
                .iter()
                .filter_map(|(path, contents)| (!contents.ends_with('\n')).then_some(path))
                .collect::<Vec<_>>();
            assert!(
                unterminated.is_empty(),
                "unterminated files: {unterminated:?}"
            );

            let readme = rendered.get("README.md").unwrap();
            let guide = rendered.get("crates/exampleproject/AGENTS.md").unwrap();
            match artifact {
                RustOnlyArtifact::Library => {
                    assert!(readme.contains("Rust library"));
                    assert!(guide.contains("initial Rust library"));
                }
                RustOnlyArtifact::Cli => {
                    assert!(readme.contains("Rust command-line application"));
                    assert!(readme.contains("cargo run -p exampleproject"));
                    assert!(guide.contains("initial Rust command-line artifact"));
                }
            }
        }
    }

    #[test]
    fn manifests_are_resolvable_license_neutral_and_use_the_workspace_msrv() {
        for artifact in [RustOnlyArtifact::Library, RustOnlyArtifact::Cli] {
            let plan = plan(artifact, "ExampleProject");
            let rendered = rendered_by_path(&plan);
            let root = rendered.get("Cargo.toml").unwrap();
            let package = rendered.get("crates/exampleproject/Cargo.toml").unwrap();
            let root_toml = toml::from_str::<toml::Value>(root).unwrap();
            let package_toml = toml::from_str::<toml::Value>(package).unwrap();
            assert_workspace_manifest(&root_toml, &package_toml, &rendered);
            assert_license_neutral(&root_toml, &package_toml, &rendered);
            assert_artifact_manifest(artifact, &package_toml, &rendered);
            assert_member_manifest_resolves(&root_toml, &rendered);
        }
    }

    #[test]
    fn artifact_sources_are_rustfmt_stable() {
        for artifact in [RustOnlyArtifact::Library, RustOnlyArtifact::Cli] {
            let plan = plan(artifact, "ExampleProject");
            let rendered = rendered_by_path(&plan);
            let source = rendered
                .iter()
                .find(|(path, _)| path.ends_with(".rs"))
                .unwrap();
            let directory = tempdir().unwrap();
            let path = directory.path().join("source.rs");
            fs::write(&path, source.1).unwrap();
            let status = Command::new("rustfmt")
                .args(["--check", "--edition", "2024"])
                .arg(&path)
                .status()
                .unwrap();
            assert!(status.success(), "rustfmt rejected {}", source.0);
        }
    }

    #[test]
    fn strict_template_errors_name_missing_sources_and_context() {
        let missing = render_scaffold_template("rust-only/missing.jinja", &json!({}))
            .unwrap_err()
            .to_string();
        assert_eq!(
            missing,
            "Scaffold template rust-only/missing.jinja was not embedded"
        );

        let missing_context = render_scaffold_template(
            "rust-only/workspace/Cargo.toml.jinja",
            &json!({"rust_version": RUST_ONLY_RUST_VERSION}),
        )
        .unwrap_err();
        let error = format!("{missing_context:#}");
        assert!(
            error.contains("rust-only/workspace/Cargo.toml.jinja"),
            "{error}"
        );
        assert!(error.contains("undefined value"), "{error}");
    }

    #[test]
    fn package_names_normalize_and_reuse_the_existing_maximum_boundary() {
        let normalized = plan(RustOnlyArtifact::Library, "123 ExampleProject");
        assert_eq!(normalized.package_name, "app-123-exampleproject");

        let boundary = "r".repeat(216);
        let accepted = plan(RustOnlyArtifact::Cli, &boundary);
        assert_eq!(accepted.package_name, boundary);
        let rejected = InitScaffoldPlan::rust_only(
            RustOnlyArtifact::Cli,
            &AnswerOpts {
                repo_name: Some("r".repeat(217)),
                ..AnswerOpts::default()
            },
            tempdir().unwrap().path(),
        )
        .unwrap_err()
        .to_string();
        assert!(rejected.contains("at most 216 bytes"), "{rejected}");
    }

    #[test]
    fn plans_have_private_identities_and_no_backend_or_web_state() {
        for (artifact, identity) in [
            (RustOnlyArtifact::Library, "rust-library"),
            (RustOnlyArtifact::Cli, "rust-cli"),
        ] {
            let plan = plan(artifact, "ExampleProject");
            assert_eq!(plan.identity().as_str(), identity);
            assert_eq!(
                plan.project.backend_language(),
                crate::backend::BackendLanguage::Rust
            );
            assert_eq!(plan.database(), ScaffoldDb::None);
            assert!(plan.frontends().is_empty());
            assert!(plan.custom_frontend_notices().is_empty());
            assert!(!plan.database_enabled());
            assert!(!plan.scaffolds_frontend_contracts());
            assert!(!plan.scaffolds_go_postgres_integration());

            let mut answers = AnswerOpts::default();
            plan.apply_answer_defaults(&mut answers);
            assert_eq!(answers.sqlx_enabled, Some(false));
            assert_eq!(answers.rust_crate_roots, ["crates"]);
            assert!(answers.web_package_manager.is_none());
            assert!(answers.frontend_apps.is_empty());
            assert!(answers.dev_apps.is_empty());
            assert!(answers.rust_migration_dir.is_none());
            assert!(answers.migration_dir.is_none());
        }

        assert_eq!(
            ScaffoldPreset::value_variants()
                .iter()
                .map(|preset| preset.as_str())
                .collect::<Vec<_>>(),
            [
                "rust-react",
                "go-react",
                "harness-only",
                "rust-library",
                "rust-cli",
            ]
        );
    }

    #[test]
    fn reports_use_rust_only_identity_strings_for_both_artifacts() {
        for (artifact, identity) in [
            (RustOnlyArtifact::Library, "rust-library"),
            (RustOnlyArtifact::Cli, "rust-cli"),
        ] {
            let plan = plan(artifact, "ExampleProject");
            let destination = tempdir().unwrap();
            let report = plan.write(destination.path(), false).unwrap();
            assert_eq!(report["preset"], identity);
            assert_eq!(report["db"], "none");
            assert_eq!(report["frontends"], json!([]));
        }
    }
}
