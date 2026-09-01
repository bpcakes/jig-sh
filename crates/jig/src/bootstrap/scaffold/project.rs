use crate::backend::{BackendLanguage, GoDatabase};
use crate::bootstrap::{APPLICATION_BACKEND_DEV_APP_NAME, DevApp, ScaffoldDb, ScaffoldPreset};

use super::frontend::FrontendScaffold;

#[derive(Clone, Debug)]
pub(super) enum ScaffoldProjectPlan {
    RustReact(RustReactScaffoldPlan),
    GoReact(GoReactScaffoldPlan),
    RustOnly(RustOnlyScaffoldPlan),
}

impl ScaffoldProjectPlan {
    pub(super) const fn identity(&self) -> ScaffoldIdentity {
        match self {
            Self::RustReact(_) => ScaffoldIdentity::RustReact,
            Self::GoReact(_) => ScaffoldIdentity::GoReact,
            Self::RustOnly(project) => project.artifact.identity(),
        }
    }

    pub(super) const fn database(&self) -> ScaffoldDb {
        match self {
            Self::RustReact(project) => project.backend.database,
            Self::GoReact(project) => match project.backend.database {
                GoDatabase::None => ScaffoldDb::None,
                GoDatabase::Postgres => ScaffoldDb::Postgres,
            },
            Self::RustOnly(_) => ScaffoldDb::None,
        }
    }

