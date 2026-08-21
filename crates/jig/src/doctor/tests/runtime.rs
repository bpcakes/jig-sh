use super::*;
use std::time::SystemTime;

use crate::context::INSTALLER_CACHE_LAYOUT_MARKER;

fn generated_launcher_with_contract(contract_version: u32) -> String {
    current_generated_launcher().replace(
        &format!(
            "CONTRACT_VERSION=\"{}\"",
            crate::context::CURRENT_CONTRACT_VERSION
        ),
        &format!("CONTRACT_VERSION=\"{contract_version}\""),
    )
}

#[test]
fn runtime_check_rejects_launcher_without_contract_probe() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join("scripts")).unwrap();
    fs::write(temp.path().join("scripts/jig"), "#!/usr/bin/env bash\n").unwrap();
    fs::write(temp.path().join("scripts/install-jig.sh"), "").unwrap();

    let output = runtime_check(
        temp.path(),
        Some(crate::context::CURRENT_CONTRACT_VERSION),
        None,
        true,
    );

    assert!(!output.ok);
    assert_eq!(output.status, "outdated");
    let fix = output.fix.as_deref().unwrap();
    assert!(!fix.contains("--launcher-only"));
    assert!(fix.contains("cargo install jig-sh"));
    assert!(fix.contains(crate::bootstrap::MANAGED_PATHS_MANIFEST_PATH));
    assert!(fix.contains(" adopt "));
    assert_eq!(
        output.data["runtime_executable"],
        std::env::current_exe().unwrap().to_string_lossy().as_ref()
    );
}

#[test]
fn runtime_check_rejects_comment_only_contract_probe() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join("scripts")).unwrap();
    fs::write(
        temp.path().join("scripts/jig"),
        "#!/bin/sh\n# Runtime selection uses __runtime-compatible.\n",
    )
    .unwrap();
    fs::write(temp.path().join("scripts/install-jig.sh"), "").unwrap();
    fs::create_dir_all(temp.path().join(".agent")).unwrap();
    fs::write(
        temp.path()
            .join(crate::bootstrap::MANAGED_PATHS_MANIFEST_PATH),
        "{}",
    )
    .unwrap();

    let output = runtime_check(temp.path(), Some(4), None, true);

    assert!(!output.ok);
    assert_eq!(output.status, "outdated");
    assert_eq!(output.data["launcher_uses_contract_probe"], false);
}

#[test]
fn runtime_check_reports_legacy_launcher_migration() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join("scripts")).unwrap();
    fs::write(
        temp.path().join("scripts/jig"),
        "#!/bin/sh\nJIG_VERSION=\"0.2.0-beta.1\"\n",
    )
    .unwrap();
    fs::write(temp.path().join("scripts/install-jig.sh"), "").unwrap();
    fs::create_dir_all(temp.path().join(".agent")).unwrap();
    fs::write(
        temp.path()
            .join(crate::bootstrap::MANAGED_PATHS_MANIFEST_PATH),
        "{}",
    )
    .unwrap();

    let output = runtime_check(temp.path(), Some(3), Some("0.2.0-beta.1"), true);

    assert!(!output.ok);
    assert_eq!(output.status, "migration needed");
    assert_eq!(output.data["legacy_launcher_version"], "0.2.0-beta.1");
    assert!(output.fix.as_deref().unwrap().contains("--launcher-only"));
}

