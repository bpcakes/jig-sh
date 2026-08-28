use super::super::embedded_templates::EMBEDDED_SCAFFOLD_TEMPLATE_FILES;
use super::super::templates::ScaffoldTemplateFile;
use super::super::{ScaffoldFrontendKind, ScaffoldPreset};
use super::app::FrontendScaffold;

pub(super) const FRONTEND_WORKSPACE_TEMPLATES: &[ScaffoldTemplateFile] = &[
    ScaffoldTemplateFile {
        template: "rust-react/frontend/workspace/README.md.jinja",
        output: "README.md",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/workspace/package.json.jinja",
        output: "package.json",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/workspace/.node-version.jinja",
        output: ".node-version",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/workspace/contracts.mjs.jinja",
        output: "scripts/contracts.mjs",
    },
];

pub(super) const REACT_ESLINT_TEMPLATE: ScaffoldTemplateFile = ScaffoldTemplateFile {
    template: "rust-react/frontend/workspace/eslint.config.shared.mjs.jinja",
    output: "eslint.config.shared.mjs",
};

pub(super) const PNPM_WORKSPACE_TEMPLATE: ScaffoldTemplateFile = ScaffoldTemplateFile {
    template: "rust-react/frontend/workspace/pnpm-workspace.yaml.jinja",
    output: "pnpm-workspace.yaml",
};

pub(super) const YARN_WORKSPACE_TEMPLATE: ScaffoldTemplateFile = ScaffoldTemplateFile {
    template: "rust-react/frontend/workspace/.yarnrc.yml.jinja",
    output: ".yarnrc.yml",
};

pub(super) const E2E_WORKFLOW_TEMPLATE: ScaffoldTemplateFile = ScaffoldTemplateFile {
    template: "rust-react/frontend/workspace/e2e.yml.jinja",
    output: ".github/workflows/e2e.yml",
};

