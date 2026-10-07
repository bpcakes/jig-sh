mod agent_guides;
mod agent_launch;
mod bootstrap;
#[cfg(test)]
#[path = "../build_identity.rs"]
mod build_identity;
mod cli;
mod command;
#[cfg(feature = "dev-proxy")]
mod dev_proxy;
mod doctor;
mod exit;
#[cfg(not(feature = "dev-proxy"))]
mod dev_proxy {
    // Keep the CLI surface parseable in `--no-default-features` binaries while
    // returning a direct runtime error for commands that require proxy support.
    pub(crate) mod commands {
        use anyhow::{Result, bail};
        use jig_context::RepoContext;
        use serde_json::Value;

        use crate::command::{DevCommand, ProxyCommand};

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
mod info;
mod policy;
mod progress;
mod repository;
mod root_commands;
mod runtime;
mod runtime_artifacts;
mod runtime_cache_lock;
mod rust_syntax;
mod shell;
mod signal_supervision;
mod source_identity;
mod source_projection;
mod status;
mod surface;
#[cfg(test)]
use jig_context::test_support as test_env;
#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod test_process;
mod tool_defs;
mod ui;

// Shared protocol between generated optional Cargo command defaults and
// `work check`: keep this prefix stable unless both sides change.
// User commands that intentionally print this prefix are treated as those
// generated harness skips in summary output.
pub(crate) const CARGO_SKIP_OUTPUT_PREFIX: &str = "No Cargo.toml found; skipping cargo ";

/// Runs the Jig command-line interface.
///
/// Call this before the process creates any threads. Right after parsing,
/// commands that cannot capture the vault passphrase remove the reserved
/// `JIG_VAULT_PASSPHRASE` and `JIG_VAULT_NEW_PASSPHRASE` variables from the
/// process environment, and launcher handoffs set `JIG_REPO_ROOT`; those
/// environment mutations are sound only while the process is single-threaded.
///
/// # Errors
///
/// Returns an error when command parsing, repository loading, command
/// execution, structured output, or cleanup fails.
pub fn run() -> anyhow::Result<()> {
    cli::run()
}

/// Returns whether the command already reported this failure in its own
/// protocol. Callers must then exit without printing the error.
pub fn error_is_structured_command_failure(error: &anyhow::Error) -> bool {
    exit::CliExit::of(error).is_some_and(exit::CliExit::is_reported)
}

/// Returns the process exit status this failure decided, if it decided one.
pub fn error_exit_code(error: &anyhow::Error) -> Option<i32> {
    exit::CliExit::of(error).map(exit::CliExit::code)
}

/// Returns whether human stderr delivery was abandoned after its shutdown
/// deadline. Callers must not perform another blocking stderr write afterward.
pub fn stderr_delivery_abandoned() -> bool {
    progress::stderr_delivery_abandoned()
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
        let ctx = jig_context::RepoContext::load_from(temp.path()).unwrap();

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

#[cfg(test)]
mod build_identity_tests {
    use tempfile::tempdir;

    fn build_configuration() -> Vec<(String, String)> {
        vec![
            ("TARGET".into(), "x86_64-unknown-linux-gnu".into()),
            ("HOST".into(), "x86_64-unknown-linux-gnu".into()),
            ("PROFILE".into(), "debug".into()),
            ("CARGO_PKG_VERSION".into(), "0.3.0".into()),
            (
                "JIG_BUILD_OFFICIAL_TEMPLATE_PIN".into(),
                "unreleased".into(),
            ),
        ]
    }

    #[test]
    fn development_display_version_adds_git_provenance() {
        assert_eq!(
            crate::build_identity::development_display_version(
                "0.4.1-dev",
                "16",
                "0bd929d1",
                false,
            ),
            "0.4.1-dev.16+g0bd929d1"
        );
        assert_eq!(
            crate::build_identity::development_display_version("0.4.1-dev", "16", "0bd929d1", true,),
            "0.4.1-dev.16+g0bd929d1.dirty"
        );
    }

    #[test]
    fn development_display_version_handles_stable_and_metadata_versions() {
        assert_eq!(
            crate::build_identity::development_display_version("0.4.1", "2", "abcdef12", false,),
            "0.4.1-dev.2+gabcdef12"
        );
        assert_eq!(
            crate::build_identity::development_display_version(
                "0.4.1-dev+local",
                "2",
                "abcdef12",
                false,
            ),
            "0.4.1-dev.2+local.gabcdef12"
        );
    }

    #[test]
    fn packaged_layout_identity_uses_only_package_local_inputs() {
        let package = tempdir().unwrap();
        std::fs::create_dir_all(package.path().join("src/nested")).unwrap();
        std::fs::write(
            package.path().join("Cargo.toml"),
            "[package]\nname='jig-sh'\n",
        )
        .unwrap();
        std::fs::write(package.path().join("build.rs"), "fn main() {}\n").unwrap();
        std::fs::write(package.path().join("build_identity.rs"), "shared helper\n").unwrap();
        std::fs::write(package.path().join("src/lib.rs"), "pub fn one() {}\n").unwrap();
        std::fs::write(package.path().join("src/nested/data.txt"), "one\n").unwrap();

        let first = crate::build_identity::compute(package.path(), &build_configuration()).unwrap();
        std::fs::write(package.path().join("src/nested/data.txt"), "two\n").unwrap();
        let second =
            crate::build_identity::compute(package.path(), &build_configuration()).unwrap();

        assert!(first.starts_with("sha256:"));
        assert_ne!(first, second);
    }

    #[test]
    fn packaged_layout_nested_at_crates_jig_ignores_unrelated_host_workspace() {
        let workspace = tempdir().unwrap();
        let package = workspace.path().join("crates/jig");
        std::fs::create_dir_all(package.join("src")).unwrap();
        std::fs::create_dir_all(workspace.path().join("templates/project")).unwrap();
        std::fs::create_dir_all(workspace.path().join("templates/scaffolds")).unwrap();
        std::fs::write(workspace.path().join("Cargo.toml"), "[workspace]\n").unwrap();
        std::fs::write(workspace.path().join("Cargo.lock"), "# host lock\n").unwrap();
        std::fs::write(
            workspace.path().join("templates/project/ambient.jinja"),
            "ambient one\n",
        )
        .unwrap();
        std::fs::write(package.join("Cargo.toml"), "[package]\nname='jig-sh'\n").unwrap();
        std::fs::write(package.join("build.rs"), "fn main() {}\n").unwrap();
        std::fs::write(package.join("build_identity.rs"), "shared helper\n").unwrap();
        std::fs::write(package.join("src/lib.rs"), "pub fn packaged() {}\n").unwrap();

        let layout = crate::build_identity::resolve_source_layout(&package).unwrap();
        assert!(layout.live_template_root("project").is_none());
        let first = crate::build_identity::compute(&package, &build_configuration()).unwrap();
        std::fs::write(
            workspace.path().join("templates/project/ambient.jinja"),
            "ambient two\n",
        )
        .unwrap();
        let second = crate::build_identity::compute(&package, &build_configuration()).unwrap();

        assert!(first.starts_with("sha256:"));
        assert_eq!(first, second);
    }

    #[test]
    fn native_identity_changes_with_behavior_affecting_build_configuration() {
        let package = tempdir().unwrap();
        std::fs::create_dir_all(package.path().join("src")).unwrap();
        std::fs::write(
            package.path().join("Cargo.toml"),
            "[package]\nname='jig-sh'\n",
        )
        .unwrap();
        std::fs::write(package.path().join("build.rs"), "fn main() {}\n").unwrap();
        std::fs::write(package.path().join("build_identity.rs"), "shared helper\n").unwrap();
        std::fs::write(package.path().join("src/lib.rs"), "pub fn packaged() {}\n").unwrap();
        let baseline_configuration = build_configuration();
        let baseline =
            crate::build_identity::compute(package.path(), &baseline_configuration).unwrap();

        for (key, value) in [
            ("TARGET", "aarch64-apple-darwin"),
            ("CARGO_FEATURE_DEV_PROXY", "1"),
            ("PROFILE", "release"),
            ("CARGO_PKG_VERSION", "0.3.1"),
            ("JIG_EMBEDDED_TEMPLATE_SNAPSHOT", "1"),
            ("JIG_BUILD_OFFICIAL_TEMPLATE_PIN", "released"),
        ] {
            let mut changed = baseline_configuration.clone();
            changed.retain(|(existing, _)| existing != key);
            changed.push((key.into(), value.into()));
            assert_ne!(
                baseline,
                crate::build_identity::compute(package.path(), &changed).unwrap(),
                "build identity ignored {key}"
            );
        }
    }

    #[test]
    fn cargo_rerun_keys_cover_every_build_identity_environment_input() {
        let mut configuration = build_configuration();
        configuration.extend([
            ("CARGO_CFG_TARGET_ARCH".into(), "x86_64".into()),
            ("CARGO_FEATURE_DEV_PROXY".into(), "1".into()),
            ("RUSTC_VERSION_VERBOSE".into(), "rustc example".into()),
        ]);

        let keys = crate::build_identity::cargo_rerun_environment_keys(&configuration);

        assert!(
            crate::build_identity::FIXED_BUILD_ENVIRONMENT_KEYS
                .iter()
                .all(|key| keys.iter().any(|candidate| candidate == key))
        );
        assert!(keys.iter().any(|key| key == "CARGO_CFG_TARGET_ARCH"));
        assert!(keys.iter().any(|key| key == "CARGO_FEATURE_DEV_PROXY"));
        assert!(!keys.iter().any(|key| key == "RUSTC_VERSION_VERBOSE"));
        assert!(
            !keys
                .iter()
                .any(|key| key == "JIG_BUILD_OFFICIAL_TEMPLATE_PIN")
        );
    }

    #[test]
    fn refreshed_build_identity_is_a_fixed_point() {
        let package = tempdir().unwrap();
        let snapshot = package.path().join("src/bootstrap/snapshot.rs");
        std::fs::create_dir_all(snapshot.parent().unwrap()).unwrap();
        std::fs::write(
            package.path().join("Cargo.toml"),
            "[package]\nname='jig-sh'\n",
        )
        .unwrap();
        std::fs::write(package.path().join("build.rs"), "fn main() {}\n").unwrap();
        std::fs::write(package.path().join("build_identity.rs"), "shared helper\n").unwrap();
        std::fs::write(package.path().join("src/lib.rs"), "pub fn packaged() {}\n").unwrap();
        std::fs::write(&snapshot, "stale snapshot\n").unwrap();
        let configuration = build_configuration();
        let stale = crate::build_identity::compute(package.path(), &configuration).unwrap();

        let layout = crate::build_identity::resolve_source_layout(package.path()).unwrap();
        let refreshed =
            crate::build_identity::compute_after_input_refresh(&layout, &configuration, |_| {
                std::fs::write(&snapshot, "current snapshot\n").unwrap()
            })
            .unwrap();
        let repeated =
            crate::build_identity::compute_after_input_refresh(&layout, &configuration, |_| {
                std::fs::write(&snapshot, "current snapshot\n").unwrap()
            })
            .unwrap();

        assert_ne!(stale, refreshed);
        assert_eq!(refreshed, repeated);
    }

    #[test]
    fn build_configuration_records_the_final_template_pin_policy() {
        let _guard = crate::test_env::lock_env();
        let configuration =
            crate::build_identity::configuration_from_environment("released").unwrap();

        assert!(configuration.iter().any(|(key, value)| {
            key == "JIG_BUILD_OFFICIAL_TEMPLATE_PIN" && value == "released"
        }));
    }

    #[cfg(unix)]
    #[test]
    fn packaged_layout_rejects_symlinked_source_inputs() {
        use std::os::unix::fs::symlink;

        let package = tempdir().unwrap();
        std::fs::create_dir_all(package.path().join("src")).unwrap();
        std::fs::write(
            package.path().join("Cargo.toml"),
            "[package]\nname='jig-sh'\n",
        )
        .unwrap();
        std::fs::write(package.path().join("build.rs"), "fn main() {}\n").unwrap();
        std::fs::write(package.path().join("build_identity.rs"), "shared helper\n").unwrap();
        std::fs::write(package.path().join("outside.rs"), "pub fn linked() {}\n").unwrap();
        symlink("../outside.rs", package.path().join("src/generated.rs")).unwrap();

        let error =
            crate::build_identity::compute(package.path(), &build_configuration()).unwrap_err();

        assert!(error.contains("must not be a symlink"), "{error}");
        assert!(error.contains("src/generated.rs"), "{error}");
    }

    #[cfg(unix)]
    #[test]
    fn checkout_layout_rejects_symlinked_template_inputs() {
        use std::os::unix::fs::symlink;

        let workspace = tempdir().unwrap();
        let package = workspace.path().join("crates/jig");
        for directory in [
            "crates/jig/src",
            "crates/jig-core",
            "crates/jig-features",
            "templates/project",
            "templates/scaffolds",
        ] {
            std::fs::create_dir_all(workspace.path().join(directory)).unwrap();
        }
        for (relative, contents) in [
            ("Cargo.toml", "[workspace]\n"),
            ("Cargo.lock", "# lock\n"),
            ("crates/jig/Cargo.toml", "[package]\nname='jig-sh'\n"),
            ("crates/jig/build.rs", "fn main() {}\n"),
            ("crates/jig/build_identity.rs", "shared helper\n"),
            ("crates/jig/src/lib.rs", "pub fn checkout() {}\n"),
            ("crates/jig-core/Cargo.toml", "[package]\nname='jig-core'\n"),
            (
                "crates/jig-features/Cargo.toml",
                "[package]\nname='jig-features'\n",
            ),
            (
                "templates/project/.jig.toml.jinja",
                "repo_name = 'fixture'\n",
            ),
            ("outside.jinja", "linked template\n"),
        ] {
            std::fs::write(workspace.path().join(relative), contents).unwrap();
        }
        symlink(
            "../../outside.jinja",
            workspace.path().join("templates/project/linked.jinja"),
        )
        .unwrap();

        let error = crate::build_identity::compute(&package, &build_configuration()).unwrap_err();

        assert!(error.contains("must not be a symlink"), "{error}");
        assert!(error.contains("templates/project/linked.jinja"), "{error}");
    }
}