#[test]
fn intact_legacy_launcher_recommends_full_migration_before_narrow_repair() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join("scripts")).unwrap();
    fs::write(
        temp.path().join("scripts/jig"),
        r#"#!/usr/bin/env bash
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
INSTALLER="$ROOT_DIR/scripts/install-jig.sh"
JIG_VERSION="0.2.0-beta.1"
binary_version() { :; }
use_matching_binary() {
  actual_version="$(binary_version "$bin_path" || true)"
}
exec "$bin_path" "$@"
"#,
    )
    .unwrap();
    fs::write(
        temp.path().join("scripts/install-jig.sh"),
        r#"#!/usr/bin/env bash
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
ANSWERS_FILE="$ROOT_DIR/.jig.toml"
JIG_VERSION="0.2.0-beta.1"
assert_exact_version() { :; }
acquire_install_lock() { :; }
install_from_local_source() { :; }
install_from_git_source() { :; }
printf '%s\n' "$BIN_PATH"
"#,
    )
    .unwrap();
    fs::create_dir_all(temp.path().join(".agent")).unwrap();
    fs::write(
        temp.path()
            .join(crate::bootstrap::MANAGED_PATHS_MANIFEST_PATH),
        "{}",
    )
    .unwrap();

    let output = runtime_check(temp.path(), Some(3), Some("0.2.0-beta.1"), true);

    let fix = output.fix.as_deref().unwrap();
    let full_update = format!(
        "update {} --force",
        crate::shell::quote(&temp.path().to_string_lossy())
    );
    assert!(fix.contains(&full_update), "{fix}");
    assert!(fix.contains("--launcher-only --force"), "{fix}");
    assert!(
        fix.find(&full_update) < fix.find("--launcher-only --force"),
        "{fix}"
    );
}

#[test]
fn runtime_check_reports_unreadable_launcher() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join("scripts")).unwrap();
    fs::create_dir_all(temp.path().join("scripts/jig")).unwrap();
    fs::write(temp.path().join("scripts/install-jig.sh"), "").unwrap();
    fs::create_dir_all(temp.path().join(".agent")).unwrap();
    fs::write(
        temp.path()
            .join(crate::bootstrap::MANAGED_PATHS_MANIFEST_PATH),
        "{}",
    )
    .unwrap();

    let output = runtime_check(temp.path(), Some(4), None, true);

    assert!(!output.ok);
    assert_eq!(output.status, "unreadable");
    assert!(output.detail.contains("unreadable"));
    assert!(output.data["launcher_error"].as_str().is_some());
    let fix = output.fix.as_deref().unwrap();
    assert!(fix.contains(" --launcher-only --force"));
    assert!(fix.contains(&temp.path().display().to_string()));
    assert!(!fix.contains("<repo>"));
}

#[test]
fn generated_launcher_is_recognized_by_runtime_check() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join("scripts")).unwrap();
    fs::write(
        temp.path().join("scripts/jig"),
        current_generated_launcher(),
    )
    .unwrap();
    fs::write(
        temp.path().join("scripts/install-jig.sh"),
        CURRENT_GENERATED_INSTALLER,
    )
    .unwrap();

    let output = runtime_check(
        temp.path(),
        Some(crate::context::CURRENT_CONTRACT_VERSION),
        None,
        true,
    );

    assert!(output.ok, "{}", output.detail);
    assert_eq!(output.data["launcher_uses_contract_probe"], true);
    assert_eq!(
        output.data["launcher_contract_version"],
        crate::context::CURRENT_CONTRACT_VERSION
    );
}

#[test]
fn repaired_legacy_runtime_reports_its_seeded_cache_dependency() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join("scripts")).unwrap();
    fs::write(
        temp.path().join("scripts/jig"),
        generated_launcher_with_contract(3),
    )
    .unwrap();
    fs::write(
        temp.path().join("scripts/install-jig.sh"),
        CURRENT_GENERATED_INSTALLER,
    )
    .unwrap();
    let stamp_dir = temp.path().join(".agent/.cache/jig/contract-3-runtime");
    fs::create_dir_all(&stamp_dir).unwrap();
    fs::write(
        stamp_dir.join(".jig-source-stamp"),
        "jig-seeded-runtime-v1\nbinary:sha256:fixture\nsource:sha256:fixture\n",
    )
    .unwrap();

    let output = runtime_check(temp.path(), Some(3), Some("0.2.0-beta.1"), true);

    assert!(output.ok, "{}", output.detail);
    assert_eq!(output.status, "compatible");
    assert!(output.detail.contains("launcher-repair seeded cache"));
    assert!(output.fix.is_none());
    assert_eq!(output.data["launcher_repair_seeded_cache"], true);

    let advisory = launcher_repair_cache_check(temp.path(), 3);
    assert!(!advisory.required);
    assert!(!advisory.ok);
    assert_eq!(advisory.status, "temporary seed");
    assert!(advisory.detail.contains("fresh-clone or cache-cleared"));
    let fix = advisory.fix.as_deref().expect("repair seed needs a fix");
    assert!(fix.contains(" adopt "));
    assert!(fix.contains("--write"));
    assert!(fix.contains("--force"));
}

