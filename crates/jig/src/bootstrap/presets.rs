use clap::ValueEnum;
use serde::Serialize;
use serde_json::{Value, json};

use super::{
    APPLICATION_BACKEND_DEV_APP_NAME, RUST_REACT_ADMIN_BACKEND_DEV_APP_NAME, ScaffoldPreset,
};
use crate::backend::BackendLanguage;

#[derive(Clone, Copy, Debug, Serialize)]
pub(crate) struct ScaffoldPresetDescriptor {
    name: &'static str,
    summary: &'static str,
    defaults: &'static [&'static str],
    layout: &'static [&'static str],
    frontend_shorthands: &'static [ScaffoldFrontendShorthand],
    examples: &'static [&'static str],
    ownership: &'static str,
    non_goals: &'static [&'static str],
}

impl ScaffoldPresetDescriptor {
    pub(crate) const fn summary(self) -> &'static str {
        self.summary
    }

    pub(crate) const fn frontend_shorthands(self) -> &'static [ScaffoldFrontendShorthand] {
        self.frontend_shorthands
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
pub(crate) struct ScaffoldFrontendShorthand {
    name: &'static str,
    expands_to: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ScaffoldChoiceCapability {
    supported: bool,
    required: bool,
}

impl ScaffoldChoiceCapability {
    const UNSUPPORTED: Self = Self::new(false, false);
    const REQUIRED: Self = Self::new(true, true);

    const fn new(supported: bool, required: bool) -> Self {
        assert!(
            !required || supported,
            "a required scaffold choice must be supported"
        );
        Self {
            supported,
            required,
        }
    }

    const fn is_supported(self) -> bool {
        self.supported
    }

    const fn is_required(self) -> bool {
        self.required
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ScaffoldPresetCapabilities {
    has_project_scaffold: bool,
    database: ScaffoldChoiceCapability,
    frontends: ScaffoldChoiceCapability,
    go_module: ScaffoldChoiceCapability,
}

impl ScaffoldFrontendShorthand {
    pub(crate) const fn name(self) -> &'static str {
        self.name
    }

    pub(crate) const fn expands_to(self) -> &'static str {
        self.expands_to
    }
}

pub fn scaffold_presets_report() -> Value {
    let presets = ScaffoldPreset::value_variants()
        .iter()
        .copied()
        .map(ScaffoldPreset::descriptor)
        .collect::<Vec<_>>();
    json!({
        "ok": true,
        "command": crate::tool_defs::cli_command::PRESETS,
        "presets": presets
    })
}

impl ScaffoldPreset {
    const fn capabilities(self) -> ScaffoldPresetCapabilities {
        match self {
            Self::RustReact => ScaffoldPresetCapabilities {
                has_project_scaffold: true,
                database: ScaffoldChoiceCapability::REQUIRED,
                frontends: ScaffoldChoiceCapability::REQUIRED,
                go_module: ScaffoldChoiceCapability::UNSUPPORTED,
            },
            Self::GoReact => ScaffoldPresetCapabilities {
                has_project_scaffold: true,
                database: ScaffoldChoiceCapability::REQUIRED,
                frontends: ScaffoldChoiceCapability::REQUIRED,
                go_module: ScaffoldChoiceCapability::REQUIRED,
            },
            Self::HarnessOnly => ScaffoldPresetCapabilities {
                has_project_scaffold: false,
                database: ScaffoldChoiceCapability::UNSUPPORTED,
                frontends: ScaffoldChoiceCapability::UNSUPPORTED,
                go_module: ScaffoldChoiceCapability::UNSUPPORTED,
            },
            Self::RustLibrary | Self::RustCli => ScaffoldPresetCapabilities {
                has_project_scaffold: true,
                database: ScaffoldChoiceCapability::UNSUPPORTED,
                frontends: ScaffoldChoiceCapability::UNSUPPORTED,
                go_module: ScaffoldChoiceCapability::UNSUPPORTED,
            },
        }
    }

