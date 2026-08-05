mod agent_guides;
mod bootstrap;
mod cancellation;
mod cli;
mod codex;
mod command;
mod context;
#[cfg(feature = "dev-proxy")]
mod dev_proxy;
mod doctor;
mod frontend_metadata;
#[cfg(not(feature = "dev-proxy"))]
mod dev_proxy {
    // Keep the CLI surface parseable in `--no-default-features` binaries while
    // returning a direct runtime error for commands that require proxy support.
    pub(crate) mod commands {
        use anyhow::{Result, bail};
        use serde_json::Value;

        use crate::command::{DevCommand, ProxyCommand};
        use crate::context::RepoContext;

        pub(crate) fn dev(_ctx: &RepoContext, _command: DevCommand) -> Result<Value> {
            bail!(
                "`jig dev` is unavailable because this binary was built without the `dev-proxy` feature"
            )
        }

        pub(crate) fn dev_without_context(_command: DevCommand) -> Result<Value> {
            bail!(
                "`jig dev` is unavailable because this binary was built without the `dev-proxy` feature"
            )
        }

        pub(crate) fn proxy(_ctx: &RepoContext, _command: ProxyCommand) -> Result<Value> {
            bail!(
                "`jig proxy` is unavailable because this binary was built without the `dev-proxy` feature"
            )
        }

        pub(crate) fn proxy_without_context(_command: ProxyCommand) -> Result<Value> {
            bail!(
                "`jig proxy` is unavailable because this binary was built without the `dev-proxy` feature"
            )
        }
    }
}
mod git_receipts;
mod info;
mod mcp;
mod policy;
mod progress;
mod prompt_registry;
mod root_commands;
mod runtime;
mod serde_helpers;
mod shell;
mod state;
mod status;
#[cfg(test)]
mod test_env;
#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod test_process;
mod text;
mod tool_defs;
mod ui;

// Shared protocol between generated optional Cargo command defaults and
// `work check`: keep this prefix stable unless both sides change.
// User commands that intentionally print this prefix are treated as those
// generated harness skips in summary output.
pub(crate) const CARGO_SKIP_OUTPUT_PREFIX: &str = "No Cargo.toml found; skipping cargo ";

/// Runs the Jig command-line interface.
///
/// # Errors
///
/// Returns an error when command parsing, repository loading, command
/// execution, structured output, or cleanup fails.
pub fn run() -> anyhow::Result<()> {
    cli::run()
}

pub fn error_is_structured_command_failure(error: &anyhow::Error) -> bool {
    cli::is_structured_json_failure(error)
}

pub fn error_exit_code(error: &anyhow::Error) -> Option<i32> {
    cli::structured_error_exit_code(error)
}

#[cfg(all(test, not(feature = "dev-proxy")))]
mod no_dev_proxy_feature_tests {
    use tempfile::tempdir;

    use crate::test_env::TestRepoBuilder;

    use super::*;

    fn write_minimal_repo(root: &std::path::Path) {
        TestRepoBuilder::new(root).write();
    }

    #[test]
    fn runtime_dispatch_reports_proxy_disabled_without_dev_proxy_feature() {
        let temp = tempdir().unwrap();
        write_minimal_repo(temp.path());
        let ctx = context::RepoContext::load_from(temp.path()).unwrap();

        let error = runtime::dispatch(
            &ctx,
            command::RuntimeCommand::Proxy(
                cli::ProxyCommand::List(cli::ProxyListOpts::default()).into(),
            ),
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("without the `dev-proxy` feature"));
    }

    #[test]
    fn every_dev_action_reports_proxy_disabled_without_repo_lookup() {
        let commands = [
            cli::DevOpts {
                command: None,
                launch: cli::DevLaunchOpts::default(),
            }
            .into(),
            cli::DevOpts {
                command: Some(cli::DevSubcommand::Status(cli::DevStatusOpts::default())),
                launch: cli::DevLaunchOpts::default(),
            }
            .into(),
            cli::DevOpts {
                command: Some(cli::DevSubcommand::Stop(cli::DevStopOpts::default())),
                launch: cli::DevLaunchOpts::default(),
            }
            .into(),
        ];

        for command in commands {
            let error = dev_proxy::commands::dev_without_context(command)
                .unwrap_err()
                .to_string();
            assert!(error.contains("without the `dev-proxy` feature"));
        }
    }
}
