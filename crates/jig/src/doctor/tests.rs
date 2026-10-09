use super::*;

const CURRENT_GENERATED_LAUNCHER_TEMPLATE: &str =
    include_str!("../../../jig-bootstrap/src/embedded_template_snapshots/scripts/jig.jinja");

const CURRENT_GENERATED_INSTALLER: &str = include_str!(
    "../../../jig-bootstrap/src/embedded_template_snapshots/scripts/install-jig.sh.jinja"
);

fn current_generated_launcher() -> String {
    CURRENT_GENERATED_LAUNCHER_TEMPLATE.replace(
        "<<[ _jig.contract_version ]>>",
        &jig_context::CURRENT_CONTRACT_VERSION.to_string(),
    )
}

mod agent;
mod argv;
mod cargo_sqlx;
mod context_checks;
mod database_url;
mod operator_setup;
mod probe_signal_session;
mod programs;
mod proxy;
mod required_programs;
mod required_tools;
mod required_tools_redaction;
mod required_tools_sqlx_probe;
mod root;
mod run_history;
mod runtime;
mod shell_analysis;
mod sqlx_driver;
mod sqlx_driver_probe;
#[cfg(unix)]
mod sqlx_versions;
mod support;
mod toolchains;
mod vault;
mod version_authority;
