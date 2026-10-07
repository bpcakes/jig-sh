//! Keeps the generated launcher's command-list markers and `case` arms in
//! step with the root command registry in `jig-commands`.

use jig_commands::root_commands::LauncherScope::{CapabilityOnly, Repository};
use jig_commands::root_commands::{CHECK, launcher_subcommands};

const LAUNCHER_REFRESH_ENV: &str = "JIG_REFRESH_LAUNCHER_COMMAND_LISTS";
const LAUNCHER_REFRESH_COMMAND: &str = "JIG_REFRESH_LAUNCHER_COMMAND_LISTS=1 cargo test -p jig-sh --lib generated_launcher_command_lists";
/// Workspace-relative launcher copies; the embedded snapshot is the one
/// packaged with the crate.
const LAUNCHER_COPIES: &[&str] = &[
    "templates/project/scripts/jig.jinja",
    "crates/jig/src/bootstrap/embedded_template_snapshots/scripts/jig.jinja",
    "scripts/jig",
];

/// Rewrites the launcher's command-list marker comments and the `case`
/// arms that follow their `-begin` markers from the registry.
fn with_registry_command_lists(launcher: &str) -> String {
    let lists = [
        (
            "jig-capability-only-subcommands",
            launcher_subcommands(CapabilityOnly),
        ),
        (
            "jig-repository-scope-subcommands",
            launcher_subcommands(Repository),
        ),
    ];
    let mut rendered = Vec::new();
    let mut pending_arm: Option<Vec<&str>> = None;
    let mut replaced = 0;
    for line in launcher.lines() {
        if let Some(names) = pending_arm.take() {
            let indent = &line[..line.len() - line.trim_start().len()];
            rendered.push(format!("{indent}{})", names.join(" | ")));
            replaced += 1;
            continue;
        }
        let mut marker_line = None;
        for (marker, names) in &lists {
            if line.starts_with(&format!("# {marker}:")) {
                marker_line = Some(format!("# {marker}:{}", names.join(",")));
                replaced += 1;
            } else if line.trim_start() == format!("# {marker}-begin") {
                // `check` keeps its own arm because `check contract` alone
                // is capability-only.
                pending_arm = Some(
                    names
                        .iter()
                        .copied()
                        .filter(|name| *name != CHECK.name)
                        .collect(),
                );
            }
        }
        rendered.push(marker_line.unwrap_or_else(|| line.to_owned()));
    }
    assert_eq!(
        replaced, 4,
        "the launcher must declare both command-list markers and both `case` arms"
    );
    let mut output = rendered.join("\n");
    if launcher.ends_with('\n') {
        output.push('\n');
    }
    output
}

#[test]
fn generated_launcher_command_lists_match_the_registry() {
    if std::env::var_os(LAUNCHER_REFRESH_ENV).is_some() {
        let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        for relative in LAUNCHER_COPIES {
            let path = workspace.join(relative);
            let launcher = std::fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
            std::fs::write(&path, with_registry_command_lists(&launcher))
                .unwrap_or_else(|error| panic!("failed to write {}: {error}", path.display()));
        }
        return;
    }

    let launcher = include_str!("bootstrap/embedded_template_snapshots/scripts/jig.jinja");
    assert!(
        launcher == with_registry_command_lists(launcher),
        "the generated launcher's command lists drifted from the root command registry; \
         refresh every launcher copy with `{LAUNCHER_REFRESH_COMMAND}`"
    );
}
