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

#[test]
fn rendered_guidance_keeps_secret_access_operator_owned() {
    for version in [5, 8, 9] {
        for projection in [
            RepositoryProjectionHint::Backend,
            RepositoryProjectionHint::RustWorkspace,
        ] {
            let destination = tempfile::tempdir().unwrap();
            render_template_files(
                &live_template_source(),
                &rust_render_answers(projection),
                destination.path(),
                Some(&BTreeSet::from([PathBuf::from("AGENTS.md")])),
                Some(version),
            )
            .unwrap();
            let guide = fs::read_to_string(destination.path().join("AGENTS.md")).unwrap();
            let case = format!("v{version} {projection:?}");

            assert!(
                guide.contains(
                    "when Jig Codex skills are missing, except operator-owned vault setup (see Vault)."
                ),
                "{case}: Start Here must except vault setup"
            );
            let start = guide.find("\n\n## Vault\n\n- ").expect(&case);
            let compatibility = guide.find("## Compatibility And Cutovers").expect(&case);
            let section = &guide[start + 2..];
            let end = section.find("\n\n## ").expect(&case);
            let next_heading = &section[end + 2..];
            let section = &section[..end];
            assert!(compatibility < start, "{case}: Vault moved");
            assert!(
                next_heading.starts_with("## Rust Defaults\n")
                    || next_heading.starts_with("## Backend Defaults\n"),
                "{case}: Vault must precede the defaults heading"
            );
            assert!(
                guide.find("<!-- END JIG MANAGED BLOCK -->").expect(&case) > start,
                "{case}: Vault must stay in the managed block"
            );
            for expected in [
                "operator-owned",
                "override any Jig next step",
                "Run only `scripts/jig vault status`",
                "when the operator has already provided `JIG_VAULT_PASSPHRASE` to the session",
                "`scripts/jig vault exec --env-file REFS_FILE -- COMMAND`",
                "`scripts/jig vault run`",
                "Every other vault subcommand is operator-only.",
                "`COMMAND` is the underlying task command itself.",
                "Never wrap `scripts/jig check`, `scripts/jig run`, or another Jig runner in `vault exec` or `vault run`",
                "`.agent/state/runs.jsonl` outside vault redaction",
                "Never pass `--home` or `--global`",
                "never set `JIG_VAULT_HOME`",
                "Never delete, move, or edit the vault rollback witness",
                "Never request, print, inspect, test, choose, store, or set the passphrase",
                "Never create or edit refs files or add references.",
                "Never wrap commands that print, encode, or transmit injected values",
                "`.env.local`",
                "stop and ask the operator",
            ] {
                assert!(section.contains(expected), "{case}: missing {expected}");
            }
            // v9 agent-guides validates local links; keep placeholders plain.
            assert!(!section.contains("]("), "{case}: unexpected link");
            assert!(!section.contains('<'), "{case}: unexpected angle bracket");
        }
    }
}
