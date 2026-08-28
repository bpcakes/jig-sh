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
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::RustReact => "rust-react",
            Self::GoReact => "go-react",
            Self::HarnessOnly => "harness-only",
        }
    }

    pub(crate) const fn generated_backend_language(self) -> Option<BackendLanguage> {
        match self {
            Self::RustReact => Some(BackendLanguage::Rust),
            Self::GoReact => Some(BackendLanguage::Go),
            Self::HarnessOnly => None,
        }
    }

    pub(crate) const fn reserved_backend_dev_app_names(self) -> &'static [&'static str] {
        match self {
            Self::RustReact => &[
                APPLICATION_BACKEND_DEV_APP_NAME,
                RUST_REACT_ADMIN_BACKEND_DEV_APP_NAME,
            ],
            Self::GoReact => &[APPLICATION_BACKEND_DEV_APP_NAME],
            Self::HarnessOnly => &[],
        }
    }

    pub(crate) const fn reserved_backend_roots(self) -> &'static [&'static str] {
        match self {
            Self::RustReact => &["apps", "crates"],
            Self::GoReact => &["cmd", "internal"],
            Self::HarnessOnly => &[],
        }
    }

    pub(crate) const fn descriptor(self) -> ScaffoldPresetDescriptor {
        match self {
            Self::RustReact => ScaffoldPresetDescriptor {
                name: "rust-react",
                summary: "Rust API workspace plus shadcn React product/admin apps and an optional Astro site.",
                defaults: &[
                    "Rust crate roots default to apps and crates.",
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
        }
    }
}
