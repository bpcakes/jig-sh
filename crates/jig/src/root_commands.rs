//! The single registry of top-level CLI commands.
//!
//! A root command declares its name, help placement, and generated-launcher
//! scope once here. Clap attributes, launcher handoff validation, the generated
//! launcher's command lists, and `jig info --commands` read this registry
//! instead of repeating command names.

use std::fmt::Write;

use LauncherScope::{CapabilityOnly, Repository};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RootCommandCategory {
    GetStarted,
    Develop,
    StructuredWork,
    ProjectData,
    LocalServices,
    AgentAutomation,
}

impl RootCommandCategory {
    pub(crate) const ALL: &[Self] = &[
        Self::GetStarted,
        Self::Develop,
        Self::StructuredWork,
        Self::ProjectData,
        Self::LocalServices,
        Self::AgentAutomation,
    ];

    pub(crate) const fn id(self) -> &'static str {
        match self {
            Self::GetStarted => "get_started",
            Self::Develop => "develop",
            Self::StructuredWork => "structured_work",
            Self::ProjectData => "project_data",
            Self::LocalServices => "local_services",
            Self::AgentAutomation => "agent_automation",
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::GetStarted => "Get started",
            Self::Develop => "Develop",
            Self::StructuredWork => "Workflows",
            Self::ProjectData => "Project data",
            Self::LocalServices => "Local services",
            Self::AgentAutomation => "Agent and automation",
        }
    }

    pub(crate) fn from_id(id: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|category| category.id() == id)
    }
}

/// How the generated launcher hands a top-level command to the runtime.
///
/// Every command chooses explicitly; there is no default.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LauncherScope {
    /// Needs only a capable binary: the command accepts a caller-relative
    /// repository target, or none at all.
    CapabilityOnly,
    /// Operates exclusively on the generated launcher's validated root.
    Repository,
}

/// A top-level command as the generated launcher sees it. Hidden commands have
/// no help placement, so this is their whole registry entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LauncherCommand {
    pub(crate) name: &'static str,
    pub(crate) scope: LauncherScope,
}

