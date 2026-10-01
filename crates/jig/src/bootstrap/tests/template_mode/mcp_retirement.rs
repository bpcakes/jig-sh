use super::*;

const GENERATED: &str = r#"{"mcpServers":{"jig":{"command":"./scripts/jig","args":["mcp"]}}}"#;

fn legacy_registration(repo: &Path, contents: &str, owned: bool) {
    fs::write(repo.join(".mcp.json"), contents).unwrap();
    if owned {
        let mut paths = managed_paths::load_manifest(repo).unwrap().unwrap();
        paths.insert(PathBuf::from(".mcp.json"));
        managed_paths::write_manifest(repo, &paths).unwrap();
    }
}

fn refresh(
    repo: &Path,
    template: &Path,
    adopt: bool,
    force: bool,
) -> anyhow::Result<serde_json::Value> {
    if adopt {
        run_adopt(AdoptOpts {
            components: Default::default(),
            path: repo.to_path_buf(),
            template: Some(template.display().to_string()),
            template_mode: Some(TemplateMode::Committed),
            vcs_ref: None,
            force,
            write: true,
            minimal: false,
            defaults: true,
            no_input: true,
            no_vault: true,
            answers: AnswerOpts::default(),
        })
    } else {
        run_update(UpdateOpts {
            path: repo.to_path_buf(),
            template: None,
            template_mode: None,
            recopy: false,
            launcher_only: false,
            force,
            vcs_ref: None,
            defaults: true,
            no_input: true,
        })
    }
}

#[test]
fn refresh_retires_owned_mcp_registration_without_deleting_other_servers() {
    let _guard = lock_env();
    let template = materialize_template_git_worktree();
    for adopt in [false, true] {
        for shared in [false, true] {
            let temp = tempdir().unwrap();
            let repo = temp.path().join("repo");
            write_test_crate_guide(&repo);
            adopt_repo_for_test(&repo, template.path(), TemplateMode::Committed);
            assert!(!repo.join(".mcp.json").exists());
            let mut config: serde_json::Value = serde_json::from_str(GENERATED).unwrap();
            if shared {
                config["mcpServers"]["example"] = json!({"command": "example-server", "args": []});
                config["project_setting"] = json!(true);
            }
            legacy_registration(&repo, &serde_json::to_string(&config).unwrap(), true);
            #[cfg(unix)]
            if shared {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(repo.join(".mcp.json"), fs::Permissions::from_mode(0o600))
                    .unwrap();
            }
            let output = refresh(&repo, template.path(), adopt, true).unwrap();
            assert!(
                !managed_paths::load_manifest(&repo)
                    .unwrap()
                    .unwrap()
                    .contains(Path::new(".mcp.json"))
            );
            if shared {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    assert_eq!(
                        fs::metadata(repo.join(".mcp.json"))
                            .unwrap()
                            .permissions()
                            .mode()
                            & 0o777,
                        0o600
                    );
                }
                config["mcpServers"].as_object_mut().unwrap().remove("jig");
                let retained: serde_json::Value =
                    serde_json::from_slice(&fs::read(repo.join(".mcp.json")).unwrap()).unwrap();
                assert_eq!(retained, config);
                assert!(
                    output["render_report"]["files_modified"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|path| path == ".mcp.json")
                );
                let before = fs::read(repo.join(".mcp.json")).unwrap();
                refresh(&repo, template.path(), adopt, true).unwrap();
                assert_eq!(fs::read(repo.join(".mcp.json")).unwrap(), before);
            } else {
                assert!(!repo.join(".mcp.json").exists());
                assert!(
                    output["render_report"]["files_removed"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|path| path == ".mcp.json")
                );
            }
        }
    }
}

#[test]
fn refresh_preserves_unowned_customized_and_malformed_mcp_config() {
    let _guard = lock_env();
    let template = materialize_template_git_worktree();
    for (owned, contents) in [
        (false, GENERATED),
        (true, "{invalid JSON"),
        (
            true,
            r#"{"mcpServers":{"jig":{"command":"custom-jig","args":["mcp"]}}}"#,
        ),
        (
            true,
            r#"{"mcpServers":{"jig":{"command":"./scripts/jig","args":["mcp"],"env":{"EXAMPLE":"value"}}}}"#,
        ),
        (true, r#"{"mcpServers":{},"project_setting":true}"#),
        (
            true,
            r#"{"mcpServers":{},"mcpServers":{"jig":{"command":"./scripts/jig","args":["mcp"]}}}"#,
        ),
    ] {
        let temp = tempdir().unwrap();
        let repo = temp.path().join("repo");
        write_test_crate_guide(&repo);
        adopt_repo_for_test(&repo, template.path(), TemplateMode::Committed);
        legacy_registration(&repo, contents, owned);
        refresh(&repo, template.path(), false, true).unwrap();
        assert_eq!(
            fs::read_to_string(repo.join(".mcp.json")).unwrap(),
            contents
        );
        assert!(
            !managed_paths::load_manifest(&repo)
                .unwrap()
                .unwrap()
                .contains(Path::new(".mcp.json"))
        );
    }
}

#[test]
fn mcp_retirement_requires_force_and_leaves_the_original_config_and_manifest_on_refusal() {
    let _guard = lock_env();
    let template = materialize_template_git_worktree();
    for adopt in [false, true] {
        let temp = tempdir().unwrap();
        let repo = temp.path().join("repo");
        write_test_crate_guide(&repo);
        adopt_repo_for_test(&repo, template.path(), TemplateMode::Committed);
        legacy_registration(&repo, GENERATED, true);
        let manifest = fs::read(repo.join(managed_paths::MANIFEST_PATH)).unwrap();
        let error = refresh(&repo, template.path(), adopt, false).unwrap_err();
        assert!(error.to_string().contains(".mcp.json"), "{error:#}");
        assert_eq!(
            fs::read_to_string(repo.join(".mcp.json")).unwrap(),
            GENERATED
        );
        assert_eq!(
            fs::read(repo.join(managed_paths::MANIFEST_PATH)).unwrap(),
            manifest
        );
    }
}

#[cfg(unix)]
#[test]
fn mcp_retirement_preserves_nonregular_config_and_symlink_targets() {
    let _guard = lock_env();
    let template = materialize_template_git_worktree();
    for symlink in [false, true] {
        let temp = tempdir().unwrap();
        let repo = temp.path().join("repo");
        write_test_crate_guide(&repo);
        adopt_repo_for_test(&repo, template.path(), TemplateMode::Committed);
        legacy_registration(&repo, GENERATED, true);
        fs::remove_file(repo.join(".mcp.json")).unwrap();
        let target = temp.path().join("example-config.json");
        fs::write(&target, GENERATED).unwrap();
        if symlink {
            std::os::unix::fs::symlink(&target, repo.join(".mcp.json")).unwrap();
        } else {
            fs::create_dir(repo.join(".mcp.json")).unwrap();
        }
        refresh(&repo, template.path(), false, true).unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), GENERATED);
        let metadata = fs::symlink_metadata(repo.join(".mcp.json")).unwrap();
        assert_eq!(metadata.is_symlink(), symlink);
        assert_eq!(metadata.is_dir(), !symlink);
        assert!(
            !managed_paths::load_manifest(&repo)
                .unwrap()
                .unwrap()
                .contains(Path::new(".mcp.json"))
        );
    }
}
