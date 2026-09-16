use super::*;

const WORKSPACE_ID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

fn configured_repo(root: &Path) {
    TestRepoBuilder::new(root)
        .config(format!(
            "[work.tracker]\nkind = \"beads\"\nworkspace_id = \"{WORKSPACE_ID}\"\n"
        ))
        .write();
    fs::create_dir(root.join(".beads")).unwrap();
    fs::write(root.join(".beads/beads.db"), b"database fixture").unwrap();
    fs::write(root.join(".beads/issues.jsonl"), b"jsonl fixture\n").unwrap();
}

#[test]
fn unconfigured_tracker_does_not_probe_path_or_existing_store() {
    let _env = lock_env();
    let temp = tempdir().unwrap();
    let bin = temp.path().join("bin");
    fs::create_dir(&bin).unwrap();
    let marker = temp.path().join("called");
    write_test_executable(
        &bin.join("br"),
        &format!("#!/bin/sh\nprintf called > {}\nexit 99\n", marker.display()),
    );
    TestRepoBuilder::new(temp.path()).write();
    fs::create_dir(temp.path().join(".beads")).unwrap();
    let _br = crate::tracker::TestBrOverride::set(Some(&bin.join("br")));
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let result = super::super::tracker::tracker_check(
        &ctx,
        DoctorProcessControl::allowed_without_signal_session(),
    );

    assert!(result.ok);
    assert!(!result.required);
    assert_eq!(result.status, "not configured");
    assert!(!marker.exists());
}

#[test]
fn configured_tracker_reports_a_missing_external_binary() {
    let _env = lock_env();
    let temp = tempdir().unwrap();
    let root = temp.path().join("repo");
    fs::create_dir(&root).unwrap();
    configured_repo(&root);
    let _br = crate::tracker::TestBrOverride::set(None);
    let ctx = RepoContext::load_from(&root).unwrap();

    let result = super::super::tracker::tracker_check(
        &ctx,
        DoctorProcessControl::allowed_without_signal_session(),
    );

    assert!(!result.ok);
    assert!(result.required);
    assert_eq!(result.status, "missing");
    assert_eq!(result.data["configured"], true);
    assert_eq!(result.data["root"], ".beads");
    assert!(result.fix.unwrap().contains("Install supported `br 0.5.7`"));
    assert!(!result.detail.contains(root.to_string_lossy().as_ref()));
}

#[test]
fn transient_tracker_failures_do_not_prescribe_installation_or_store_repair() {
    for error in [
        crate::tracker::TrackerError::TimedOut {
            operation: crate::tracker::TrackerOperation::Version,
        },
        crate::tracker::TrackerError::CancelledBeforeStart {
            operation: crate::tracker::TrackerOperation::Version,
        },
    ] {
        let result = super::super::tracker::tracker_error(error, None);

        assert!(!result.ok);
        assert!(result.fix.is_none());
    }
}

#[test]
fn hard_linked_tracker_authority_has_specific_recovery() {
    let result = super::super::tracker::tracker_error(
        crate::tracker::TrackerError::InvalidWorkspace {
            reason: crate::tracker::InvalidWorkspaceReason::HardLinkedAuthority,
        },
        None,
    );

    assert_eq!(result.status, "invalid workspace");
    assert!(result.detail.contains("multiple hard links"));
    assert!(
        result
            .fix
            .unwrap()
            .contains("hard-linked tracker authority")
    );
}

#[test]
fn unsupported_tracker_platform_has_platform_specific_recovery() {
    let result = super::super::tracker::tracker_error(
        crate::tracker::TrackerError::UnsupportedPlatform,
        None,
    );

    assert!(!result.ok);
    assert!(result.required);
    assert_eq!(result.status, "unsupported platform");
    assert!(result.fix.unwrap().contains("Linux or macOS"));
}

#[test]
fn unsafe_tracker_temporary_directory_has_specific_recovery() {
    let result = super::super::tracker::tracker_error(
        crate::tracker::TrackerError::UnsafeTemporaryDirectory {
            operation: crate::tracker::TrackerOperation::SyncStatus,
        },
        None,
    );

    assert!(!result.ok);
    assert!(result.required);
    assert_eq!(result.status, "unsafe temporary directory");
    let fix = result.fix.unwrap();
    assert!(fix.contains("`TMPDIR`"));
    assert!(fix.contains("outside the repository"));
}