pub(super) const ADMIN_TEMPLATE_PREFIX: &str = "rust-react/frontend/admin-shadcn/";
pub(super) const PUBLIC_API_CLIENT_SHARED_TEMPLATES: &[ScaffoldTemplateFile] = &[
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-public/package.json.jinja",
        output: "packages/public-api-client/package.json",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-public/src/index.ts.jinja",
        output: "packages/public-api-client/src/index.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-public/src/generated/client.gen.ts.jinja",
        output: "packages/public-api-client/src/generated/client.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-public/src/generated/client/client.gen.ts.jinja",
        output: "packages/public-api-client/src/generated/client/client.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-public/src/generated/client/index.ts.jinja",
        output: "packages/public-api-client/src/generated/client/index.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-public/src/generated/client/types.gen.ts.jinja",
        output: "packages/public-api-client/src/generated/client/types.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-public/src/generated/client/utils.gen.ts.jinja",
        output: "packages/public-api-client/src/generated/client/utils.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-public/src/generated/core/auth.gen.ts.jinja",
        output: "packages/public-api-client/src/generated/core/auth.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-public/src/generated/core/bodySerializer.gen.ts.jinja",
        output: "packages/public-api-client/src/generated/core/bodySerializer.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-public/src/generated/core/params.gen.ts.jinja",
        output: "packages/public-api-client/src/generated/core/params.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-public/src/generated/core/pathSerializer.gen.ts.jinja",
        output: "packages/public-api-client/src/generated/core/pathSerializer.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-public/src/generated/core/queryKeySerializer.gen.ts.jinja",
        output: "packages/public-api-client/src/generated/core/queryKeySerializer.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-public/src/generated/core/serverSentEvents.gen.ts.jinja",
        output: "packages/public-api-client/src/generated/core/serverSentEvents.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-public/src/generated/core/types.gen.ts.jinja",
        output: "packages/public-api-client/src/generated/core/types.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-public/src/generated/core/utils.gen.ts.jinja",
        output: "packages/public-api-client/src/generated/core/utils.gen.ts",
    },
];
pub(super) const RUST_PUBLIC_API_CLIENT_CONTRACT_TEMPLATES: &[ScaffoldTemplateFile] = &[
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-public/src/generated/types.gen.ts.jinja",
        output: "packages/public-api-client/src/generated/types.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-public/src/generated/index.ts.jinja",
        output: "packages/public-api-client/src/generated/index.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-public/src/generated/sdk.gen.ts.jinja",
        output: "packages/public-api-client/src/generated/sdk.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-public/src/generated/zod.gen.ts.jinja",
        output: "packages/public-api-client/src/generated/zod.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-public/src/generated/@tanstack/react-query.gen.ts.jinja",
        output: "packages/public-api-client/src/generated/@tanstack/react-query.gen.ts",
    },
];
pub(super) const GO_PUBLIC_API_CLIENT_CONTRACT_TEMPLATES: &[ScaffoldTemplateFile] = &[
    ScaffoldTemplateFile {
        template: "go-react/frontend/api-client-public/src/generated/types.gen.ts.jinja",
        output: "packages/public-api-client/src/generated/types.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "go-react/frontend/api-client-public/src/generated/index.ts.jinja",
        output: "packages/public-api-client/src/generated/index.ts",
    },
    ScaffoldTemplateFile {
        template: "go-react/frontend/api-client-public/src/generated/sdk.gen.ts.jinja",
        output: "packages/public-api-client/src/generated/sdk.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "go-react/frontend/api-client-public/src/generated/zod.gen.ts.jinja",
        output: "packages/public-api-client/src/generated/zod.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "go-react/frontend/api-client-public/src/generated/@tanstack/react-query.gen.ts.jinja",
        output: "packages/public-api-client/src/generated/@tanstack/react-query.gen.ts",
    },
];
pub(super) const ADMIN_API_CLIENT_TEMPLATES: &[ScaffoldTemplateFile] = &[
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-admin/package.json.jinja",
        output: "packages/admin-api-client/package.json",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-admin/src/index.ts.jinja",
        output: "packages/admin-api-client/src/index.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-admin/src/generated/client.gen.ts.jinja",
        output: "packages/admin-api-client/src/generated/client.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-admin/src/generated/client/client.gen.ts.jinja",
        output: "packages/admin-api-client/src/generated/client/client.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-admin/src/generated/client/index.ts.jinja",
        output: "packages/admin-api-client/src/generated/client/index.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-admin/src/generated/client/types.gen.ts.jinja",
        output: "packages/admin-api-client/src/generated/client/types.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-admin/src/generated/client/utils.gen.ts.jinja",
        output: "packages/admin-api-client/src/generated/client/utils.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-admin/src/generated/core/auth.gen.ts.jinja",
        output: "packages/admin-api-client/src/generated/core/auth.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-admin/src/generated/core/bodySerializer.gen.ts.jinja",
        output: "packages/admin-api-client/src/generated/core/bodySerializer.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-admin/src/generated/core/params.gen.ts.jinja",
        output: "packages/admin-api-client/src/generated/core/params.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-admin/src/generated/core/pathSerializer.gen.ts.jinja",
        output: "packages/admin-api-client/src/generated/core/pathSerializer.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-admin/src/generated/core/queryKeySerializer.gen.ts.jinja",
        output: "packages/admin-api-client/src/generated/core/queryKeySerializer.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-admin/src/generated/core/serverSentEvents.gen.ts.jinja",
        output: "packages/admin-api-client/src/generated/core/serverSentEvents.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-admin/src/generated/core/types.gen.ts.jinja",
        output: "packages/admin-api-client/src/generated/core/types.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-admin/src/generated/core/utils.gen.ts.jinja",
        output: "packages/admin-api-client/src/generated/core/utils.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-admin/src/generated/types.gen.ts.jinja",
        output: "packages/admin-api-client/src/generated/types.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-admin/src/generated/index.ts.jinja",
        output: "packages/admin-api-client/src/generated/index.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-admin/src/generated/sdk.gen.ts.jinja",
        output: "packages/admin-api-client/src/generated/sdk.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-admin/src/generated/zod.gen.ts.jinja",
        output: "packages/admin-api-client/src/generated/zod.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/api-client-admin/src/generated/@tanstack/react-query.gen.ts.jinja",
        output: "packages/admin-api-client/src/generated/@tanstack/react-query.gen.ts",
    },
];
pub(super) const VITE_REACT_TEMPLATES: &[ScaffoldTemplateFile] = &[
    ScaffoldTemplateFile {
        template: "rust-react/frontend/vite-react/.gitignore.jinja",
        output: ".gitignore",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/vite-react/package.json.jinja",
        output: "package.json",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/vite-react/index.html.jinja",
        output: "index.html",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/vite-react/vite.config.ts.jinja",
        output: "vite.config.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/vite-react/playwright.config.ts.jinja",
        output: "playwright.config.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/vite-react/tsconfig.json.jinja",
        output: "tsconfig.json",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/vite-react/tsconfig.app.json.jinja",
        output: "tsconfig.app.json",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/vite-react/tsconfig.node.json.jinja",
        output: "tsconfig.node.json",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/vite-react/eslint.config.js.jinja",
        output: "eslint.config.js",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/vite-react/src/main.tsx.jinja",
        output: "src/main.tsx",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/vite-react/src/app/providers.tsx.jinja",
        output: "src/app/providers.tsx",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/vite-react/src/app/router-context.ts.jinja",
        output: "src/app/router-context.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/vite-react/src/app/router.ts.jinja",
        output: "src/app/router.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/vite-react/src/App.tsx.jinja",
        output: "src/App.tsx",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/vite-react/src/App.test.tsx.jinja",
        output: "src/App.test.tsx",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/vite-react/src/api.ts.jinja",
        output: "src/api.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/vite-react/src/lib/query-client.ts.jinja",
        output: "src/lib/query-client.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/vite-react/src/routes/__root.tsx.jinja",
        output: "src/routes/__root.tsx",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/vite-react/src/routes/index.tsx.jinja",
        output: "src/routes/index.tsx",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/vite-react/src/routeTree.gen.ts.jinja",
        output: "src/routeTree.gen.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/vite-react/README.md.jinja",
        output: "README.md",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/admin-shadcn/src/index.css.jinja",
        output: "src/index.css",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/vite-react/src/test-setup.ts.jinja",
        output: "src/test-setup.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/vite-react/e2e/app.spec.ts.jinja",
        output: "e2e/app.spec.ts",
    },
];

