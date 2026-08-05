//! Shared information architecture for visible root CLI commands.

use std::fmt::Write;

use crate::tool_defs::cli_command;

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
            Self::StructuredWork => "Structured work",
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RootCommand {
    pub(crate) name: &'static str,
    pub(crate) category: RootCommandCategory,
    pub(crate) display_order: usize,
}

const fn command(
    name: &'static str,
    category: RootCommandCategory,
    display_order: usize,
) -> RootCommand {
    RootCommand {
        name,
        category,
        display_order,
    }
}

pub(crate) const INIT: RootCommand =
    command(cli_command::INIT, RootCommandCategory::GetStarted, 10);
pub(crate) const PRESETS: RootCommand =
    command(cli_command::PRESETS, RootCommandCategory::GetStarted, 20);
pub(crate) const ADOPT: RootCommand =
    command(cli_command::ADOPT, RootCommandCategory::GetStarted, 30);
pub(crate) const UPDATE: RootCommand =
    command(cli_command::UPDATE, RootCommandCategory::GetStarted, 40);
pub(crate) const BOOTSTRAP: RootCommand =
    command(cli_command::BOOTSTRAP, RootCommandCategory::GetStarted, 50);
pub(crate) const SETUP: RootCommand =
    command(cli_command::SETUP, RootCommandCategory::GetStarted, 55);
pub(crate) const DOCTOR: RootCommand =
    command(cli_command::DOCTOR, RootCommandCategory::GetStarted, 60);
pub(crate) const INFO: RootCommand =
    command(cli_command::INFO, RootCommandCategory::GetStarted, 70);

pub(crate) const DEV: RootCommand = command(cli_command::DEV, RootCommandCategory::Develop, 100);
pub(crate) const CHECK: RootCommand =
    command(cli_command::CHECK, RootCommandCategory::Develop, 110);
pub(crate) const STATUS: RootCommand =
    command(cli_command::STATUS, RootCommandCategory::Develop, 120);
pub(crate) const UI: RootCommand = command(cli_command::UI, RootCommandCategory::Develop, 130);

pub(crate) const WORK: RootCommand =
    command(cli_command::WORK, RootCommandCategory::StructuredWork, 200);
pub(crate) const LOOP: RootCommand =
    command(cli_command::LOOP, RootCommandCategory::StructuredWork, 210);

pub(crate) const MIGRATION_ADD: RootCommand = command(
    cli_command::MIGRATION_ADD,
    RootCommandCategory::ProjectData,
    300,
);
pub(crate) const SCHEMA_DUMP: RootCommand = command(
    cli_command::SCHEMA_DUMP,
    RootCommandCategory::ProjectData,
    310,
);
pub(crate) const VAULT: RootCommand =
    command(cli_command::VAULT, RootCommandCategory::ProjectData, 320);

pub(crate) const PROXY: RootCommand =
    command(cli_command::PROXY, RootCommandCategory::LocalServices, 400);

pub(crate) const PROMPT: RootCommand = command(
    cli_command::PROMPT,
    RootCommandCategory::AgentAutomation,
    500,
);
pub(crate) const AGENT: RootCommand = command(
    cli_command::AGENT,
    RootCommandCategory::AgentAutomation,
    510,
);
pub(crate) const CODEX: RootCommand = command(
    cli_command::CODEX,
    RootCommandCategory::AgentAutomation,
    515,
);
pub(crate) const AGENT_MAP: RootCommand = command(
    cli_command::AGENT_MAP,
    RootCommandCategory::AgentAutomation,
    520,
);
pub(crate) const STATE: RootCommand = command(
    cli_command::STATE,
    RootCommandCategory::AgentAutomation,
    530,
);
pub(crate) const MCP: RootCommand =
    command(cli_command::MCP, RootCommandCategory::AgentAutomation, 540);

pub(crate) const ALL: &[RootCommand] = &[
    INIT,
    PRESETS,
    ADOPT,
    UPDATE,
    BOOTSTRAP,
    SETUP,
    DOCTOR,
    INFO,
    DEV,
    CHECK,
    STATUS,
    UI,
    WORK,
    LOOP,
    MIGRATION_ADD,
    SCHEMA_DUMP,
    VAULT,
    PROXY,
    PROMPT,
    AGENT,
    CODEX,
    AGENT_MAP,
    STATE,
    MCP,
];

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
}
