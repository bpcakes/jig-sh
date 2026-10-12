//! Git primitives shared across Jig: which program runs as `git`, which
//! inherited `GIT_*` variables a Jig-owned Git command may keep, and
//! byte-preserving reads of Git's on-disk metadata.

use std::env;
use std::process::Command;

pub mod metadata;

/// Overrides the program Jig runs as `git`.
pub const GIT_BIN_ENV: &str = "JIG_GIT_BIN";

/// The program Jig runs as `git`: `JIG_GIT_BIN` when set, otherwise `git`.
pub fn git_program() -> String {
    env::var(GIT_BIN_ENV).unwrap_or_else(|_| "git".to_string())
}

pub fn scrub_known_repository_git_environment(command: &mut Command) {
    // Keep the user's ordinary environment, read-only config sources, and the
    // authentication knobs needed by remote template fetches. Repository
    // discovery/redirection, alternate object/index paths, quarantine state,
    // replacement refs, namespaces, and command-scoped config are stripped so
    // a command aimed at a known repository cannot escape to ambient metadata.
    const ALLOWED_GIT_ENVIRONMENT: &[&str] = &[
        "GIT_ASKPASS",
        "GIT_CONFIG_GLOBAL",
        "GIT_CONFIG_NOSYSTEM",
        "GIT_CONFIG_SYSTEM",
        "GIT_SSH",
        "GIT_SSH_COMMAND",
        "GIT_SSH_VARIANT",
        "GIT_TERMINAL_PROMPT",
    ];
    scrub_git_repository_environment_except(command, ALLOWED_GIT_ENVIRONMENT);
}

pub fn scrub_git_repository_environment_except(command: &mut Command, allowed: &[&str]) {
    let explicitly_configured = command
        .get_envs()
        .map(|(name, _)| name.to_os_string())
        .collect::<Vec<_>>();
    for name in env::vars_os()
        .map(|(name, _)| name)
        .chain(explicitly_configured)
    {
        let normalized = name.to_string_lossy().to_ascii_uppercase();
        if normalized.starts_with("GIT_") && !allowed.contains(&normalized.as_str()) {
            command.env_remove(name);
        }
    }
}
