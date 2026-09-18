use super::*;

#[test]
fn tracker_guidance_does_not_change_contract_authority() {
    let temp = tempdir().unwrap();
    let tracker_config =
        "[work.tracker]\nkind = \"beads\"\nworkspace_id = \"01ARZ3NDEKTSV4RRFFQ69G5FAV\"\n";
    TestRepoBuilder::new(temp.path())
        .config(tracker_config)
        .write();
    let config_path = temp.path().join(".jig.toml");
    let source = fs::read_to_string(&config_path).unwrap();
    let original = RepoContext::load_from(temp.path()).unwrap();

    for guidance in [
        "Run scripts/export-beads.",
        "Run the repository export helper.",
    ] {
        fs::write(
            &config_path,
            format!("{source}\nmanual_export_guidance = {guidance:?}\n"),
        )
        .unwrap();
        let changed = original.reload_execution_authority().unwrap();
        assert_eq!(changed.contract_digest(), original.contract_digest());
        assert_eq!(
            changed.work_tracker().unwrap().manual_export_guidance(),
            Some(guidance)
        );
    }

    fs::write(
        &config_path,
        source.replace("01ARZ3NDEKTSV4RRFFQ69G5FAV", "01ARZ3NDEKTSV4RRFFQ69G5FAW"),
    )
    .unwrap();
    let changed = original.reload_execution_authority().unwrap();
    assert_ne!(changed.contract_digest(), original.contract_digest());

    // Manual is the only supported policy; spelling out its default is inert.
    fs::write(&config_path, format!("{source}\nexport = \"manual\"\n")).unwrap();
    let explicit_manual = original.reload_execution_authority().unwrap();
    assert_eq!(
        explicit_manual.contract_digest(),
        original.contract_digest()
    );
}
