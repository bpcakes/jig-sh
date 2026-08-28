// Generated from templates/scaffolds. Update with JIG_REFRESH_EMBEDDED_TEMPLATE_SNAPSHOT=1 cargo check -p jig-sh.
#[cfg(test)]
#[allow(dead_code)]
pub(super) const EMBEDDED_SCAFFOLD_TEMPLATE_FILES_FROM_SNAPSHOT: bool = true;
pub(super) static EMBEDDED_SCAFFOLD_TEMPLATE_FILES: &[EmbeddedScaffoldTemplateFile] = &[
    EmbeddedScaffoldTemplateFile {
        relative_path: "go-react/frontend/api-client-public/src/generated/@tanstack/react-query.gen.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/go-react/frontend/api-client-public/src/generated/@tanstack/react-query.gen.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "go-react/frontend/api-client-public/src/generated/index.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/go-react/frontend/api-client-public/src/generated/index.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "go-react/frontend/api-client-public/src/generated/sdk.gen.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/go-react/frontend/api-client-public/src/generated/sdk.gen.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "go-react/frontend/api-client-public/src/generated/types.gen.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/go-react/frontend/api-client-public/src/generated/types.gen.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "go-react/frontend/api-client-public/src/generated/zod.gen.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/go-react/frontend/api-client-public/src/generated/zod.gen.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "go-react/workspace/.env.example.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/go-react/workspace/.env.example.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "go-react/workspace/cmd/api/database_command.go.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/go-react/workspace/cmd/api/database_command.go.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "go-react/workspace/cmd/api/main.go.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/go-react/workspace/cmd/api/main.go.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "go-react/workspace/cmd/api/main_test.go.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/go-react/workspace/cmd/api/main_test.go.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "go-react/workspace/cmd/openapi/main.go.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/go-react/workspace/cmd/openapi/main.go.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "go-react/workspace/go.mod.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/go-react/workspace/go.mod.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "go-react/workspace/internal/config/config.go.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/go-react/workspace/internal/config/config.go.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "go-react/workspace/internal/config/config_test.go.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/go-react/workspace/internal/config/config_test.go.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "go-react/workspace/internal/database/database.go.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/go-react/workspace/internal/database/database.go.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "go-react/workspace/internal/database/database_test.go.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/go-react/workspace/internal/database/database_test.go.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "go-react/workspace/internal/database/migrations/00001_app_metadata.sql.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/go-react/workspace/internal/database/migrations/00001_app_metadata.sql.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "go-react/workspace/internal/database/queries/metadata.sql.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/go-react/workspace/internal/database/queries/metadata.sql.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "go-react/workspace/internal/database/sqlc/db.go.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/go-react/workspace/internal/database/sqlc/db.go.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "go-react/workspace/internal/database/sqlc/metadata.sql.go.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/go-react/workspace/internal/database/sqlc/metadata.sql.go.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "go-react/workspace/internal/database/sqlc/models.go.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/go-react/workspace/internal/database/sqlc/models.go.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "go-react/workspace/internal/httpapi/httpapi.go.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/go-react/workspace/internal/httpapi/httpapi.go.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "go-react/workspace/internal/httpapi/httpapi_test.go.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/go-react/workspace/internal/httpapi/httpapi_test.go.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "go-react/workspace/openapi/public.json.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/go-react/workspace/openapi/public.json.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "go-react/workspace/scripts/test-postgres.sh.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/go-react/workspace/scripts/test-postgres.sh.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "go-react/workspace/sqlc.yaml.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/go-react/workspace/sqlc.yaml.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/admin-shadcn/.prettierignore.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/admin-shadcn/.prettierignore.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/admin-shadcn/.prettierrc.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/admin-shadcn/.prettierrc.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/admin-shadcn/README.md.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/admin-shadcn/README.md.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/admin-shadcn/components.json.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/admin-shadcn/components.json.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/admin-shadcn/eslint.config.js.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/admin-shadcn/eslint.config.js.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/admin-shadcn/index.html.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/admin-shadcn/index.html.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/admin-shadcn/package.json.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/admin-shadcn/package.json.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/admin-shadcn/src/app/providers.tsx.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/admin-shadcn/src/app/providers.tsx.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/admin-shadcn/src/app/router-context.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/admin-shadcn/src/app/router-context.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/admin-shadcn/src/app/router.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/admin-shadcn/src/app/router.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/admin-shadcn/src/app/shell.tsx.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/admin-shadcn/src/app/shell.tsx.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/admin-shadcn/src/components/app-sidebar.tsx.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/admin-shadcn/src/components/app-sidebar.tsx.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/admin-shadcn/src/components/mode-toggle.tsx.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/admin-shadcn/src/components/mode-toggle.tsx.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/admin-shadcn/src/components/theme-provider.tsx.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/admin-shadcn/src/components/theme-provider.tsx.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/admin-shadcn/src/components/ui/alert.tsx.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/admin-shadcn/src/components/ui/alert.tsx.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/admin-shadcn/src/components/ui/badge.tsx.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/admin-shadcn/src/components/ui/badge.tsx.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/admin-shadcn/src/components/ui/breadcrumb.tsx.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/admin-shadcn/src/components/ui/breadcrumb.tsx.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/admin-shadcn/src/components/ui/button.tsx.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/admin-shadcn/src/components/ui/button.tsx.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/admin-shadcn/src/components/ui/card.tsx.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/admin-shadcn/src/components/ui/card.tsx.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/admin-shadcn/src/components/ui/dropdown-menu.tsx.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/admin-shadcn/src/components/ui/dropdown-menu.tsx.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/admin-shadcn/src/components/ui/empty.tsx.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/admin-shadcn/src/components/ui/empty.tsx.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/admin-shadcn/src/components/ui/input.tsx.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/admin-shadcn/src/components/ui/input.tsx.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/admin-shadcn/src/components/ui/separator.tsx.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/admin-shadcn/src/components/ui/separator.tsx.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/admin-shadcn/src/components/ui/sheet.tsx.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/admin-shadcn/src/components/ui/sheet.tsx.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/admin-shadcn/src/components/ui/sidebar.tsx.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/admin-shadcn/src/components/ui/sidebar.tsx.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/admin-shadcn/src/components/ui/skeleton.tsx.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/admin-shadcn/src/components/ui/skeleton.tsx.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/admin-shadcn/src/components/ui/sonner.tsx.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/admin-shadcn/src/components/ui/sonner.tsx.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/admin-shadcn/src/components/ui/table.tsx.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/admin-shadcn/src/components/ui/table.tsx.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/admin-shadcn/src/components/ui/tooltip.tsx.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/admin-shadcn/src/components/ui/tooltip.tsx.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/admin-shadcn/src/features/overview/overview-page.test.tsx.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/admin-shadcn/src/features/overview/overview-page.test.tsx.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/admin-shadcn/src/features/overview/overview-page.tsx.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/admin-shadcn/src/features/overview/overview-page.tsx.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/admin-shadcn/src/features/settings/settings-page.tsx.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/admin-shadcn/src/features/settings/settings-page.tsx.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/admin-shadcn/src/hooks/use-mobile.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/admin-shadcn/src/hooks/use-mobile.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/admin-shadcn/src/index.css.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/admin-shadcn/src/index.css.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/admin-shadcn/src/lib/api.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/admin-shadcn/src/lib/api.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/admin-shadcn/src/lib/query-client.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/admin-shadcn/src/lib/query-client.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/admin-shadcn/src/lib/utils.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/admin-shadcn/src/lib/utils.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/admin-shadcn/src/main.tsx.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/admin-shadcn/src/main.tsx.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/admin-shadcn/src/routeTree.gen.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/admin-shadcn/src/routeTree.gen.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/admin-shadcn/src/routes/__root.tsx.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/admin-shadcn/src/routes/__root.tsx.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/admin-shadcn/src/routes/index.tsx.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/admin-shadcn/src/routes/index.tsx.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/admin-shadcn/src/routes/settings.tsx.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/admin-shadcn/src/routes/settings.tsx.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/admin-shadcn/src/test-setup.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/admin-shadcn/src/test-setup.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/admin-shadcn/tsconfig.app.json.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/admin-shadcn/tsconfig.app.json.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/admin-shadcn/tsconfig.json.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/admin-shadcn/tsconfig.json.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/admin-shadcn/tsconfig.node.json.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/admin-shadcn/tsconfig.node.json.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/admin-shadcn/vite.config.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/admin-shadcn/vite.config.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/api-client-admin/package.json.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/api-client-admin/package.json.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/api-client-admin/src/generated/@tanstack/react-query.gen.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/api-client-admin/src/generated/@tanstack/react-query.gen.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/api-client-admin/src/generated/client.gen.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/api-client-admin/src/generated/client.gen.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/api-client-admin/src/generated/client/client.gen.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/api-client-admin/src/generated/client/client.gen.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/api-client-admin/src/generated/client/index.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/api-client-admin/src/generated/client/index.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/api-client-admin/src/generated/client/types.gen.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/api-client-admin/src/generated/client/types.gen.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/api-client-admin/src/generated/client/utils.gen.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/api-client-admin/src/generated/client/utils.gen.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/api-client-admin/src/generated/core/auth.gen.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/api-client-admin/src/generated/core/auth.gen.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/api-client-admin/src/generated/core/bodySerializer.gen.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/api-client-admin/src/generated/core/bodySerializer.gen.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/api-client-admin/src/generated/core/params.gen.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/api-client-admin/src/generated/core/params.gen.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/api-client-admin/src/generated/core/pathSerializer.gen.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/api-client-admin/src/generated/core/pathSerializer.gen.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/api-client-admin/src/generated/core/queryKeySerializer.gen.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/api-client-admin/src/generated/core/queryKeySerializer.gen.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/api-client-admin/src/generated/core/serverSentEvents.gen.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/api-client-admin/src/generated/core/serverSentEvents.gen.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/api-client-admin/src/generated/core/types.gen.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/api-client-admin/src/generated/core/types.gen.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/api-client-admin/src/generated/core/utils.gen.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/api-client-admin/src/generated/core/utils.gen.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/api-client-admin/src/generated/index.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/api-client-admin/src/generated/index.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/api-client-admin/src/generated/sdk.gen.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/api-client-admin/src/generated/sdk.gen.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/api-client-admin/src/generated/types.gen.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/api-client-admin/src/generated/types.gen.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/api-client-admin/src/generated/zod.gen.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/api-client-admin/src/generated/zod.gen.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/api-client-admin/src/index.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/api-client-admin/src/index.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/api-client-public/package.json.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/api-client-public/package.json.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/api-client-public/src/generated/@tanstack/react-query.gen.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/api-client-public/src/generated/@tanstack/react-query.gen.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/api-client-public/src/generated/client.gen.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/api-client-public/src/generated/client.gen.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/api-client-public/src/generated/client/client.gen.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/api-client-public/src/generated/client/client.gen.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/api-client-public/src/generated/client/index.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/api-client-public/src/generated/client/index.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/api-client-public/src/generated/client/types.gen.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/api-client-public/src/generated/client/types.gen.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/api-client-public/src/generated/client/utils.gen.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/api-client-public/src/generated/client/utils.gen.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/api-client-public/src/generated/core/auth.gen.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/api-client-public/src/generated/core/auth.gen.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/api-client-public/src/generated/core/bodySerializer.gen.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/api-client-public/src/generated/core/bodySerializer.gen.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/api-client-public/src/generated/core/params.gen.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/api-client-public/src/generated/core/params.gen.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/api-client-public/src/generated/core/pathSerializer.gen.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/api-client-public/src/generated/core/pathSerializer.gen.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/api-client-public/src/generated/core/queryKeySerializer.gen.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/api-client-public/src/generated/core/queryKeySerializer.gen.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/api-client-public/src/generated/core/serverSentEvents.gen.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/api-client-public/src/generated/core/serverSentEvents.gen.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/api-client-public/src/generated/core/types.gen.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/api-client-public/src/generated/core/types.gen.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/api-client-public/src/generated/core/utils.gen.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/api-client-public/src/generated/core/utils.gen.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/api-client-public/src/generated/index.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/api-client-public/src/generated/index.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/api-client-public/src/generated/sdk.gen.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/api-client-public/src/generated/sdk.gen.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/api-client-public/src/generated/types.gen.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/api-client-public/src/generated/types.gen.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/api-client-public/src/generated/zod.gen.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/api-client-public/src/generated/zod.gen.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/api-client-public/src/index.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/api-client-public/src/index.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/astro/astro.config.mjs.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/astro/astro.config.mjs.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/astro/package.json.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/astro/package.json.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/astro/src/pages/index.astro.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/astro/src/pages/index.astro.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/astro/tsconfig.json.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/astro/tsconfig.json.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/vite-react/.gitignore.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/vite-react/.gitignore.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/vite-react/README.md.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/vite-react/README.md.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/vite-react/e2e/app.spec.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/vite-react/e2e/app.spec.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/vite-react/eslint.config.js.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/vite-react/eslint.config.js.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/vite-react/index.html.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/vite-react/index.html.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/vite-react/package.json.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/vite-react/package.json.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/vite-react/playwright.config.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/vite-react/playwright.config.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/vite-react/src/App.test.tsx.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/vite-react/src/App.test.tsx.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/vite-react/src/App.tsx.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/vite-react/src/App.tsx.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/vite-react/src/api.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/vite-react/src/api.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/vite-react/src/app/providers.tsx.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/vite-react/src/app/providers.tsx.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/vite-react/src/app/router-context.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/vite-react/src/app/router-context.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/vite-react/src/app/router.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/vite-react/src/app/router.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/vite-react/src/lib/query-client.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/vite-react/src/lib/query-client.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/vite-react/src/main.tsx.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/vite-react/src/main.tsx.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/vite-react/src/routeTree.gen.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/vite-react/src/routeTree.gen.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/vite-react/src/routes/__root.tsx.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/vite-react/src/routes/__root.tsx.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/vite-react/src/routes/index.tsx.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/vite-react/src/routes/index.tsx.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/vite-react/src/test-setup.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/vite-react/src/test-setup.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/vite-react/tsconfig.app.json.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/vite-react/tsconfig.app.json.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/vite-react/tsconfig.json.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/vite-react/tsconfig.json.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/vite-react/tsconfig.node.json.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/vite-react/tsconfig.node.json.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/vite-react/vite.config.ts.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/vite-react/vite.config.ts.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/workspace/.node-version.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/workspace/.node-version.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/workspace/.yarnrc.yml.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/workspace/.yarnrc.yml.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/workspace/README.md.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/workspace/README.md.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/workspace/contracts.mjs.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/workspace/contracts.mjs.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/workspace/e2e.yml.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/workspace/e2e.yml.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/workspace/eslint.config.shared.mjs.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/workspace/eslint.config.shared.mjs.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/workspace/package.json.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/workspace/package.json.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/frontend/workspace/pnpm-workspace.yaml.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/frontend/workspace/pnpm-workspace.yaml.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/workspace/.env.example.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/workspace/.env.example.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/workspace/Cargo.toml.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/workspace/Cargo.toml.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/workspace/apps/admin-api/Cargo.toml.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/workspace/apps/admin-api/Cargo.toml.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/workspace/apps/admin-api/src/bin/export-openapi.rs.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/workspace/apps/admin-api/src/bin/export-openapi.rs.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/workspace/apps/admin-api/src/main.rs.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/workspace/apps/admin-api/src/main.rs.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/workspace/apps/api/Cargo.toml.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/workspace/apps/api/Cargo.toml.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/workspace/apps/api/src/bin/export-openapi.rs.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/workspace/apps/api/src/bin/export-openapi.rs.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/workspace/apps/api/src/main.rs.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/workspace/apps/api/src/main.rs.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/workspace/crates/admin-http/AGENTS.md.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/workspace/crates/admin-http/AGENTS.md.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/workspace/crates/admin-http/Cargo.toml.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/workspace/crates/admin-http/Cargo.toml.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/workspace/crates/admin-http/src/lib.rs.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/workspace/crates/admin-http/src/lib.rs.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/workspace/crates/app/AGENTS.md.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/workspace/crates/app/AGENTS.md.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/workspace/crates/app/Cargo.toml.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/workspace/crates/app/Cargo.toml.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/workspace/crates/app/src/lib.rs.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/workspace/crates/app/src/lib.rs.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/workspace/crates/core/Cargo.toml.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/workspace/crates/core/Cargo.toml.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/workspace/crates/core/src/lib.rs.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/workspace/crates/core/src/lib.rs.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/workspace/crates/db/AGENTS.md.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/workspace/crates/db/AGENTS.md.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/workspace/crates/db/Cargo.toml.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/workspace/crates/db/Cargo.toml.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/workspace/crates/db/src/lib.rs.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/workspace/crates/db/src/lib.rs.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/workspace/crates/http-common/AGENTS.md.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/workspace/crates/http-common/AGENTS.md.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/workspace/crates/http-common/Cargo.toml.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/workspace/crates/http-common/Cargo.toml.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/workspace/crates/http-common/src/lib.rs.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/workspace/crates/http-common/src/lib.rs.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/workspace/crates/http/AGENTS.md.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/workspace/crates/http/AGENTS.md.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/workspace/crates/http/Cargo.toml.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/workspace/crates/http/Cargo.toml.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/workspace/crates/http/src/lib.rs.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/workspace/crates/http/src/lib.rs.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/workspace/crates/http/src/public.rs.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/workspace/crates/http/src/public.rs.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/workspace/crates/test-support/AGENTS.md.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/workspace/crates/test-support/AGENTS.md.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/workspace/crates/test-support/Cargo.toml.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/workspace/crates/test-support/Cargo.toml.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/workspace/crates/test-support/src/app.rs.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/workspace/crates/test-support/src/app.rs.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/workspace/crates/test-support/src/db.rs.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/workspace/crates/test-support/src/db.rs.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/workspace/crates/test-support/src/http.rs.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/workspace/crates/test-support/src/http.rs.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/workspace/crates/test-support/src/lib.rs.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/workspace/crates/test-support/src/lib.rs.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/workspace/crates/test-support/src/responses.rs.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/workspace/crates/test-support/src/responses.rs.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/workspace/crates/test-support/tests/http.rs.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/workspace/crates/test-support/tests/http.rs.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/workspace/crates/test-support/tests/postgres.rs.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/workspace/crates/test-support/tests/postgres.rs.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/workspace/openapi/admin.json.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/workspace/openapi/admin.json.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/workspace/openapi/public.json.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/workspace/openapi/public.json.jinja")),
    },
    EmbeddedScaffoldTemplateFile {
        relative_path: "rust-react/workspace/scripts/test-postgres.sh.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/scaffold/embedded_template_snapshots/rust-react/workspace/scripts/test-postgres.sh.jinja")),
    },
];
