use super::*;

#[test]
fn ignored_guide_directories_are_diagnosed_and_their_children_discovered() {
    let root = fixture("# Example ownership\n");
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(root.path())
            .status()
            .unwrap()
            .success()
    );
    fs::write(root.path().join(".gitignore"), "AGENTS.md\n").unwrap();
    fs::remove_file(root.path().join("AGENTS.md")).unwrap();
    let directories = ["AGENTS.md", "src/AGENTS.md"];
    for directory in directories {
        fs::create_dir_all(root.path().join(directory)).unwrap();
    }

    let output = run(root.path(), &["check", "agent-guides", "--json"]);
    assert!(!output.status.success(), "{output:?}");
    let report = parse(&output);
    assert_eq!(report["ok"], false);
    assert_eq!(report["guide_count"], directories.len());
    let diagnostics = report["diagnostics"].as_array().unwrap();
    assert_eq!(diagnostics.len(), directories.len());
    for directory in directories {
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic["guide"] == directory
                && diagnostic["code"] == "guide_unreadable"
                && diagnostic["severity"] == "error"
        }));
        let child = root.path().join(directory).join("area");
        fs::create_dir(&child).unwrap();
        fs::write(child.join("AGENTS.md"), "[Broken](missing.md)\n").unwrap();
    }

    let output = run(root.path(), &["check", "agent-guides", "--json"]);
    assert!(!output.status.success(), "{output:?}");
    let report = parse(&output);
    assert_eq!(report["ok"], false);
    assert_eq!(report["guide_count"], directories.len() * 2);
    let diagnostics = report["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|diagnostic| diagnostic["severity"] == "error")
        .collect::<Vec<_>>();
    assert_eq!(diagnostics.len(), directories.len() * 2);
    for directory in directories {
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic["guide"] == format!("{directory}/area/AGENTS.md")
                && diagnostic["code"] == "reference_missing"
                && diagnostic["severity"] == "error"
        }));
    }
}

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