#[test]
fn configured_tracker_reports_supported_read_only_profile() {
    let _env = lock_env();
    let temp = tempdir().unwrap();
    let root = temp.path().join("repo");
    let bin = temp.path().join("bin");
    fs::create_dir(&root).unwrap();
    fs::create_dir(&bin).unwrap();
    let root = root.canonicalize().unwrap();
    let bin = bin.canonicalize().unwrap();
    configured_repo(&root);
    let log = temp.path().join("calls.log");
    let beads_dir = serde_json::to_string(&root.join(".beads")).unwrap();
    let database = serde_json::to_string(&root.join(".beads/beads.db")).unwrap();
    let jsonl = serde_json::to_string(&root.join(".beads/issues.jsonl")).unwrap();
    write_test_executable(
        &bin.join("br"),
        &format!(
            r#"#!/bin/sh
printf '%s\n' "$*" >> {log}
case " $* " in
  *" version "*) [ "${{BD_NO_DB-}}" = true ] || exit 92; printf '%s' '{{"version":"0.5.7"}}' ;;
  *" --no-db where "*) [ "${{BD_NO_DB-}}" = true ] || exit 92; printf '%s' '{{"path":{beads_dir},"database_path":{database},"jsonl_path":{jsonl}}}' ;;
  *" sync --allow-external-jsonl --status "*) [ "${{BD_NO_DB-}}" = false ] || exit 92; printf '%s' '{{"jsonl_newer":false,"db_newer":false,"coverage_drift":false,"workspace_health":"healthy","reliability_audit":{{"source":"sync.status","health":"healthy","anomaly_count":0,"anomalies":[]}}}}' ;;
  *) exit 91 ;;
esac
"#,
            log = log.display(),
        ),
    );
    let _br = crate::tracker::TestBrOverride::set(Some(&bin.join("br")));
    let ctx = RepoContext::load_from(&root).unwrap();
    let database_before = fs::read(root.join(".beads/beads.db")).unwrap();
    let database_modified_before = fs::metadata(root.join(".beads/beads.db"))
        .unwrap()
        .modified()
        .unwrap();
    let jsonl_before = fs::read(root.join(".beads/issues.jsonl")).unwrap();
    let jsonl_modified_before = fs::metadata(root.join(".beads/issues.jsonl"))
        .unwrap()
        .modified()
        .unwrap();

    let result = super::super::tracker::tracker_check(
        &ctx,
        DoctorProcessControl::allowed_without_signal_session(),
    );

    assert!(result.ok, "{}", result.detail);
    assert!(result.required);
    assert_eq!(result.status, "ready");
    assert_eq!(result.data["version"], "0.5.7");
    assert_eq!(result.data["profile"], "beads_0_5_7");
    assert_eq!(
        result.data["supported_operations"]
            .as_array()
            .unwrap()
            .len(),
        5
    );
    let calls = fs::read_to_string(log).unwrap();
    assert!(
        calls
            .lines()
            .all(|line| { line.contains("--no-auto-import") && line.contains("--no-auto-flush") })
    );
    assert!(calls.contains(" version"));
    assert!(calls.contains(" --no-db where"));
    assert!(calls.contains(" sync --allow-external-jsonl --status"));
    assert!(!calls.contains(" show "));
    assert!(!calls.contains(" comments "));
    assert!(!calls.contains(" update "));
    assert!(!calls.contains(" close "));
    let sync_call = calls.lines().find(|line| line.contains(" sync ")).unwrap();
    assert!(!sync_call.contains(root.to_string_lossy().as_ref()));
    assert_eq!(
        fs::read(root.join(".beads/beads.db")).unwrap(),
        database_before
    );
    assert_eq!(
        fs::metadata(root.join(".beads/beads.db"))
            .unwrap()
            .modified()
            .unwrap(),
        database_modified_before
    );
    assert_eq!(
        fs::read(root.join(".beads/issues.jsonl")).unwrap(),
        jsonl_before
    );
    assert_eq!(
        fs::metadata(root.join(".beads/issues.jsonl"))
            .unwrap()
            .modified()
            .unwrap(),
        jsonl_modified_before
    );
}

#[test]
fn configured_unknown_profile_stops_before_workspace_or_issue_operations() {
    let _env = lock_env();
    let temp = tempdir().unwrap();
    let root = temp.path().join("repo");
    let bin = temp.path().join("bin");
    fs::create_dir(&root).unwrap();
    fs::create_dir(&bin).unwrap();
    configured_repo(&root);
    let log = temp.path().join("calls.log");
    write_test_executable(
        &bin.join("br"),
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> {}\nprintf '%s' '{{\"version\":\"0.6.0\"}}'\n",
            log.display()
        ),
    );
    let _br = crate::tracker::TestBrOverride::set(Some(&bin.join("br")));
    let ctx = RepoContext::load_from(&root).unwrap();

    let result = super::super::tracker::tracker_check(
        &ctx,
        DoctorProcessControl::allowed_without_signal_session(),
    );

    assert!(!result.ok);
    assert!(result.required);
    assert_eq!(result.status, "unsupported");
    assert_eq!(result.data["version"], "0.6.0");
    assert_eq!(fs::read_to_string(log).unwrap().lines().count(), 1);
}
