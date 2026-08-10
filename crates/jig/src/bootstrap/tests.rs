use tempfile::{TempDir, tempdir};

use super::path;
use super::*;
use crate::test_env::{EnvVarGuard, lock_env};

const CURRENT_GENERATED_LAUNCHER_TEMPLATE: &str =
    include_str!("embedded_template_snapshots/scripts/jig.jinja");
const CURRENT_GENERATED_INSTALLER: &str =
    include_str!("embedded_template_snapshots/scripts/install-jig.sh.jinja");

fn current_generated_launcher() -> String {
    CURRENT_GENERATED_LAUNCHER_TEMPLATE.replace(
        "<<[ _jig.contract_version ]>>",
        &crate::context::CURRENT_CONTRACT_VERSION.to_string(),
    )
}

#[test]
fn launcher_repair_cache_publication_restores_all_prior_caches_on_late_failure() {
    let temp = tempdir().unwrap();
    let cache_base = temp.path().join("cache");
    fs::create_dir(&cache_base).unwrap();
    let staging = tempfile::Builder::new()
        .prefix(".jig-launcher-repair-")
        .tempdir_in(&cache_base)
        .unwrap();

    let runtime_staged = staging.path().join("runtime");
    fs::create_dir(&runtime_staged).unwrap();
    fs::write(runtime_staged.join("sentinel"), "new-runtime").unwrap();
    // Deliberately omit the default staged cache so publication fails only
    // after runtime has replaced its existing cache.

    for cache_name in ["contract-3-runtime", "contract-3"] {
        let cache = cache_base.join(cache_name);
        fs::create_dir(&cache).unwrap();
        fs::write(cache.join("sentinel"), format!("old-{cache_name}")).unwrap();
    }

    let error = publish_launcher_repair_caches(
        staging,
        &cache_base,
        3,
        &[RuntimeCacheProfile::Runtime, RuntimeCacheProfile::Default],
    )
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("Failed to publish staged launcher-repair cache"),
        "{error:#}"
    );
    for cache_name in ["contract-3-runtime", "contract-3"] {
        assert_eq!(
            fs::read_to_string(cache_base.join(cache_name).join("sentinel")).unwrap(),
            format!("old-{cache_name}")
        );
    }
}

#[test]
fn repair_seed_retirement_preserves_ordinary_cache_provenance() {
    let repo = tempdir().unwrap();
    let default_cache = runtime_cache_base(repo.path()).join(runtime_profile_cache_name(
        crate::context::CURRENT_CONTRACT_VERSION,
        RuntimeCacheProfile::Default,
    ));
    let runtime_cache = runtime_cache_base(repo.path()).join(runtime_profile_cache_name(
        crate::context::CURRENT_CONTRACT_VERSION,
        RuntimeCacheProfile::Runtime,
    ));
    for cache in [&default_cache, &runtime_cache] {
        fs::create_dir_all(cache.join("bin")).unwrap();
        fs::write(cache.join("bin/jig"), "cached runtime").unwrap();
        fs::write(cache.join(".jig-source-metadata-stamp"), "metadata\n").unwrap();
    }
    fs::write(
        default_cache.join(".jig-source-stamp"),
        format!("{LAUNCHER_REPAIR_SEED_STAMP_HEADER}\nsource:fixture\n"),
    )
    .unwrap();
    fs::write(
        runtime_cache.join(".jig-source-stamp"),
        "source-v1\nordinary\n",
    )
    .unwrap();

    let outcome =
        retire_launcher_repair_seeded_caches(repo.path(), crate::context::CURRENT_CONTRACT_VERSION);
    assert_eq!(outcome.retired, 1);
    assert!(outcome.errors.is_empty());

    assert!(!default_cache.join(".jig-source-stamp").exists());
    assert!(!default_cache.join(".jig-source-metadata-stamp").exists());
    assert!(default_cache.join("bin/jig").exists());
    assert_eq!(
        fs::read_to_string(runtime_cache.join(".jig-source-stamp")).unwrap(),
        "source-v1\nordinary\n"
    );
    assert!(runtime_cache.join(".jig-source-metadata-stamp").exists());
}

