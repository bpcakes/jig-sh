fn assert_generated_rust_clippy_defaults(destination: &Path) {
    let root_manifest_path = destination.join("Cargo.toml");
    let root_manifest = toml::from_str::<toml::Value>(
        &fs::read_to_string(&root_manifest_path).unwrap(),
    )
    .unwrap();
    assert_eq!(
        root_manifest["workspace"]["lints"]["clippy"]["cognitive_complexity"].as_str(),
        Some("warn")
    );
    assert_eq!(
        fs::read_to_string(destination.join("clippy.toml")).unwrap(),
        "cognitive-complexity-threshold = 20\n"
    );
    let readme = fs::read_to_string(destination.join("README.md")).unwrap();
    assert!(readme.contains("`cognitive_complexity` restriction lint"));
    assert!(readme.contains("treats all warnings as failures"));
    assert!(readme.contains("Clippy configuration is project-owned after init"));

    for member in root_manifest["workspace"]["members"].as_array().unwrap() {
        let member = member.as_str().unwrap();
        let member_manifest_path = destination.join(member).join("Cargo.toml");
        let member_manifest = toml::from_str::<toml::Value>(
            &fs::read_to_string(&member_manifest_path).unwrap(),
        )
        .unwrap();
        assert_eq!(
            member_manifest["lints"]["workspace"].as_bool(),
            Some(true),
            "{} does not inherit workspace lints",
            member_manifest_path.display()
        );
    }
}