#[test]
fn repaired_current_runtime_exposes_a_structured_cache_rebuild_fix() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join("scripts")).unwrap();
    fs::write(
        temp.path().join("scripts/jig"),
        current_generated_launcher(),
    )
    .unwrap();
    fs::write(
        temp.path().join("scripts/install-jig.sh"),
        CURRENT_GENERATED_INSTALLER,
    )
    .unwrap();
    let default_stamp_dir = temp.path().join(format!(
        ".agent/.cache/jig/contract-{}",
        crate::context::CURRENT_CONTRACT_VERSION
    ));
    fs::create_dir_all(&default_stamp_dir).unwrap();
    fs::write(
        default_stamp_dir.join(".jig-source-stamp"),
        "jig-seeded-runtime-v1\nbinary:sha256:fixture\nsource:sha256:fixture\n",
    )
    .unwrap();
    let managed_manifest = temp
        .path()
        .join(crate::bootstrap::MANAGED_PATHS_MANIFEST_PATH);
    fs::create_dir_all(managed_manifest.parent().unwrap()).unwrap();
    fs::write(&managed_manifest, "{}\n").unwrap();

    let output = runtime_check(
        temp.path(),
        Some(crate::context::CURRENT_CONTRACT_VERSION),
        None,
        true,
    );

    assert!(output.ok, "{}", output.detail);
    assert!(output.fix.is_none());
    let advisory =
        launcher_repair_cache_check(temp.path(), crate::context::CURRENT_CONTRACT_VERSION);
    let fix = advisory
        .fix
        .as_deref()
        .expect("current repair seed needs a fix");
    assert!(fix.contains(" update "));
    assert!(fix.contains("--force"));
    assert_eq!(output.data["launcher_repair_seeded_cache"], true);
}

#[test]
fn doctor_reports_preserved_launcher_repair_staging_for_manual_recovery() {
    let temp = tempdir().unwrap();
    let cache_base = temp.path().join(".agent/.cache/jig");
    let staging = cache_base.join(".jig-launcher-repair-preserved");
    fs::create_dir_all(staging.join("backup-runtime")).unwrap();

    assert!(
        launcher_repair_staging_check(temp.path()).is_none(),
        "doctor must not flag an in-flight launcher repair"
    );
    let check = launcher_repair_staging_check_at(
        temp.path(),
        SystemTime::now() + LAUNCHER_REPAIR_STAGING_DOCTOR_MIN_AGE + Duration::from_secs(1),
    )
    .expect("staging advisory");

    assert!(!check.required);
    assert!(!check.ok);
    assert_eq!(check.status, "recovery artifacts");
    assert!(check.detail.contains(&staging.display().to_string()));
    assert_eq!(check.data["paths"][0], staging.display().to_string());
    assert!(check.fix.as_deref().unwrap().contains("backup-*"));

    fs::remove_dir_all(staging).unwrap();
    assert!(launcher_repair_staging_check(temp.path()).is_none());
}

#[test]
fn doctor_reports_only_version_shaped_legacy_runtime_caches() {
    let temp = tempdir().unwrap();
    let cache_base = temp.path().join(".agent/.cache/jig");
    for name in [
        "0.2.0",
        "0.2.0-beta.1-runtime",
        "1.2.3",
        "contract-4",
        "other-cache",
    ] {
        fs::create_dir_all(cache_base.join(name)).unwrap();
    }
    for name in ["0.2.0", "0.2.0-beta.1-runtime"] {
        fs::create_dir_all(cache_base.join(name).join("bin")).unwrap();
        fs::write(cache_base.join(name).join("bin/jig"), "fixture").unwrap();
    }

    let check = legacy_version_cache_check(temp.path()).expect("legacy cache advisory");

    assert!(!check.required);
    assert!(!check.ok);
    assert_eq!(check.status, "cleanup available");
    assert_eq!(check.data["paths"].as_array().unwrap().len(), 2);
    assert!(check.detail.contains("0.2.0"));
    assert!(!check.detail.contains("1.2.3"));
    assert!(!check.detail.contains("contract-4"));
    assert!(!check.detail.contains("other-cache"));
    assert!(
        check
            .fix
            .as_deref()
            .unwrap()
            .contains("full harness update")
    );
}