#[test]
fn repair_seed_retirement_does_not_block_or_fail_committed_harness_changes() {
    let repo = tempdir().unwrap();
    let cache = runtime_cache_base(repo.path()).join(runtime_profile_cache_name(
        crate::context::CURRENT_CONTRACT_VERSION,
        RuntimeCacheProfile::Default,
    ));
    fs::create_dir_all(cache.join("bin")).unwrap();
    fs::write(cache.join("bin/jig"), "cached runtime").unwrap();
    fs::write(
        cache.join(".jig-source-stamp"),
        format!("{LAUNCHER_REPAIR_SEED_STAMP_HEADER}\nsource:fixture\n"),
    )
    .unwrap();
    let active_runtime = RuntimeCacheLocks::acquire(
        std::slice::from_ref(&cache),
        RuntimeCacheLockPolicy::immediate(),
    )
    .unwrap();

    let blocked =
        retire_launcher_repair_seeded_caches(repo.path(), crate::context::CURRENT_CONTRACT_VERSION);
    assert_eq!(blocked.retired, 0);
    assert_eq!(blocked.errors.len(), 1);
    let warning = launcher_repair_retirement_warning(&blocked.errors[0]);
    assert!(warning.starts_with("Warning: harness changes were committed"));
    assert!(warning.contains(LAUNCHER_REPAIR_RETIREMENT_RETRY_GUIDANCE));
    assert!(warning.contains("Timed out waiting for Jig installer lock"));
    assert!(
        warning.contains(&cache.display().to_string()),
        "warning must identify the cache whose lock blocked retirement: {warning}"
    );
    assert!(
        cache.join(".jig-source-stamp").exists(),
        "busy repair cache must remain intact for a later retirement attempt"
    );

    drop(active_runtime);
    let outcome =
        retire_launcher_repair_seeded_caches(repo.path(), crate::context::CURRENT_CONTRACT_VERSION);
    assert_eq!(outcome.retired, 1);
    assert!(outcome.errors.is_empty());
}

#[test]
fn repair_seed_retirement_reports_completed_work_before_a_later_cache_error() {
    let repo = tempdir().unwrap();
    let cache_base = runtime_cache_base(repo.path());
    let default_cache = cache_base.join(runtime_profile_cache_name(
        crate::context::CURRENT_CONTRACT_VERSION,
        RuntimeCacheProfile::Default,
    ));
    let runtime_cache = cache_base.join(runtime_profile_cache_name(
        crate::context::CURRENT_CONTRACT_VERSION,
        RuntimeCacheProfile::Runtime,
    ));
    for cache in [&default_cache, &runtime_cache] {
        fs::create_dir_all(cache.join("bin")).unwrap();
        fs::write(cache.join("bin/jig"), "cached runtime").unwrap();
        fs::write(
            cache.join(".jig-source-stamp"),
            format!("{LAUNCHER_REPAIR_SEED_STAMP_HEADER}\nsource:fixture\n"),
        )
        .unwrap();
    }
    fs::create_dir(runtime_cache.join(".jig-source-metadata-stamp")).unwrap();

    let outcome =
        retire_launcher_repair_seeded_caches(repo.path(), crate::context::CURRENT_CONTRACT_VERSION);

    assert_eq!(outcome.retired, 1);
    assert_eq!(outcome.errors.len(), 1);
    assert!(!default_cache.join(".jig-source-stamp").exists());
    assert!(runtime_cache.join(".jig-source-stamp").exists());
}

#[test]
fn launcher_repair_cache_publication_rolls_back_after_a_later_transaction_failure() {
    let temp = tempdir().unwrap();
    let cache_base = temp.path().join("cache");
    fs::create_dir(&cache_base).unwrap();
    let staging = tempfile::Builder::new()
        .prefix(".jig-launcher-repair-")
        .tempdir_in(&cache_base)
        .unwrap();

    for (profile, cache_name) in [("runtime", "contract-3-runtime"), ("default", "contract-3")] {
        let staged = staging.path().join(profile);
        fs::create_dir(&staged).unwrap();
        fs::write(staged.join("sentinel"), format!("new-{profile}")).unwrap();
        let existing = cache_base.join(cache_name);
        fs::create_dir(&existing).unwrap();
        fs::write(existing.join("sentinel"), format!("old-{profile}")).unwrap();
    }

    let publication = publish_launcher_repair_caches(
        staging,
        &cache_base,
        3,
        &[RuntimeCacheProfile::Runtime, RuntimeCacheProfile::Default],
    )
    .unwrap();
    assert_eq!(
        fs::read_to_string(cache_base.join("contract-3-runtime/sentinel")).unwrap(),
        "new-runtime"
    );
    for cache_name in ["contract-3-runtime", "contract-3"] {
        assert!(
            cache_base.join(format!("{cache_name}.lock")).is_dir(),
            "publication must hold the installer lock for {cache_name}"
        );
    }

    let error = publication.finish_failed(anyhow::anyhow!("script transaction commit failed"));
    assert!(
        error
            .to_string()
            .contains("script transaction commit failed")
    );
    for (profile, cache_name) in [("runtime", "contract-3-runtime"), ("default", "contract-3")] {
        assert_eq!(
            fs::read_to_string(cache_base.join(cache_name).join("sentinel")).unwrap(),
            format!("old-{profile}")
        );
        assert!(!cache_base.join(format!("{cache_name}.lock")).exists());
    }
}

