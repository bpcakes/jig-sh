use super::*;

#[test]
fn ignored_root_nested_and_directory_guides_are_validated() {
    let root = fixture("[Broken](missing.md)\n");
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(root.path())
            .status()
            .unwrap()
            .success()
    );
    fs::write(root.path().join(".gitignore"), "AGENTS.md\nignored/\n").unwrap();
    let guides = ["AGENTS.md", "src/AGENTS.md", "ignored/area/AGENTS.md"];
    for guide in guides {
        let path = root.path().join(guide);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "[Broken](missing.md)\n").unwrap();
        assert!(
            Command::new("git")
                .args(["check-ignore", "--quiet", guide])
                .current_dir(root.path())
                .status()
                .unwrap()
                .success(),
            "fixture guide should be ignored: {guide}"
        );
    }
    for excluded in [".git", "target", "src/.git", "src/target"] {
        let directory = root.path().join(excluded);
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            directory.join("AGENTS.md"),
            "[Broken](excluded-missing.md)\n",
        )
        .unwrap();
    }

    let broken = run(root.path(), &["check", "agent-guides", "--json"]);
    assert!(!broken.status.success(), "{broken:?}");
    let report = parse(&broken);
    assert_eq!(report["ok"], false);
    assert_eq!(report["guide_count"], guides.len());
    let errors = report["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|diagnostic| diagnostic["severity"] == "error")
        .collect::<Vec<_>>();
    assert_eq!(errors.len(), guides.len());
    for guide in guides {
        assert!(errors.iter().any(|diagnostic| {
            diagnostic["guide"] == guide && diagnostic["code"] == "reference_missing"
        }));
        fs::write(root.path().join(guide), "# Example ownership\n").unwrap();
    }

    let fixed = run(root.path(), &["check", "agent-guides", "--json"]);
    assert!(fixed.status.success(), "{fixed:?}");
    let report = parse(&fixed);
    assert_eq!(report["ok"], true);
    assert_eq!(report["guide_count"], guides.len());
    assert!(
        report["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .all(|diagnostic| {
                diagnostic["severity"] == "warning" && diagnostic["code"] == "guide_structure"
            })
    );
}