// Keep one canonical copy of registry-generated shadcn source while allowing each
// generated application to own the rendered component files independently.
pub(super) const SPA_SHADCN_TEMPLATES: &[ScaffoldTemplateFile] = &[
    ScaffoldTemplateFile {
        template: "rust-react/frontend/admin-shadcn/components.json.jinja",
        output: "components.json",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/admin-shadcn/src/lib/utils.ts.jinja",
        output: "src/lib/utils.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/admin-shadcn/src/components/ui/alert.tsx.jinja",
        output: "src/components/ui/alert.tsx",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/admin-shadcn/src/components/ui/badge.tsx.jinja",
        output: "src/components/ui/badge.tsx",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/admin-shadcn/src/components/ui/button.tsx.jinja",
        output: "src/components/ui/button.tsx",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/admin-shadcn/src/components/ui/card.tsx.jinja",
        output: "src/components/ui/card.tsx",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/admin-shadcn/src/components/ui/skeleton.tsx.jinja",
        output: "src/components/ui/skeleton.tsx",
    },
];

pub(super) const ASTRO_TEMPLATES: &[ScaffoldTemplateFile] = &[
    ScaffoldTemplateFile {
        template: "rust-react/frontend/astro/package.json.jinja",
        output: "package.json",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/astro/astro.config.mjs.jinja",
        output: "astro.config.mjs",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/astro/tsconfig.json.jinja",
        output: "tsconfig.json",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/astro/src/pages/index.astro.jinja",
        output: "src/pages/index.astro",
    },
];

pub(super) fn admin_template_files() -> Vec<ScaffoldTemplateFile> {
    EMBEDDED_SCAFFOLD_TEMPLATE_FILES
        .iter()
        .filter_map(|file| {
            let output = file
                .relative_path
                .strip_prefix(ADMIN_TEMPLATE_PREFIX)?
                .strip_suffix(".jinja")?;
            Some(ScaffoldTemplateFile {
                template: file.relative_path,
                output,
            })
        })
        .collect()
}

pub(super) fn frontend_workspace_template_files_for_backend(
    preset: ScaffoldPreset,
    package_manager: &str,
    frontends: &[FrontendScaffold],
) -> Vec<ScaffoldTemplateFile> {
    if frontends.is_empty() {
        return Vec::new();
    }
    let has_spa = frontends
        .iter()
        .any(|frontend| frontend.kind == ScaffoldFrontendKind::Spa);
    let has_admin = frontends
        .iter()
        .any(|frontend| frontend.kind == ScaffoldFrontendKind::Admin);
    let has_react = frontends.iter().any(|frontend| {
        matches!(
            frontend.kind,
            ScaffoldFrontendKind::Spa | ScaffoldFrontendKind::Admin
        )
    });
    FRONTEND_WORKSPACE_TEMPLATES
        .iter()
        .copied()
        .chain(has_react.then_some(REACT_ESLINT_TEMPLATE))
        .chain((package_manager == "pnpm").then_some(PNPM_WORKSPACE_TEMPLATE))
        .chain((package_manager == "yarn").then_some(YARN_WORKSPACE_TEMPLATE))
        .chain(has_spa.then_some(E2E_WORKFLOW_TEMPLATE))
        .chain(PUBLIC_API_CLIENT_SHARED_TEMPLATES.iter().copied())
        .chain(
            if preset == ScaffoldPreset::GoReact {
                GO_PUBLIC_API_CLIENT_CONTRACT_TEMPLATES
            } else {
                RUST_PUBLIC_API_CLIENT_CONTRACT_TEMPLATES
            }
            .iter()
            .copied(),
        )
        .chain(
            has_admin
                .then_some(ADMIN_API_CLIENT_TEMPLATES)
                .into_iter()
                .flatten()
                .copied(),
        )
        .collect()
}