#[test]
fn launcher_repair_cache_publication_refuses_an_active_installer_lock_before_mutation() {
    let temp = tempdir().unwrap();
    let cache_base = temp.path().join("cache");
    fs::create_dir(&cache_base).unwrap();
    let staging = tempfile::Builder::new()
        .prefix(".jig-launcher-repair-")
        .tempdir_in(&cache_base)
        .unwrap();
    let staged = staging.path().join("runtime");
    fs::create_dir(&staged).unwrap();
    fs::write(staged.join("sentinel"), "new-runtime").unwrap();

    let destination = cache_base.join("contract-3-runtime");
    fs::create_dir(&destination).unwrap();
    fs::write(destination.join("sentinel"), "old-runtime").unwrap();
    let lock = cache_base.join("contract-3-runtime.lock");
    fs::create_dir(&lock).unwrap();

    let error = publish_launcher_repair_caches_with_lock_policy(
        staging,
        &cache_base,
        3,
        &[RuntimeCacheProfile::Runtime],
        RuntimeCacheLockPolicy::immediate(),
    )
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("Timed out waiting for Jig installer lock"),
        "{error:#}"
    );
    assert_eq!(
        fs::read_to_string(destination.join("sentinel")).unwrap(),
        "old-runtime"
    );
    assert!(lock.is_dir(), "repair must not remove an active lock");
}

#[test]
fn launcher_repair_cache_publication_releases_installer_locks_after_commit() {
    let temp = tempdir().unwrap();
    let cache_base = temp.path().join("cache");
    fs::create_dir(&cache_base).unwrap();
    let staging = tempfile::Builder::new()
        .prefix(".jig-launcher-repair-")
        .tempdir_in(&cache_base)
        .unwrap();
    let staged = staging.path().join("runtime");
    fs::create_dir(&staged).unwrap();
    fs::write(staged.join("sentinel"), "new-runtime").unwrap();
    let lock = cache_base.join("contract-3-runtime.lock");

    let publication =
        publish_launcher_repair_caches(staging, &cache_base, 3, &[RuntimeCacheProfile::Runtime])
            .unwrap();
    assert!(lock.is_dir());

    publication.commit();

    assert!(!lock.exists());
    assert_eq!(
        fs::read_to_string(cache_base.join("contract-3-runtime/sentinel")).unwrap(),
        "new-runtime"
    );
}

#[test]
fn launcher_repair_cache_rollback_preserves_backups_when_recovery_fails() {
    let temp = tempdir().unwrap();
    let staging = tempfile::Builder::new()
        .prefix(".jig-launcher-repair-")
        .tempdir_in(temp.path())
        .unwrap();
    let staging_path = staging.path().to_path_buf();
    let backup = staging.path().join("backup-runtime");
    fs::create_dir(&backup).unwrap();
    fs::write(backup.join("sentinel"), "old-runtime").unwrap();
    let mut published = vec![PublishedLauncherRepairCache {
        destination: temp.path().join("missing-published-runtime"),
        backup: Some(backup.clone()),
    }];

    let rollback = rollback_published_repair_caches(&staging, &mut published).unwrap_err();
    let error = preserve_launcher_repair_staging(
        staging,
        anyhow::anyhow!("primary publication failure"),
        &[format!("{rollback:#}")],
    );

    assert!(
        error
            .to_string()
            .contains("Recovery artifacts were preserved")
    );
    assert_eq!(
        fs::read_to_string(backup.join("sentinel")).unwrap(),
        "old-runtime"
    );
    fs::remove_dir_all(staging_path).unwrap();
}