    pub(super) const fn summary_label(&self) -> &'static str {
        match self {
            Self::RustReact(_) => "Rust backend",
            Self::GoReact(_) => "Go backend",
            Self::RustOnly(project) => project.artifact.summary_label(),
        }
    }

    pub(super) const fn backend_language(&self) -> BackendLanguage {
        match self {
            Self::RustReact(_) | Self::RustOnly(_) => BackendLanguage::Rust,
            Self::GoReact(_) => BackendLanguage::Go,
        }
    }

    pub(super) fn react(&self) -> Option<&ReactScaffoldPlan> {
        match self {
            Self::RustReact(project) => Some(&project.react),
            Self::GoReact(project) => Some(&project.react),
            Self::RustOnly(_) => None,
        }
    }

    pub(super) fn application_backend_dev_app(&self, package_name: &str) -> Option<DevApp> {
        let (dir, argv) = match self {
            Self::RustReact(_) => (
                ".",
                vec![
                    "cargo".into(),
                    "run".into(),
                    "-p".into(),
                    format!("{package_name}-api"),
                ],
            ),
            Self::GoReact(project) => (
                project.backend.component_root.as_str(),
                vec!["go".into(), "run".into(), "./cmd/api".into()],
            ),
            Self::RustOnly(_) => return None,
        };
        Some(DevApp {
            name: APPLICATION_BACKEND_DEV_APP_NAME.into(),
            dir: Some(dir.into()),
            kind: "env-port".into(),
            command: None,
            argv,
            port: None,
            host: None,
            proxy: true,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ScaffoldIdentity {
    RustReact,
    GoReact,
    RustLibrary,
    RustCli,
}

impl ScaffoldIdentity {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::RustReact => "rust-react",
            Self::GoReact => "go-react",
            Self::RustLibrary => "rust-library",
            Self::RustCli => "rust-cli",
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct RustReactScaffoldPlan {
    pub(super) backend: RustScaffoldPlan,
    pub(super) react: ReactScaffoldPlan,
}

#[derive(Clone, Debug)]
pub(super) struct GoReactScaffoldPlan {
    pub(super) backend: GoScaffoldPlan,
    pub(super) react: ReactScaffoldPlan,
}

#[derive(Clone, Debug)]
pub(super) struct RustOnlyScaffoldPlan {
    pub(super) artifact: RustOnlyArtifact,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::bootstrap) enum RustOnlyArtifact {
    Library,
    Cli,
}

impl RustOnlyArtifact {
    pub(super) const fn identity(self) -> ScaffoldIdentity {
        match self {
            Self::Library => ScaffoldIdentity::RustLibrary,
            Self::Cli => ScaffoldIdentity::RustCli,
        }
    }

    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Library => "library",
            Self::Cli => "cli",
        }
    }

    const fn summary_label(self) -> &'static str {
        match self {
            Self::Library => "Rust library workspace",
            Self::Cli => "Rust CLI workspace",
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct ReactScaffoldPlan {
    /// The DNS-safe repo label used by Jig's development proxy.
    pub(super) repo_dns_label: String,
    pub(super) dev_proxy_port: u16,
    pub(super) dev_tld: String,
    pub(super) package_manager: String,
    pub(super) frontends: Vec<FrontendScaffold>,
    pub(super) custom_frontend_notices: Vec<String>,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct ReactBackendRenderContext<'a> {
    pub(super) preset: ScaffoldPreset,
    pub(super) database: ScaffoldDb,
    pub(super) root: &'a str,
    pub(super) migration_dir: &'a str,
    pub(super) sqlx_metadata_dir: &'a str,
}

#[derive(Clone, Debug)]
pub(super) struct RustScaffoldPlan {
    pub(super) database: ScaffoldDb,
    pub(super) migration_dir: String,
    pub(super) sqlx_metadata_dir: String,
}

#[derive(Clone, Debug)]
pub(super) struct GoScaffoldPlan {
    pub(super) database: GoDatabase,
    pub(super) module: String,
    pub(super) component_root: String,
    pub(super) migration_dir: String,
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use crate::bootstrap::{AnswerOpts, ScaffoldOpts};

    use super::super::InitScaffoldPlan;
    use super::*;

    fn plan_for(preset: ScaffoldPreset, db: Option<ScaffoldDb>) -> Option<InitScaffoldPlan> {
        let destination = tempdir().unwrap();
        let answers = if preset == ScaffoldPreset::GoReact {
            AnswerOpts {
                go_module: Some("example.com/ExampleProject".into()),
                ..AnswerOpts::default()
            }
        } else {
            AnswerOpts::default()
        };
        InitScaffoldPlan::from_opts(
            &ScaffoldOpts {
                preset: Some(preset),
                db,
                ..ScaffoldOpts::default()
            },
            &answers,
            destination.path(),
        )
        .unwrap()
    }

    #[test]
    fn rust_react_dispatches_to_typed_project() {
        let rust = plan_for(ScaffoldPreset::RustReact, Some(ScaffoldDb::None)).unwrap();
        assert!(matches!(&rust.project, ScaffoldProjectPlan::RustReact(_)));
        assert_eq!(rust.identity(), ScaffoldIdentity::RustReact);
        assert_eq!(rust.database(), ScaffoldDb::None);
        assert_eq!(rust.frontends().len(), 1);
    }

    #[test]
    fn go_react_dispatches_to_typed_project() {
        let go = plan_for(ScaffoldPreset::GoReact, Some(ScaffoldDb::Postgres)).unwrap();
        assert!(matches!(&go.project, ScaffoldProjectPlan::GoReact(_)));
        assert_eq!(go.identity(), ScaffoldIdentity::GoReact);
        assert_eq!(go.database(), ScaffoldDb::Postgres);
        assert_eq!(go.frontends().len(), 1);
    }

    #[test]
    fn harness_only_has_no_project_plan() {
        assert!(plan_for(ScaffoldPreset::HarnessOnly, None).is_none());
    }

    #[test]
    fn rust_library_dispatches_to_typed_project() {
        let library = plan_for(ScaffoldPreset::RustLibrary, None).unwrap();
        assert!(matches!(
            &library.project,
            ScaffoldProjectPlan::RustOnly(RustOnlyScaffoldPlan {
                artifact: RustOnlyArtifact::Library
            })
        ));
        assert_eq!(library.identity(), ScaffoldIdentity::RustLibrary);
        assert_eq!(library.database(), ScaffoldDb::None);
        assert!(library.frontends().is_empty());
    }

    #[test]
    fn rust_cli_dispatches_to_typed_project() {
        let cli = plan_for(ScaffoldPreset::RustCli, None).unwrap();
        assert!(matches!(
            &cli.project,
            ScaffoldProjectPlan::RustOnly(RustOnlyScaffoldPlan {
                artifact: RustOnlyArtifact::Cli
            })
        ));
        assert_eq!(cli.identity(), ScaffoldIdentity::RustCli);
        assert_eq!(cli.database(), ScaffoldDb::None);
        assert!(cli.frontends().is_empty());
    }
}