#[test]
fn installer_cache_layout_marker_matches_doctor_probe_constants() {
    assert_eq!(
        INSTALLER_CACHE_LAYOUT_MARKER,
        format!(
            "git={GIT_RUNTIME_CACHE_BASE};fallback={FALLBACK_RUNTIME_CACHE_BASE};runtime-suffix={RUNTIME_CACHE_PROFILE_SUFFIX}"
        )
    );
}

#[test]
fn runtime_check_rejects_contract_scripts_without_repository_epoch_enforcement() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join("scripts")).unwrap();
    let launcher = current_generated_launcher()
        .replace("# jig-runtime-repository-scope:v1", "# stale-scope-marker");
    let installer = CURRENT_GENERATED_INSTALLER
        .replace("# jig-runtime-repository-scope:v1", "# stale-scope-marker");
    fs::write(temp.path().join("scripts/jig"), launcher).unwrap();
    fs::write(temp.path().join("scripts/install-jig.sh"), installer).unwrap();

    let output = runtime_check(
        temp.path(),
        Some(crate::context::CURRENT_CONTRACT_VERSION),
        None,
        true,
    );

    assert!(!output.ok);
    assert_eq!(output.status, "outdated");
    assert_eq!(output.data["launcher_uses_contract_probe"], false);
    assert_eq!(output.data["installer_uses_contract_probe"], false);
    assert!(output.fix.as_deref().unwrap().contains("--launcher-only"));
}

#[test]
fn runtime_check_does_not_recommend_launcher_repair_until_config_is_readable() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join("scripts")).unwrap();
    fs::write(
        temp.path().join("scripts/jig"),
        current_generated_launcher()
            .replace("# jig-runtime-repository-scope:v1", "# stale-scope-marker"),
    )
    .unwrap();
    fs::write(
        temp.path().join("scripts/install-jig.sh"),
        CURRENT_GENERATED_INSTALLER,
    )
    .unwrap();

    let output = runtime_check(
        temp.path(),
        Some(crate::context::CURRENT_CONTRACT_VERSION),
        None,
        false,
    );

    let fix = output.fix.as_deref().unwrap();
    assert!(fix.contains("Repair `.jig.toml`"), "{fix}");
    assert!(!fix.contains("--launcher-only"), "{fix}");
    assert_eq!(output.data["config_valid_for_launcher_repair"], false);
}

#[test]
fn runtime_check_rejects_unrecognizable_generated_installer() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join("scripts")).unwrap();
    fs::write(
        temp.path().join("scripts/jig"),
        current_generated_launcher(),
    )
    .unwrap();
    fs::write(
        temp.path().join("scripts/install-jig.sh"),
        "#!/usr/bin/env bash\nexit 0\n",
    )
    .unwrap();
    fs::create_dir_all(temp.path().join(".agent")).unwrap();
    fs::write(
        temp.path()
            .join(crate::bootstrap::MANAGED_PATHS_MANIFEST_PATH),
        "{}",
    )
    .unwrap();

    let output = runtime_check(
        temp.path(),
        Some(crate::context::CURRENT_CONTRACT_VERSION),
        None,
        true,
    );

    assert!(!output.ok);
    assert_eq!(output.status, "outdated");
    assert!(
        output.detail.contains("not a recognizable"),
        "{}",
        output.detail
    );
    assert_eq!(output.data["installer_present"], true);
    assert_eq!(output.data["installer_uses_contract_probe"], false);
    assert!(output.fix.as_deref().unwrap().contains("--launcher-only"));
}