#[test]
fn launcher_repair_reaps_only_stale_disposable_staging() {
    let temp = tempdir().unwrap();
    let cache_base = temp.path();
    let abandoned = cache_base.join(".jig-launcher-repair-abandoned");
    let active = cache_base.join(".jig-launcher-repair-active");
    let recovery = cache_base.join(".jig-launcher-repair-recovery");
    let unrelated = cache_base.join("unrelated");
    for path in [&abandoned, &active, &recovery, &unrelated] {
        fs::create_dir(path).unwrap();
    }
    fs::create_dir(recovery.join("backup-runtime")).unwrap();

    assert_eq!(
        reap_stale_launcher_repair_staging(cache_base, SystemTime::now()).unwrap(),
        0
    );
    assert!(abandoned.is_dir());
    assert!(active.is_dir());

    let removed = reap_stale_launcher_repair_staging(
        cache_base,
        SystemTime::now() + STALE_LAUNCHER_REPAIR_STAGING_AGE + Duration::from_secs(1),
    )
    .unwrap();

    assert_eq!(removed, 2);
    assert!(!abandoned.exists());
    assert!(!active.exists());
    assert!(recovery.is_dir());
    assert!(unrelated.is_dir());
}

fn template_repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .unwrap()
        .to_path_buf()
}

#[test]
fn adopt_and_update_guard_rejects_newer_declared_contracts_only() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".agent")).unwrap();
    let manifest = temp.path().join(".agent/jig-contract.json");

    fs::write(
        &manifest,
        format!(
            "{{\"contract_version\": {}}}\n",
            crate::context::CURRENT_CONTRACT_VERSION + 1
        ),
    )
    .unwrap();
    let error = reject_newer_declared_contract(temp.path()).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("Refusing to rewrite repository contract")
    );
    assert!(error.to_string().contains("--force does not permit"));

    fs::write(
        &manifest,
        format!(
            "{{\"contract_version\": {}}}\n",
            crate::context::CURRENT_CONTRACT_VERSION
        ),
    )
    .unwrap();
    reject_newer_declared_contract(temp.path()).unwrap();

    fs::write(&manifest, "{\n").unwrap();
    reject_newer_declared_contract(temp.path()).unwrap();
}

#[test]
fn embedded_generated_launcher_pair_remains_recognizable_for_narrow_repair() {
    assert!(recognizable_generated_launcher(
        &current_generated_launcher()
    ));
    assert!(recognizable_contract_launcher(&current_generated_launcher()));
    assert!(recognizable_generated_installer(
        CURRENT_GENERATED_INSTALLER
    ));
    assert!(recognizable_contract_installer(CURRENT_GENERATED_INSTALLER));
}

#[test]
fn published_beta_one_launcher_pair_remains_recognizable_for_narrow_repair() {
    // These are the generated-script signatures shipped by v0.2.0-beta.1.
    // Published repos from that release predate the managed-path manifest, so
    // launcher-only recovery depends on recognizing this Bash-era pair.
    let launcher = r#"#!/usr/bin/env bash
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
INSTALLER="$ROOT_DIR/scripts/install-jig.sh"
JIG_VERSION="0.2.0-beta.1"
binary_version() { :; }
use_matching_binary() {
  actual_version="$(binary_version "$bin_path" || true)"
}
exec "$bin_path" "$@"
"#;
    let installer = r#"#!/usr/bin/env bash
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
ANSWERS_FILE="$ROOT_DIR/.jig.toml"
JIG_VERSION="0.2.0-beta.1"
assert_exact_version() { :; }
acquire_install_lock() { :; }
install_from_local_source() { :; }
install_from_git_source() { :; }
printf '%s\n' "$BIN_PATH"
"#;

    assert!(recognizable_generated_launcher(launcher));
    assert!(recognizable_generated_installer(installer));
    assert!(!recognizable_contract_launcher(launcher));
    assert!(!recognizable_contract_installer(installer));

    let repo = tempdir().unwrap();
    fs::create_dir_all(repo.path().join("scripts")).unwrap();
    fs::write(repo.path().join("scripts/jig"), launcher).unwrap();
    fs::write(repo.path().join("scripts/install-jig.sh"), installer).unwrap();
    assert_eq!(
        legacy_launcher_only_paths(repo.path()).unwrap(),
        LAUNCHER_ONLY_MANAGED_PATHS
            .map(PathBuf::from)
            .into_iter()
            .collect()
    );
}

