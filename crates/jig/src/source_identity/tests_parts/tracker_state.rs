use super::*;

/// Contract 12 declares tracker ownership in `[repository]`; earlier
/// contracts use `[work]` receipt metadata.
const OPT_INS: [&str; 2] = [
    "[repository]\ntracker = \"beads\"\n",
    "[work]\nreceipt_metadata = [\"beads\"]\n",
];

fn fixture(root: &Path, config: &str) {
    run_git(root, &["init", "-q"]);
    run_git(root, &["config", "user.name", "ExampleMaintainer"]);
    run_git(root, &["config", "user.email", "example@example.invalid"]);
    fs::create_dir_all(root.join(".beads")).unwrap();
    fs::write(root.join(".beads/issues.jsonl"), "ExampleTask\n").unwrap();
    fs::write(root.join("source.rs"), "fn example() {}\n").unwrap();
    fs::write(root.join("guide.md"), "Example guide\n").unwrap();
    fs::write(root.join(".jig.toml"), config).unwrap();
    run_git(root, &["add", "."]);
    run_git(root, &["commit", "-qm", "Example baseline"]);
}

fn fingerprint(root: &Path) -> String {
    repository_source_snapshot(root)
        .unwrap()
        .worktree_fingerprint
}

#[test]
fn tracker_metadata_is_source_by_default() {
    for config in ["[work]\n", "[repository]\n"] {
        let root = tempdir().unwrap();
        fixture(root.path(), config);
        let before = fingerprint(root.path());
        fs::write(root.path().join(".beads/issues.jsonl"), "ExampleClosed\n").unwrap();
        assert_ne!(before, fingerprint(root.path()), "{config}");
    }
}

#[test]
fn opt_in_excludes_only_root_tracker_across_dirty_staged_and_committed_states() {
    for config in OPT_INS {
        let root = tempdir().unwrap();
        let root = root.path();
        fixture(root, config);
        let before = fingerprint(root);
        fs::write(root.join(".beads/issues.jsonl"), "ExampleClosed\n").unwrap();
        fs::write(root.join(".beads/new.jsonl"), "ExampleNew\n").unwrap();
        assert_eq!(before, fingerprint(root), "{config}");
        run_git(root, &["add", ".beads"]);
        assert_eq!(before, fingerprint(root), "{config}");
        run_git(root, &["commit", "-qm", "Example tracker update"]);
        assert_eq!(before, fingerprint(root), "{config}");
        fs::create_dir_all(root.join("fixtures/.beads")).unwrap();
        fs::write(
            root.join("fixtures/.beads/issues.jsonl"),
            "ExampleFixture\n",
        )
        .unwrap();
        assert_ne!(before, fingerprint(root), "{config}");
    }
}

#[test]
fn opt_in_keeps_source_documentation_and_configuration_authoritative() {
    for (name, config) in ["source.rs", "guide.md", ".jig.toml"]
        .into_iter()
        .flat_map(|name| OPT_INS.map(|config| (name, config)))
    {
        let root = tempdir().unwrap();
        fixture(root.path(), config);
        let before = fingerprint(root.path());
        let path = root.path().join(name);
        let original = fs::read_to_string(&path).unwrap();
        fs::write(&path, original + "\n# ExampleChange\n").unwrap();
        assert_ne!(before, fingerprint(root.path()), "{name}");
        run_git(root.path(), &["add", name]);
        run_git(root.path(), &["commit", "-qm", "Example source update"]);
        assert_ne!(before, fingerprint(root.path()), "committed {name}");
    }
}

#[test]
fn changing_opt_in_invalidates_prior_identity_and_arbitrary_exclusions_fail() {
    for config in OPT_INS {
        let root = tempdir().unwrap();
        fixture(root.path(), "");
        let before = fingerprint(root.path());
        fs::write(root.path().join(".jig.toml"), config).unwrap();
        assert_ne!(before, fingerprint(root.path()), "{config}");
    }
    let root = tempdir().unwrap();
    fixture(root.path(), "");
    for value in ["docs", "**", "../outside", "bead"] {
        for config in [
            format!("[work]\nreceipt_metadata = [\"{value}\"]\n"),
            format!("[repository]\ntracker = \"{value}\"\n"),
        ] {
            fs::write(root.path().join(".jig.toml"), &config).unwrap();
            assert!(repository_source_snapshot(root.path()).is_err(), "{config}");
        }
    }
}