#[test]
fn legacy_contract_migration_is_an_optional_actionable_doctor_issue() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".agent")).unwrap();
    fs::write(
        temp.path()
            .join(crate::bootstrap::MANAGED_PATHS_MANIFEST_PATH),
        "{}",
    )
    .unwrap();
    let migration = contract_migration_check(temp.path(), 3);

    assert!(!migration.required);
    assert!(!migration.ok);
    assert_eq!(migration.status, "migration available");
    assert!(migration.detail.contains("remains supported"));
    let fix = migration.fix.as_deref().unwrap();
    assert!(fix.contains(" update "));
    assert!(fix.contains("--force"));
    assert!(!fix.contains("--launcher-only"));

    let report = output(None, vec![migration]);
    assert_eq!(report["ok"], true);
    assert_eq!(report["next_issue"]["id"], "contract_migration");
    assert_eq!(report["optional_setup"], report["next_step"]);
}

#[test]
fn legacy_contract_migration_requires_adoption_when_ownership_is_missing() {
    let temp = tempdir().unwrap();
    let migration = contract_migration_check(temp.path(), 3);

    let fix = migration.fix.as_deref().unwrap();
    assert!(fix.contains(" adopt "));
    assert!(fix.contains("--write --force"));
    assert!(!fix.contains(" update "));
    assert_eq!(
        migration.data["managed_paths_manifest_present"],
        serde_json::Value::Bool(false)
    );
}

#[test]
fn runtime_check_rejects_launcher_contract_epoch_drift() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join("scripts")).unwrap();
    let launcher = generated_launcher_with_contract(3);
    fs::write(temp.path().join("scripts/jig"), launcher).unwrap();
    fs::write(
        temp.path().join("scripts/install-jig.sh"),
        CURRENT_GENERATED_INSTALLER,
    )
    .unwrap();

    let output = runtime_check(temp.path(), Some(4), None, true);

    assert!(!output.ok);
    assert_eq!(output.status, "outdated");
    assert!(output.detail.contains("embeds contract 3"));
    assert_eq!(output.data["contract_version"], 4);
    assert_eq!(output.data["launcher_contract_version"], 3);
}

#[test]
fn missing_launcher_and_ownership_manifest_recommends_adoption() {
    let temp = tempdir().unwrap();

    let output = runtime_check(temp.path(), Some(4), None, true);

    assert!(!output.ok);
    assert_eq!(output.status, "missing");
    let fix = output.fix.as_deref().unwrap();
    assert!(fix.contains(" adopt "));
    assert!(fix.contains("--write --force"));
    assert!(!fix.contains("--launcher-only"));
}

#[test]
fn damaged_contract_manifest_recommends_full_update_not_launcher_repair() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join("scripts")).unwrap();
    fs::write(
        temp.path().join("scripts/jig"),
        current_generated_launcher(),
    )
    .unwrap();
    fs::write(
        temp.path().join("scripts/install-jig.sh"),
        CURRENT_GENERATED_INSTALLER,
    )
    .unwrap();
    fs::create_dir_all(temp.path().join(".agent")).unwrap();
    fs::write(
        temp.path()
            .join(crate::bootstrap::MANAGED_PATHS_MANIFEST_PATH),
        "{}",
    )
    .unwrap();

    let output = runtime_check(temp.path(), None, None, true);

    assert!(!output.ok);
    assert_eq!(output.status, "unreadable");
    assert!(output.detail.contains("contract version is unreadable"));
    let fix = output.fix.as_deref().unwrap();
    assert!(fix.contains(" update "));
    assert!(fix.contains("--force"));
    assert!(!fix.contains("--launcher-only"));
}

#[test]
fn unsupported_newer_contract_does_not_recommend_downgrade_repair() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join("scripts")).unwrap();
    fs::write(
        temp.path().join("scripts/jig"),
        generated_launcher_with_contract(99),
    )
    .unwrap();
    fs::write(
        temp.path().join("scripts/install-jig.sh"),
        CURRENT_GENERATED_INSTALLER,
    )
    .unwrap();

    let output = runtime_check(temp.path(), Some(99), None, true);

    assert!(!output.ok);
    assert_eq!(output.status, "unsupported");
    assert!(
        output
            .detail
            .contains("does not support repository contract 99")
    );
    assert_eq!(output.data["launcher_contract_version"], 99);
    let fix = output.fix.as_deref().unwrap();
    assert!(fix.contains("does not support"));
    assert!(fix.contains("newer compatible Jig"));
    assert!(!fix.contains(" update "));
    assert!(!fix.contains(" adopt "));
}