#[test]
fn current_contract_recognition_requires_repository_epoch_enforcement() {
    let stale_launcher = current_generated_launcher()
        .replace("# jig-runtime-repository-scope:v1", "# stale-scope-marker");
    let stale_installer = CURRENT_GENERATED_INSTALLER
        .replace("# jig-runtime-repository-scope:v1", "# stale-scope-marker");

    assert!(recognizable_generated_launcher(&stale_launcher));
    assert!(recognizable_generated_installer(&stale_installer));
    assert!(!recognizable_contract_launcher(&stale_launcher));
    assert!(!recognizable_contract_installer(&stale_installer));
}

#[test]
fn current_contract_recognition_relies_on_protocol_markers_not_shell_fragments() {
    let launcher = current_generated_launcher().replace(
        "set -- \\\n    --__launcher-contract-version",
        "set -- \\\n        --renamed-launcher-contract-option",
    );
    let installer = CURRENT_GENERATED_INSTALLER.replace(
        "--repository-scope)\n      REPOSITORY_SCOPE=1",
        "--renamed-repository-scope)\n          RENAMED_REPOSITORY_SCOPE=1",
    );

    assert!(recognizable_contract_launcher(&launcher));
    assert!(recognizable_contract_installer(&installer));
}

#[test]
fn staged_launcher_contract_must_match_the_staged_manifest_epoch() {
    let repo = tempdir().unwrap();
    fs::create_dir_all(repo.path().join("scripts")).unwrap();
    fs::write(
        repo.path().join("scripts/jig"),
        "#!/bin/sh\nCONTRACT_VERSION=\"3\"\n",
    )
    .unwrap();

    renderer::validate_staged_runtime_contract(repo.path(), 3).unwrap();
    let error = renderer::validate_staged_runtime_contract(repo.path(), 4)
        .unwrap_err()
        .to_string();
    assert!(error.contains("launcher"), "{error}");
    assert!(error.contains("contract 3"), "{error}");
    assert!(error.contains("manifest declares contract 4"), "{error}");

    fs::write(
        repo.path().join("scripts/jig"),
        "#!/bin/sh\nJIG_VERSION=\"0.2.0-beta.1\"\n",
    )
    .unwrap();
    renderer::validate_staged_runtime_contract(repo.path(), 3).unwrap();
}

#[test]
fn staged_current_contract_requires_repository_scoped_runtime_scripts() {
    let repo = tempdir().unwrap();
    fs::create_dir_all(repo.path().join("scripts")).unwrap();
    fs::write(
        repo.path().join("scripts/jig"),
        format!(
            "#!/bin/sh\nCONTRACT_VERSION=\"{}\"\n",
            crate::context::CURRENT_CONTRACT_VERSION
        ),
    )
    .unwrap();
    fs::write(
        repo.path().join("scripts/install-jig.sh"),
        CURRENT_GENERATED_INSTALLER,
    )
    .unwrap();

    let error = renderer::validate_staged_runtime_contract(
        repo.path(),
        crate::context::CURRENT_CONTRACT_VERSION,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("launcher"), "{error}");
    assert!(
        error.contains("repository-scoped runtime protocol"),
        "{error}"
    );

    fs::write(
        repo.path().join("scripts/jig"),
        current_generated_launcher(),
    )
    .unwrap();
    fs::write(
        repo.path().join("scripts/install-jig.sh"),
        "#!/usr/bin/env bash\nset -euo pipefail\n",
    )
    .unwrap();
    let error = renderer::validate_staged_runtime_contract(
        repo.path(),
        crate::context::CURRENT_CONTRACT_VERSION,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("installer"), "{error}");
    assert!(
        error.contains("repository-scoped runtime protocol"),
        "{error}"
    );

    fs::write(
        repo.path().join("scripts/install-jig.sh"),
        CURRENT_GENERATED_INSTALLER,
    )
    .unwrap();
    renderer::validate_staged_runtime_contract(
        repo.path(),
        crate::context::CURRENT_CONTRACT_VERSION,
    )
    .unwrap();
}