    pub(crate) const fn has_project_scaffold(self) -> bool {
        self.capabilities().has_project_scaffold
    }

    pub(crate) const fn supports_database(self) -> bool {
        self.capabilities().database.is_supported()
    }

    pub(crate) const fn requires_database_choice(self) -> bool {
        self.capabilities().database.is_required()
    }

    pub(crate) const fn supports_frontends(self) -> bool {
        self.capabilities().frontends.is_supported()
    }

    pub(crate) const fn requires_frontend_choice(self) -> bool {
        self.capabilities().frontends.is_required()
    }

    pub(crate) const fn supports_go_module(self) -> bool {
        self.capabilities().go_module.is_supported()
    }

    pub(crate) const fn requires_go_module(self) -> bool {
        self.capabilities().go_module.is_required()
    }

    pub(crate) const fn requires_web_package_manager(self) -> bool {
        self.supports_frontends()
    }

    pub(crate) const fn project_scaffold_label(self) -> Option<&'static str> {
        match self {
            Self::RustReact => Some("Rust React"),
            Self::GoReact => Some("Go React"),
            Self::HarnessOnly => None,
            Self::RustLibrary => Some("Rust library"),
            Self::RustCli => Some("Rust CLI"),
        }
    }

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::RustReact => "rust-react",
            Self::GoReact => "go-react",
            Self::HarnessOnly => "harness-only",
            Self::RustLibrary => "rust-library",
            Self::RustCli => "rust-cli",
        }
    }

    pub(crate) const fn generated_backend_language(self) -> Option<BackendLanguage> {
        match self {
            Self::RustReact => Some(BackendLanguage::Rust),
            Self::GoReact => Some(BackendLanguage::Go),
            Self::HarnessOnly => None,
            Self::RustLibrary | Self::RustCli => Some(BackendLanguage::Rust),
        }
    }

    pub(crate) const fn reserved_backend_dev_app_names(self) -> &'static [&'static str] {
        match self {
            Self::RustReact => &[
                APPLICATION_BACKEND_DEV_APP_NAME,
                RUST_REACT_ADMIN_BACKEND_DEV_APP_NAME,
            ],
            Self::GoReact => &[APPLICATION_BACKEND_DEV_APP_NAME],
            Self::HarnessOnly | Self::RustLibrary | Self::RustCli => &[],
        }
    }

    pub(crate) const fn reserved_backend_roots(self) -> &'static [&'static str] {
        match self {
            Self::RustReact => &["apps", "crates"],
            Self::GoReact => &["cmd", "internal"],
            Self::HarnessOnly | Self::RustLibrary | Self::RustCli => &[],
        }
    }

    pub(crate) const fn descriptor(self) -> ScaffoldPresetDescriptor {
        match self {
            Self::RustReact => ScaffoldPresetDescriptor {
                name: "rust-react",
                summary: "Rust API workspace plus shadcn React product/admin apps and an optional Astro site.",
                defaults: &[
                    "Rust crate roots default to apps and crates.",
                    "The strict Clippy gate rejects functions when Clippy's cognitive-complexity heuristic exceeds 20.",
                    "Frontends default to web when omitted.",
                    "Database scaffolding defaults to none; pass --db postgres or --db sqlite when wanted.",
                    "Generated frontend checks default to bun unless --web-package-manager is supplied.",
                    "Frontends share a pinned root workspace and install dependencies once during bootstrap.",
                    "React frontends ship tested shadcn 4 sources and provenance without running a mutable CLI during init.",
                    "Schema dumps stay disabled until a command is configured.",
                ],
                layout: &[
                    "apps/<repo>-api",
                    "crates/<repo>-core",
                    "crates/<repo>",
                    "crates/<repo>-http",
                    "crates/<repo>-test-support",
                    "crates/<repo>-db when --db postgres or --db sqlite is selected",
                ],
                frontend_shorthands: &[
                    ScaffoldFrontendShorthand {
                        name: "web",
                        expands_to: "shadcn Vite React product app in web/",
                    },
                    ScaffoldFrontendShorthand {
                        name: "landing",
                        expands_to: "Astro site in landing/",
                    },
                    ScaffoldFrontendShorthand {
                        name: "admin",
                        expands_to: "shadcn Vite React admin app in admin-panel/",
                    },
                ],
                examples: &[
                    "jig init ./my-app --preset rust-react",
                    "jig init ./my-app --preset rust-react --db postgres --frontends web,landing,admin",
                    "jig init ./my-app --preset rust-react --db sqlite --frontends web",
                ],
                ownership: "Scaffolded application code is project-owned after creation; jig update keeps the Jig harness current and does not rewrite app code.",
                non_goals: &[
                    "jig update does not migrate or overwrite scaffolded application source.",
                    "Presets are starter shapes, not long-term application frameworks.",
                ],
            },
            Self::GoReact => ScaffoldPresetDescriptor {
                name: "go-react",
                summary: "Go 1.26 chi/Huma API plus a shadcn React product app and optional Astro site.",
                defaults: &[
                    "A Go module is required; --defaults derives example.com/<repo>.",
                    "Frontends default to web when omitted.",
                    "Database scaffolding defaults to none; PostgreSQL uses pgxpool, sqlc, and Goose.",
                    "Generated frontend checks default to bun unless --web-package-manager is supplied.",
                ],
                layout: &[
                    "cmd/api and cmd/openapi",
                    "internal/config and internal/httpapi",
                    "internal/database (including embedded Goose migrations) and sqlc.yaml with --db postgres",
                    "web and packages/public-api-client",
                ],
                frontend_shorthands: &[
                    ScaffoldFrontendShorthand {
                        name: "web",
                        expands_to: "shadcn Vite React product app in web/",
                    },
                    ScaffoldFrontendShorthand {
                        name: "landing",
                        expands_to: "Astro site in landing/",
                    },
                ],
                examples: &[
                    "jig init ./my-app --preset go-react --go-module github.com/acme/my-app --db none --frontends web",
                    "jig init ./my-app --preset go-react --go-module github.com/acme/my-app --db postgres --frontends web,landing",
                ],
                ownership: "Scaffolded application code is project-owned after creation; jig update keeps the Jig harness current and does not rewrite app code.",
                non_goals: &[
                    "The initial Go preset does not support SQLite or the privileged admin API/client boundary.",
                    "jig update does not migrate or overwrite scaffolded application source.",
                ],
            },
            Self::HarnessOnly => ScaffoldPresetDescriptor {
                name: "harness-only",
                summary: "Jig harness configuration without starter application code.",
                defaults: &[
                    "SQLx defaults to disabled unless SQLx-shaped answers are supplied.",
                    "Existing frontend_apps answers are retained as project-owned configuration.",
                ],
                layout: &[],
                frontend_shorthands: &[],
                examples: &["jig init ./my-repo --preset harness-only --no-input --no-vault"],
                ownership: "Only the Jig harness is generated; application source remains entirely project-owned.",
                non_goals: &[
                    "The harness-only preset does not create Rust crates, databases, or frontend applications.",
                ],
            },
            Self::RustLibrary => ScaffoldPresetDescriptor {
                name: "rust-library",
                summary: "Expandable Rust workspace with one library crate.",
                defaults: &[
                    "The virtual workspace uses crates/<repo> as its only initial member.",
                    "Rust 2024 uses the top-level Jig workspace Rust baseline.",
                    "The strict Clippy gate rejects functions when Clippy's cognitive-complexity heuristic exceeds 20.",
                    "SQLx, schema dumps, application contracts, frontends, and dev apps are disabled.",
                ],
                layout: &[
                    "Cargo.toml virtual workspace",
                    "crates/<repo> library crate",
                ],
                frontend_shorthands: &[],
                examples: &[
                    "jig init ./example-library --preset rust-library --no-input --no-vault",
                ],
                ownership: "The generated Cargo and Clippy configuration, Rust source, crate guide, and README are project-owned after creation; jig update keeps only the Jig harness current.",
                non_goals: &[
                    "The rust-library preset does not create a database, frontend, API, dev app, release workflow, or additional crate layers.",
                    "The scaffold does not select a license or enable package publication.",
                ],
            },
            Self::RustCli => ScaffoldPresetDescriptor {
                name: "rust-cli",
                summary: "Expandable Rust workspace with one command-line binary crate.",
                defaults: &[
                    "The virtual workspace uses crates/<repo> as its only initial member.",
                    "Rust 2024 uses the top-level Jig workspace Rust baseline.",
                    "The strict Clippy gate rejects functions when Clippy's cognitive-complexity heuristic exceeds 20.",
                    "The starter binary uses only std and prints its package name and version.",
                    "SQLx, schema dumps, application contracts, frontends, and dev apps are disabled.",
                ],
                layout: &[
                    "Cargo.toml virtual workspace",
                    "crates/<repo> command-line binary crate",
                ],
                frontend_shorthands: &[],
                examples: &[
                    "jig init ./example-cli --preset rust-cli --no-input --no-vault",
                    "cargo run -p example-cli",
                ],
                ownership: "The generated Cargo and Clippy configuration, Rust source, crate guide, and README are project-owned after creation; jig update keeps only the Jig harness current.",
                non_goals: &[
                    "The rust-cli preset does not create a database, frontend, API, dev app, release workflow, library target, or additional crate layers.",
                    "The scaffold does not select a license, enable package publication, or choose an argument parser or logging framework.",
                ],
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_capabilities_are_exhaustive_and_preserve_the_public_family() {
        assert_eq!(
            ScaffoldPreset::value_variants(),
            &[
                ScaffoldPreset::RustReact,
                ScaffoldPreset::GoReact,
                ScaffoldPreset::HarnessOnly,
                ScaffoldPreset::RustLibrary,
                ScaffoldPreset::RustCli,
            ]
        );

        let expected = [
            (
                ScaffoldPreset::RustReact,
                true,
                (true, true),
                (true, true),
                (false, false),
                Some("Rust React"),
            ),
            (
                ScaffoldPreset::GoReact,
                true,
                (true, true),
                (true, true),
                (true, true),
                Some("Go React"),
            ),
            (
                ScaffoldPreset::HarnessOnly,
                false,
                (false, false),
                (false, false),
                (false, false),
                None,
            ),
            (
                ScaffoldPreset::RustLibrary,
                true,
                (false, false),
                (false, false),
                (false, false),
                Some("Rust library"),
            ),
            (
                ScaffoldPreset::RustCli,
                true,
                (false, false),
                (false, false),
                (false, false),
                Some("Rust CLI"),
            ),
        ];
        for (preset, project, database, frontends, go_module, label) in expected {
            assert_eq!(preset.has_project_scaffold(), project, "{preset:?}");
            assert_eq!(preset.project_scaffold_label(), label, "{preset:?}");
            assert_eq!(preset.supports_database(), database.0, "{preset:?}");
            assert_eq!(preset.requires_database_choice(), database.1, "{preset:?}");
            assert_eq!(preset.supports_frontends(), frontends.0, "{preset:?}");
            assert_eq!(preset.requires_frontend_choice(), frontends.1, "{preset:?}");
            assert_eq!(preset.supports_go_module(), go_module.0, "{preset:?}");
            assert_eq!(preset.requires_go_module(), go_module.1, "{preset:?}");
            assert_eq!(
                preset.requires_web_package_manager(),
                frontends.0,
                "{preset:?}"
            );
        }
        assert_eq!(ScaffoldPreset::RustCli.as_str(), "rust-cli");
        assert_eq!(
            ScaffoldPreset::RustCli.generated_backend_language(),
            Some(BackendLanguage::Rust)
        );
        assert!(
            ScaffoldPreset::RustCli
                .reserved_backend_dev_app_names()
                .is_empty()
        );
        assert!(ScaffoldPreset::RustCli.reserved_backend_roots().is_empty());
    }
}