impl LauncherCommand {
    /// The same command when one of its invocations needs only a capable
    /// binary, such as `check contract` diagnosing a broken repository.
    pub(crate) const fn capability_only(self) -> Self {
        Self {
            name: self.name,
            scope: CapabilityOnly,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RootCommand {
    pub(crate) id: RootCommandId,
    pub(crate) name: &'static str,
    pub(crate) category: RootCommandCategory,
    pub(crate) display_order: usize,
    pub(crate) launcher_scope: LauncherScope,
}

impl RootCommand {
    pub(crate) const fn launcher(self) -> LauncherCommand {
        LauncherCommand {
            name: self.name,
            scope: self.launcher_scope,
        }
    }
}

/// Declares each visible root command once: its constant, its identity, and
/// its place in [`ALL`].
macro_rules! root_commands {
    ($($constant:ident = $id:ident: $name:literal, $category:ident, $order:literal, $scope:ident;)+) => {
        /// Identity of a visible root command, for exhaustive per-command
        /// decisions.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub(crate) enum RootCommandId {
            $($id,)+
        }

        $(
            pub(crate) const $constant: RootCommand = RootCommand {
                id: RootCommandId::$id,
                name: $name,
                category: RootCommandCategory::$category,
                display_order: $order,
                launcher_scope: LauncherScope::$scope,
            };
        )+

        /// Every visible root command, in help order.
        pub(crate) const ALL: &[RootCommand] = &[$($constant,)+];
    };
}

root_commands! {
    INIT = Init: "init", GetStarted, 10, CapabilityOnly;
    PRESETS = Presets: "presets", GetStarted, 20, CapabilityOnly;
    ADOPT = Adopt: "adopt", GetStarted, 30, CapabilityOnly;
    UPDATE = Update: "update", GetStarted, 40, CapabilityOnly;
    BOOTSTRAP = Bootstrap: "bootstrap", GetStarted, 50, Repository;
    SETUP = Setup: "setup", GetStarted, 55, Repository;
    DOCTOR = Doctor: "doctor", GetStarted, 60, CapabilityOnly;
    INFO = Info: "info", GetStarted, 70, Repository;

    DEV = Dev: "dev", Develop, 100, Repository;
    CHECK = Check: "check", Develop, 110, Repository;
    RUN = Run: "run", Develop, 112, Repository;
    FILE_BUDGET = FileBudget: "file-budget", Develop, 115, Repository;
    STATUS = Status: "status", Develop, 120, Repository;
    UI = Ui: "ui", Develop, 130, Repository;

    LOOP = Loop: "loop", StructuredWork, 210, Repository;

    MIGRATION = Migration: "migration", ProjectData, 290, Repository;
    SQLX = Sqlx: "sqlx", ProjectData, 300, Repository;
    VAULT = Vault: "vault", ProjectData, 320, Repository;

    PROXY = Proxy: "proxy", LocalServices, 400, Repository;

    AGENT = Agent: "agent", AgentAutomation, 510, Repository;
    CODEX = Codex: "codex", AgentAutomation, 515, CapabilityOnly;
    CLAUDE = Claude: "claude", AgentAutomation, 517, CapabilityOnly;
    AGENT_MAP = AgentMap: "agent-map", AgentAutomation, 520, Repository;
    STATE = State: "state", AgentAutomation, 530, Repository;
}

const fn hidden(name: &'static str, scope: LauncherScope) -> LauncherCommand {
    LauncherCommand { name, scope }
}

pub(crate) const MIGRATION_ADD: LauncherCommand = hidden("migration-add", Repository);
pub(crate) const SCHEMA_DUMP: LauncherCommand = hidden("schema-dump", Repository);
pub(crate) const GENERATE_SQLX_UNCHECKED_QUERIES_TODO: LauncherCommand =
    hidden("generate-sqlx-unchecked-queries-todo", Repository);

/// Hidden legacy spellings that the generated launcher still classifies.
#[cfg(test)]
pub(crate) const LEGACY: &[LauncherCommand] = &[
    MIGRATION_ADD,
    SCHEMA_DUMP,
    GENERATE_SQLX_UNCHECKED_QUERIES_TODO,
];

/// The private compatibility probe. The generated launcher and installer
/// invoke it themselves, so the launcher's command lists never classify it.
pub(crate) const RUNTIME_COMPATIBLE: LauncherCommand =
    hidden("__runtime-compatible", CapabilityOnly);

/// The names the generated launcher classifies under `scope`, in the sorted
/// order of its marker comments.
#[cfg(test)]
pub(crate) fn launcher_subcommands(scope: LauncherScope) -> Vec<&'static str> {
    let mut names = ALL
        .iter()
        .map(|command| command.launcher())
        .chain(LEGACY.iter().copied())
        .filter(|command| command.scope == scope)
        .map(|command| command.name)
        .collect::<Vec<_>>();
    names.sort_unstable();
    names
}

pub(crate) fn categorized_help() -> String {
    let mut help = String::from("Command groups:\n");
    for category in RootCommandCategory::ALL {
        let names = ALL
            .iter()
            .filter(|command| command.category == *category)
            .map(|command| command.name)
            .collect::<Vec<_>>()
            .join(", ");
        let _ = writeln!(help, "  {}: {names}", category.label());
    }
    help
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn root_command_metadata_has_unique_names_and_orders() {
        let names = ALL
            .iter()
            .map(|command| command.name)
            .collect::<HashSet<_>>();
        let orders = ALL
            .iter()
            .map(|command| command.display_order)
            .collect::<HashSet<_>>();

        assert_eq!(names.len(), ALL.len());
        assert_eq!(orders.len(), ALL.len());
        assert!(
            ALL.windows(2)
                .all(|commands| commands[0].display_order < commands[1].display_order)
        );
    }

    #[test]
    fn categorized_help_lists_every_root_command_once() {
        let help = categorized_help();
        for category in RootCommandCategory::ALL {
            assert!(help.contains(category.label()));
        }
        let listed_names = help
            .lines()
            .skip(1)
            .flat_map(|line| line.split_once(':').expect("category line").1.split(','))
            .map(str::trim)
            .collect::<Vec<_>>();
        let expected_names = ALL.iter().map(|command| command.name).collect::<Vec<_>>();

        assert_eq!(listed_names, expected_names);
    }

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

    #[test]
    fn launcher_scopes_partition_every_classified_command() {
        let capability = launcher_subcommands(CapabilityOnly);
        let repository = launcher_subcommands(Repository);

        assert_eq!(
            capability.len() + repository.len(),
            ALL.len() + LEGACY.len()
        );
        assert!(!capability.contains(&RUNTIME_COMPATIBLE.name));
        assert!(!repository.contains(&RUNTIME_COMPATIBLE.name));
        assert_eq!(CHECK.launcher().scope, Repository);
        assert_eq!(CHECK.launcher().capability_only().scope, CapabilityOnly);
    }
}