#[test]
fn launcher_repair_requires_the_complete_render_answer_shape() {
    let repo = tempdir().unwrap();
    let answers = fs::read_to_string(template_repo_root().join(".jig.toml")).unwrap();
    let incomplete = answers
        .lines()
        .filter(|line| !line.starts_with("sqlx_enabled ="))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    fs::write(repo.path().join(".jig.toml"), &incomplete).unwrap();

    assert!(RepoContext::validate_config_file(repo.path()).is_ok());
    assert!(!launcher_only_repair_answers_are_valid(repo.path()));

    fs::write(
        repo.path().join(".jig.toml"),
        format!("sqlx_enabled = false\n{incomplete}"),
    )
    .unwrap();
    assert!(launcher_only_repair_answers_are_valid(repo.path()));
}

#[test]
fn launcher_repair_scrubs_injection_and_installer_lock_reentry() {
    assert!(
        LAUNCHER_REPAIR_ENVIRONMENT_KEYS.contains(&"JIG_INSTALL_LOCK_TOKEN"),
        "repair helpers must not inherit the installer lock re-entry capability"
    );
    let mut command = std::process::Command::new("fixture");
    for &key in LAUNCHER_REPAIR_ENVIRONMENT_KEYS {
        command.env(key, "untrusted");
    }
    command.env("JIG_SAFE_FIXTURE_VALUE", "preserved");

    sanitize_launcher_repair_environment(&mut command);

    for &expected in LAUNCHER_REPAIR_ENVIRONMENT_KEYS {
        let value = command
            .get_envs()
            .find(|(key, _)| *key == std::ffi::OsStr::new(expected))
            .unwrap_or_else(|| panic!("missing explicit removal for {expected}"))
            .1;
        assert!(value.is_none(), "repair subprocess preserved {expected}");
    }
    assert_eq!(
        command
            .get_envs()
            .find(|(key, _)| *key == std::ffi::OsStr::new("JIG_SAFE_FIXTURE_VALUE"))
            .and_then(|(_, value)| value),
        Some(std::ffi::OsStr::new("preserved"))
    );
}

#[cfg(unix)]
#[test]
fn launcher_repair_trusted_path_policy_allows_only_safe_root_owned_components() {
    assert!(root_owned_nonwritable_component(0, 0o755, true));
    assert!(!root_owned_nonwritable_component(0, 0o775, true));
    assert!(!root_owned_nonwritable_component(501, 0o755, true));

    assert!(root_owned_nonwritable_component(0, 0o1777, false));
    assert!(!root_owned_nonwritable_component(0, 0o0777, false));
    assert!(!root_owned_nonwritable_component(501, 0o1777, false));
    assert!(!is_root_owned_nonwritable_path(Path::new(
        "/jig-test-path-that-must-not-exist"
    )));
}

fn copy_dir_recursive(source: &Path, destination: &Path) {
    copy_dir_recursive_inner(source, destination, Path::new(""));
}

fn copy_dir_recursive_inner(source: &Path, destination: &Path, relative: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let entry_name = entry.file_name();
        let entry_relative = relative.join(&entry_name);
        if skip_template_fixture_path(&entry_relative) {
            continue;
        }

        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_type = entry.file_type().unwrap();

        if file_type.is_dir() {
            copy_dir_recursive_inner(&source_path, &destination_path, &entry_relative);
            continue;
        }

        if file_type.is_symlink() {
            if let Some(parent) = destination_path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            let target = fs::read_link(&source_path).unwrap();
            create_symlink(&target, &destination_path).unwrap();
            continue;
        }

        if let Some(parent) = destination_path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::copy(&source_path, &destination_path).unwrap();
    }
}

fn skip_template_fixture_path(relative: &Path) -> bool {
    matches!(
        relative.to_str(),
        Some(".git")
            | Some("target")
            | Some(".agent/.cache")
            | Some(".agent/plans")
            | Some(".agent/state")
    )
}

fn materialize_template_worktree() -> TempDir {
    let temp = tempdir().unwrap();
    copy_dir_recursive(
        &template_repo_root().join("templates"),
        &temp.path().join("templates"),
    );
    temp
}

fn materialize_template_git_worktree() -> TempDir {
    let temp = materialize_template_worktree();
    init_git_repo_for_test(temp.path());
    git(temp.path(), ["add", "."]).unwrap();
    git(temp.path(), ["commit", "-m", "template"]).unwrap();
    temp
}

