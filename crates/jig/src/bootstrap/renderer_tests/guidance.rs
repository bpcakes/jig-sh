use super::*;

#[test]
fn rendered_guidance_matches_the_contracts_guide_policy() {
    for version in [5, 8, 9] {
        let destination = tempfile::tempdir().unwrap();
        let answers = rust_render_answers(RepositoryProjectionHint::Backend);
        render_template_files(
            &live_template_source(),
            &answers,
            destination.path(),
            Some(&BTreeSet::from([PathBuf::from("AGENTS.md")])),
            Some(version),
        )
        .unwrap();
        let guide = fs::read_to_string(destination.path().join("AGENTS.md")).unwrap();
        assert_eq!(
            guide.contains("these sections are optional suggestions"),
            version >= 9
        );
        assert_eq!(guide.contains("use these sections:"), version < 9);
        assert_eq!(
            guide.contains("explicitly declared component guidance"),
            version >= 9
        );
        for retired in ["jig work", "jig mcp", ".agent/PLANS.md"] {
            assert!(
                !guide.contains(retired),
                "v{version} reintroduced {retired}"
            );
        }
    }
}
