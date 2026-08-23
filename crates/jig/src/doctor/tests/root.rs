use super::*;

#[test]
fn doctor_reports_when_configured_root_differs_from_invocation_repo() {
    let _env = lock_env();
    let temp = tempdir().unwrap();
    let selected = temp.path().join("selected");
    let invocation = temp.path().join("invocation");
    fs::create_dir_all(&selected).unwrap();
    fs::create_dir_all(&invocation).unwrap();
    write_doctor_fixture(&selected);
    write_doctor_fixture(&invocation);
    let _repo_root = EnvVarGuard::set("JIG_REPO_ROOT", &selected);

    let notice = doctor_root_override_notice(&invocation, &fs::canonicalize(&selected).unwrap())
        .expect("differing configured and invocation roots must be reported");

    assert!(notice.contains("doctor is using JIG_REPO_ROOT="));
    assert!(notice.contains(&fs::canonicalize(&selected).unwrap().display().to_string()));
    assert!(notice.contains(&fs::canonicalize(&invocation).unwrap().display().to_string()));
    assert!(notice.contains("unset JIG_REPO_ROOT"));
}