fn init_git_repo_for_test(path: &Path) {
    git(path, ["init", "-b", "main"]).unwrap();
    git(path, ["config", "user.name", "Fixture"]).unwrap();
    git(path, ["config", "user.email", "fixture@example.com"]).unwrap();
}

fn write_test_crate_guide(repo: &Path) {
    fs::create_dir_all(repo.join("crates/api")).unwrap();
    fs::write(repo.join("crates/api/AGENTS.md"), "crate guide").unwrap();
}

fn with_test_build_template_pin_policy<T>(
    policy: BuildTemplatePinPolicy,
    run: impl FnOnce() -> T,
) -> T {
    struct Guard(Option<BuildTemplatePinPolicy>);

    impl Drop for Guard {
        fn drop(&mut self) {
            TEST_BUILD_TEMPLATE_PIN_POLICY.with(|slot| slot.set(self.0));
        }
    }

    let previous = TEST_BUILD_TEMPLATE_PIN_POLICY.with(|slot| {
        let previous = slot.get();
        slot.set(Some(policy));
        previous
    });
    let _guard = Guard(previous);
    run()
}

fn adopt_repo_for_test(repo: &Path, template: &Path, template_mode: TemplateMode) {
    run_adopt(AdoptOpts {
        path: repo.to_path_buf(),
        template: Some(template.display().to_string()),
        template_mode: Some(template_mode),
        vcs_ref: None,
        force: false,
        write: true,
        minimal: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("demo".into()),
            sqlx_enabled: Some(false),
            ..AnswerOpts::default()
        },
    })
    .unwrap();
}

fn commit_template_root_guide(template: &Path, contents: &str, message: &str) -> String {
    fs::write(
        template.join("templates/project/AGENTS.md.jinja"),
        format!(
            "# Repository Guidelines\n\n<!-- BEGIN JIG MANAGED BLOCK -->\n{contents}<!-- END JIG MANAGED BLOCK -->\n"
        ),
    )
    .unwrap();
    git(template, ["add", "templates/project/AGENTS.md.jinja"]).unwrap();
    git(template, ["commit", "-m", message]).unwrap();
    git_stdout(template, ["rev-parse", "HEAD"]).unwrap()
}

fn push_template_main(template: &Path, remote_url: &str) {
    git(template, ["push", remote_url, "HEAD:refs/heads/main"]).unwrap();
}

struct NormalizedRemoteCommittedFixture {
    _root: TempDir,
    repo: PathBuf,
    template: TempDir,
    remote_url: String,
    answers_path: PathBuf,
}

impl NormalizedRemoteCommittedFixture {
    fn new(legacy_committed_state: bool) -> Self {
        let root = tempdir().unwrap();
        let repo = root.path().join("repo");
        let remote = root.path().join("template-remote.git");
        let template = materialize_template_git_worktree();
        let remote_url = format!("file://{}", remote.display());

        write_test_crate_guide(&repo);
        // Build the fixture remote through Git's normal receive path. A local
        // `clone --bare --no-hardlinks` intermittently loses its temporary pack
        // index under parallel test load, even though these repositories are
        // independent.
        git(
            template.path(),
            [
                "init",
                "--bare",
                "--initial-branch=main",
                &remote.display().to_string(),
            ],
        )
        .unwrap();
        push_template_main(template.path(), &remote_url);

        adopt_repo_for_test(&repo, template.path(), TemplateMode::Committed);
        init_git_repo_for_test(&repo);
        git(&repo, ["add", "."]).unwrap();
        git(&repo, ["commit", "-m", "adopt"]).unwrap();

        let answers_path = repo.join(".jig.toml");
        let mut answers = read_answers_toml(&answers_path).unwrap();
        answers.insert("_src_path".into(), TomlValue::String(remote_url.clone()));
        if legacy_committed_state {
            answers.remove(TEMPLATE_LOCAL_PATH_KEY);
        }
        write_answers_toml(&answers_path, &answers).unwrap();
        git(&repo, ["add", ".jig.toml"]).unwrap();
        git(&repo, ["commit", "-m", "normalize source"]).unwrap();

        Self {
            _root: root,
            repo,
            template,
            remote_url,
            answers_path,
        }
    }
}

mod basic;
mod committed;
mod frontend_adoption;
mod status_provider;
mod template_mode;
mod template_source;
mod windows_dependency_checker;
