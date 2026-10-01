use super::*;

#[test]
fn regenerating_legacy_maps_repairs_literal_percent_paths_without_changing_epoch() {
    for epoch in 2..=9 {
        let root = legacy_fixture(epoch);
        for name in ["100%", "example%20guide"] {
            fs::create_dir(root.path().join(name)).unwrap();
            fs::write(root.path().join(name).join("AGENTS.md"), "# Owner\n").unwrap();
        }
        // Exact destination spelling emitted by the previous map renderer.
        fs::write(
            root.path().join("agent-map.md"),
            "[Root](./AGENTS.md)\n[Percent](./100%/AGENTS.md)\n[Encoded-looking name](./example%20guide/AGENTS.md)\n",
        )
        .unwrap();
        let manifest_path = root.path().join(".agent/jig-contract.json");
        let manifest_before = fs::read(&manifest_path).unwrap();

        let old = run(root.path(), &["check", "agent-map", "--json"]);
        assert!(!old.status.success(), "epoch {epoch}: {old:?}");
        let report = parse(&old);
        assert_eq!(report["ok"], false);
        assert_eq!(
            report["missing_agents"],
            json!(["100%/AGENTS.md", "example%20guide/AGENTS.md"])
        );
        assert!(
            report["broken_links"]
                .as_array()
                .unwrap()
                .iter()
                .any(|link| { link.as_str().unwrap().contains("malformed percent escape") })
        );

        let generated = run(root.path(), &["agent-map", "generate", "--json"]);
        assert!(generated.status.success(), "epoch {epoch}: {generated:?}");
        let map = fs::read_to_string(root.path().join("agent-map.md")).unwrap();
        assert!(map.contains("./100%25/AGENTS.md"));
        assert!(map.contains("./example%2520guide/AGENTS.md"));
        let checked = run(root.path(), &["check", "agent-map", "--json"]);
        assert!(checked.status.success(), "epoch {epoch}: {checked:?}");
        assert_eq!(parse(&checked)["ok"], true);
        assert_eq!(parse(&checked)["agents"], 3);
        assert_eq!(fs::read(manifest_path).unwrap(), manifest_before);
    }
}
