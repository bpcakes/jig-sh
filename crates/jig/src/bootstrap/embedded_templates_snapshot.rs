// Generated from templates/project. Update with JIG_REFRESH_EMBEDDED_TEMPLATE_SNAPSHOT=1 cargo check -p jig-sh.
#[cfg(test)]
#[allow(dead_code)]
pub(super) const EMBEDDED_TEMPLATE_FILES_FROM_SNAPSHOT: bool = true;
pub(super) static EMBEDDED_TEMPLATE_FILES: &[EmbeddedTemplateFile] = &[
    EmbeddedTemplateFile {
        relative_path: ".agent/.cache/.gitignore.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/embedded_template_snapshots/.agent/.cache/.gitignore.jinja")),
    },
    EmbeddedTemplateFile {
        relative_path: ".agent/PLANS.md.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/embedded_template_snapshots/.agent/PLANS.md.jinja")),
    },
    EmbeddedTemplateFile {
        relative_path: ".agent/jig-contract.json.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/embedded_template_snapshots/.agent/jig-contract.json.jinja")),
    },
    EmbeddedTemplateFile {
        relative_path: ".agent/plans/.gitkeep.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/embedded_template_snapshots/.agent/plans/.gitkeep.jinja")),
    },
    EmbeddedTemplateFile {
        relative_path: ".agent/state/.gitkeep.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/embedded_template_snapshots/.agent/state/.gitkeep.jinja")),
    },
    EmbeddedTemplateFile {
        relative_path: ".gitattributes.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/embedded_template_snapshots/.gitattributes.jinja")),
    },
    EmbeddedTemplateFile {
        relative_path: ".github/workflows/agent-map-check.yml.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/embedded_template_snapshots/.github/workflows/agent-map-check.yml.jinja")),
    },
    EmbeddedTemplateFile {
        relative_path: ".github/workflows/go-tests.yml.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/embedded_template_snapshots/.github/workflows/go-tests.yml.jinja")),
    },
    EmbeddedTemplateFile {
        relative_path: ".github/workflows/repo-policy.yml.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/embedded_template_snapshots/.github/workflows/repo-policy.yml.jinja")),
    },
    EmbeddedTemplateFile {
        relative_path: ".github/workflows/rust-tests.yml.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/embedded_template_snapshots/.github/workflows/rust-tests.yml.jinja")),
    },
    EmbeddedTemplateFile {
        relative_path: ".github/workflows/webapp-checks.yml.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/embedded_template_snapshots/.github/workflows/webapp-checks.yml.jinja")),
    },
    EmbeddedTemplateFile {
        relative_path: ".gitignore.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/embedded_template_snapshots/.gitignore.jinja")),
    },
    EmbeddedTemplateFile {
        relative_path: ".jig.toml.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/embedded_template_snapshots/.jig.toml.jinja")),
    },
    EmbeddedTemplateFile {
        relative_path: ".mcp.json.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/embedded_template_snapshots/.mcp.json.jinja")),
    },
    EmbeddedTemplateFile {
        relative_path: "AGENTS.md.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/embedded_template_snapshots/AGENTS.md.jinja")),
    },
    EmbeddedTemplateFile {
        relative_path: "agent-map.md.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/embedded_template_snapshots/agent-map.md.jinja")),
    },
    EmbeddedTemplateFile {
        relative_path: "scripts/check-webapp-scripts.mjs.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/embedded_template_snapshots/scripts/check-webapp-scripts.mjs.jinja")),
    },
    EmbeddedTemplateFile {
        relative_path: "scripts/check-webapps.sh.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/embedded_template_snapshots/scripts/check-webapps.sh.jinja")),
    },
    EmbeddedTemplateFile {
        relative_path: "scripts/enforce-coverage.cjs.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/embedded_template_snapshots/scripts/enforce-coverage.cjs.jinja")),
    },
    EmbeddedTemplateFile {
        relative_path: "scripts/install-jig.sh.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/embedded_template_snapshots/scripts/install-jig.sh.jinja")),
    },
    EmbeddedTemplateFile {
        relative_path: "scripts/jig.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/embedded_template_snapshots/scripts/jig.jinja")),
    },
    EmbeddedTemplateFile {
        relative_path: "scripts/new-checkout.sh.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/embedded_template_snapshots/scripts/new-checkout.sh.jinja")),
    },
    EmbeddedTemplateFile {
        relative_path: "scripts/web-node.cjs.jinja",
        contents: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bootstrap/embedded_template_snapshots/scripts/web-node.cjs.jinja")),
    },
];
