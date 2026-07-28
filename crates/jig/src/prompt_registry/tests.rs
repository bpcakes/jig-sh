use super::*;
#[cfg(unix)]
use crate::test_env::{EnvVarGuard, lock_env};
use tempfile::tempdir;

#[test]
fn get_returns_exact_body_and_explicit_template_vars() {
    let temp = tempdir().unwrap();
    let registry = PromptRegistry::new(temp.path().to_path_buf(), None);
    registry
        .add_prompt(PromptAddRequest {
            name: "review-loop".into(),
            body: Some("Review {{ focus }}\nthen stop".into()),
            file: None,
            description: Some("Review loop".into()),
            tags: vec!["review".into()],
        })
        .unwrap();

    let rendered = registry
        .render_prompt(PromptRenderRequest {
            name: "review-loop".into(),
            vars: vec!["focus=auth".into()],
            raw: false,
        })
        .unwrap();

    assert_eq!(rendered, "Review auth\nthen stop");
}

#[test]
fn template_rendering_runs_even_without_vars() {
    let temp = tempdir().unwrap();
    let registry = PromptRegistry::new(temp.path().to_path_buf(), None);
    registry
        .add_prompt(PromptAddRequest {
            name: "conditional".into(),
            body: Some("{% if true %}yes{% endif %}".into()),
            file: None,
            description: None,
            tags: Vec::new(),
        })
        .unwrap();
    assert_eq!(
        registry
            .render_prompt(PromptRenderRequest {
                name: "conditional".into(),
                vars: Vec::new(),
                raw: false,
            })
            .unwrap(),
        "yes"
    );

    registry
        .add_prompt(PromptAddRequest {
            name: "missing".into(),
            body: Some("{{ required }}".into()),
            file: None,
            description: None,
            tags: Vec::new(),
        })
        .unwrap();
    let error = registry
        .render_prompt(PromptRenderRequest {
            name: "missing".into(),
            vars: Vec::new(),
            raw: false,
        })
        .unwrap_err()
        .to_string();
    assert!(error.contains("Failed to render prompt template"));
}

#[test]
fn raw_rendering_preserves_literal_template_syntax() {
    let temp = tempdir().unwrap();
    let registry = PromptRegistry::new(temp.path().to_path_buf(), None);
    registry
        .add_prompt(PromptAddRequest {
            name: "literal".into(),
            body: Some("Use {{ braces }} literally".into()),
            file: None,
            description: None,
            tags: Vec::new(),
        })
        .unwrap();

    let rendered = registry
        .render_prompt(PromptRenderRequest {
            name: "literal".into(),
            vars: Vec::new(),
            raw: true,
        })
        .unwrap();

    assert_eq!(rendered, "Use {{ braces }} literally");
}

#[test]
fn unqualified_reads_reject_ambiguous_namespaces() {
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let registry = PromptRegistry::new(temp.path().join("home"), Some(repo));
    registry
        .add_prompt(PromptAddRequest {
            name: "user:shared".into(),
            body: Some("user".into()),
            file: None,
            description: None,
            tags: Vec::new(),
        })
        .unwrap();
    registry
        .add_prompt(PromptAddRequest {
            name: "repo:shared".into(),
            body: Some("repo".into()),
            file: None,
            description: None,
            tags: Vec::new(),
        })
        .unwrap();

    let error = registry
        .render_prompt(PromptRenderRequest {
            name: "shared".into(),
            vars: Vec::new(),
            raw: false,
        })
        .unwrap_err()
        .to_string();

    assert!(error.contains("ambiguous"));
    assert!(error.contains("user:shared"));
    assert!(error.contains("repo:shared"));
}

#[test]
fn list_and_search_omit_bodies() {
    let temp = tempdir().unwrap();
    let registry = PromptRegistry::new(temp.path().to_path_buf(), None);
    registry
        .add_prompt(PromptAddRequest {
            name: "secret-review".into(),
            body: Some("sensitive body".into()),
            file: None,
            description: Some("Find regressions".into()),
            tags: vec!["review".into()],
        })
        .unwrap();

    let list = registry.list_prompts(false).unwrap();
    assert_eq!(list["prompts"][0]["qualified_name"], "user:secret-review");
    assert_eq!(list["prompts"][0].get("body"), None);
    assert_eq!(list["prompts"][0].get("path"), None);

    let search = registry.search_prompts("sensitive", false).unwrap();
    assert!(search["prompts"].as_array().unwrap().is_empty());
    let search = registry.search_prompts("sensitive", true).unwrap();
    assert_eq!(search["prompts"].as_array().unwrap().len(), 1);
}

#[test]
fn add_reports_overwrite_bumps_version_and_preserves_metadata() {
    let temp = tempdir().unwrap();
    let registry = PromptRegistry::new(temp.path().to_path_buf(), None);
    let first = registry
        .add_prompt(PromptAddRequest {
            name: "replace-me".into(),
            body: Some("first".into()),
            file: None,
            description: Some("Existing description".into()),
            tags: vec!["review".into(), "review".into()],
        })
        .unwrap();
    let second = registry
        .add_prompt(PromptAddRequest {
            name: "replace-me".into(),
            body: Some("second".into()),
            file: None,
            description: None,
            tags: Vec::new(),
        })
        .unwrap();

    assert_eq!(first["overwritten"], false);
    assert_eq!(second["overwritten"], true);
    let record = read_prompt_file(
        &temp.path().join("prompts/user/replace-me.md"),
        PromptName("replace-me".into()),
    )
    .unwrap();
    assert_eq!(record.metadata.version, 2);
    assert_eq!(
        record.metadata.description.as_deref(),
        Some("Existing description")
    );
    assert_eq!(record.metadata.tags, vec!["review"]);
}

#[test]
fn export_import_preserves_prompt_packs() {
    let temp = tempdir().unwrap();
    let source = PromptRegistry::new(temp.path().join("source"), None);
    let archive = PromptArchive {
        schema_version: ARCHIVE_SCHEMA_VERSION,
        prompts: vec![PromptArchiveEntry {
            namespace: "pack".into(),
            pack: Some("reviews".into()),
            name: "loop".into(),
            description: Some("Loop".into()),
            tags: vec!["review".into()],
            version: 2,
            updated_at: Some(123),
            body: "body".into(),
        }],
    };
    let archive_path = temp.path().join("archive.json");
    fs::write(&archive_path, serde_json::to_string(&archive).unwrap()).unwrap();
    source.import_prompts(&archive_path).unwrap();

    let rendered = source
        .render_prompt(PromptRenderRequest {
            name: "pack:reviews/loop".into(),
            vars: Vec::new(),
            raw: false,
        })
        .unwrap();
    assert_eq!(rendered, "body");

    let exported = source.export_prompts().unwrap();
    assert_eq!(exported["prompts"][0]["namespace"], "pack");
    assert_eq!(exported["prompts"][0]["pack"], "reviews");
}

#[test]
fn export_import_round_trips_user_prompts() {
    let temp = tempdir().unwrap();
    let source = PromptRegistry::new(temp.path().join("source"), None);
    source
        .add_prompt(PromptAddRequest {
            name: "review".into(),
            body: Some("body {{ focus }}".into()),
            file: None,
            description: Some("Review prompt".into()),
            tags: vec!["review".into()],
        })
        .unwrap();
    let archive_path = temp.path().join("archive.json");
    fs::write(
        &archive_path,
        serde_json::to_string(&source.export_prompts().unwrap()).unwrap(),
    )
    .unwrap();

    let target = PromptRegistry::new(temp.path().join("target"), None);
    target.import_prompts(&archive_path).unwrap();

    let rendered = target
        .render_prompt(PromptRenderRequest {
            name: "review".into(),
            vars: vec!["focus=auth".into()],
            raw: false,
        })
        .unwrap();
    assert_eq!(rendered, "body auth");
    let list = target.list_prompts(false).unwrap();
    assert_eq!(list["prompts"][0]["description"], "Review prompt");
    assert_eq!(list["prompts"][0]["tags"], serde_json::json!(["review"]));
}

#[test]
fn export_refuses_invalid_prompt_files() {
    let temp = tempdir().unwrap();
    let registry = PromptRegistry::new(temp.path().to_path_buf(), None);
    fs::create_dir_all(temp.path().join("prompts/user")).unwrap();
    fs::write(
        temp.path().join("prompts/user/broken.md"),
        "---\nname: other\n---\nbody",
    )
    .unwrap();

    let error = registry.export_prompts().unwrap_err().to_string();

    assert!(error.contains("Cannot export prompt registry while invalid prompts exist"));
}

#[test]
fn frontmatter_can_end_at_eof() {
    let temp = tempdir().unwrap();
    let registry = PromptRegistry::new(temp.path().to_path_buf(), None);
    fs::create_dir_all(temp.path().join("prompts/user")).unwrap();
    fs::write(
        temp.path().join("prompts/user/empty-body.md"),
        "---\nname: empty-body\n---",
    )
    .unwrap();

    let rendered = registry
        .render_prompt(PromptRenderRequest {
            name: "empty-body".into(),
            vars: Vec::new(),
            raw: false,
        })
        .unwrap();

    assert_eq!(rendered, "");
}

#[test]
fn list_can_omit_packs_and_skips_invalid_entries() {
    let temp = tempdir().unwrap();
    let registry = PromptRegistry::new(temp.path().to_path_buf(), None);
    registry
        .add_prompt(PromptAddRequest {
            name: "mine".into(),
            body: Some("user".into()),
            file: None,
            description: None,
            tags: Vec::new(),
        })
        .unwrap();
    let archive = PromptArchive {
        schema_version: ARCHIVE_SCHEMA_VERSION,
        prompts: vec![PromptArchiveEntry {
            namespace: "pack".into(),
            pack: Some("reviews".into()),
            name: "loop".into(),
            description: None,
            tags: Vec::new(),
            version: 1,
            updated_at: None,
            body: "pack".into(),
        }],
    };
    let archive_path = temp.path().join("archive.json");
    fs::write(&archive_path, serde_json::to_string(&archive).unwrap()).unwrap();
    registry.import_prompts(&archive_path).unwrap();
    fs::write(
        temp.path().join("prompt-packs/reviews/prompts/bad.md"),
        "---\nname: other\n---\nbad",
    )
    .unwrap();

    let without_packs = registry.list_prompts(false).unwrap();
    assert_eq!(without_packs["prompts"].as_array().unwrap().len(), 1);
    assert_eq!(without_packs["prompts"][0]["qualified_name"], "user:mine");

    let with_packs = registry.list_prompts(true).unwrap();
    assert_eq!(with_packs["prompts"].as_array().unwrap().len(), 2);
    assert_eq!(with_packs["warnings"].as_array().unwrap().len(), 1);
}

#[test]
fn import_prevalidates_before_writing() {
    let temp = tempdir().unwrap();
    let registry = PromptRegistry::new(temp.path().to_path_buf(), None);
    let archive = PromptArchive {
        schema_version: ARCHIVE_SCHEMA_VERSION,
        prompts: vec![
            PromptArchiveEntry {
                namespace: "user".into(),
                pack: None,
                name: "first".into(),
                description: None,
                tags: Vec::new(),
                version: 1,
                updated_at: None,
                body: "first".into(),
            },
            PromptArchiveEntry {
                namespace: "repo".into(),
                pack: None,
                name: "second".into(),
                description: None,
                tags: Vec::new(),
                version: 1,
                updated_at: None,
                body: "second".into(),
            },
        ],
    };
    let archive_path = temp.path().join("archive.json");
    fs::write(&archive_path, serde_json::to_string(&archive).unwrap()).unwrap();

    let error = registry
        .import_prompts(&archive_path)
        .unwrap_err()
        .to_string();

    assert!(error.contains("outside a Jig repo"));
    assert!(!temp.path().join("prompts/user/first.md").exists());
}

#[test]
fn import_rejects_unsupported_schema_version() {
    let temp = tempdir().unwrap();
    let registry = PromptRegistry::new(temp.path().to_path_buf(), None);
    let archive_path = temp.path().join("archive.json");
    fs::write(
        &archive_path,
        serde_json::json!({
            "schema_version": ARCHIVE_SCHEMA_VERSION + 1,
            "prompts": [],
        })
        .to_string(),
    )
    .unwrap();

    let error = registry
        .import_prompts(&archive_path)
        .unwrap_err()
        .to_string();

    assert!(error.contains("Unsupported prompt archive schema version"));
}

#[test]
fn import_rejects_case_insensitive_duplicate_destinations() {
    let temp = tempdir().unwrap();
    let registry = PromptRegistry::new(temp.path().to_path_buf(), None);
    let archive = PromptArchive {
        schema_version: ARCHIVE_SCHEMA_VERSION,
        prompts: vec![
            PromptArchiveEntry {
                namespace: "user".into(),
                pack: None,
                name: "Review".into(),
                description: None,
                tags: Vec::new(),
                version: 1,
                updated_at: None,
                body: "one".into(),
            },
            PromptArchiveEntry {
                namespace: "user".into(),
                pack: None,
                name: "review".into(),
                description: None,
                tags: Vec::new(),
                version: 1,
                updated_at: None,
                body: "two".into(),
            },
        ],
    };
    let archive_path = temp.path().join("archive.json");
    fs::write(&archive_path, serde_json::to_string(&archive).unwrap()).unwrap();

    let error = registry
        .import_prompts(&archive_path)
        .unwrap_err()
        .to_string();

    assert!(error.contains("duplicate destination"));
    assert!(!temp.path().join("prompts/user/Review.md").exists());
    assert!(!temp.path().join("prompts/user/review.md").exists());
}

#[test]
fn import_rejects_parent_child_collisions_before_writing() {
    let temp = tempdir().unwrap();
    let registry = PromptRegistry::new(temp.path().to_path_buf(), None);
    let archive = PromptArchive {
        schema_version: ARCHIVE_SCHEMA_VERSION,
        prompts: vec![
            PromptArchiveEntry {
                namespace: "user".into(),
                pack: None,
                name: "parent/child".into(),
                description: None,
                tags: Vec::new(),
                version: 1,
                updated_at: None,
                body: "child".into(),
            },
            PromptArchiveEntry {
                namespace: "user".into(),
                pack: None,
                name: "parent".into(),
                description: None,
                tags: Vec::new(),
                version: 1,
                updated_at: None,
                body: "parent".into(),
            },
        ],
    };
    let archive_path = temp.path().join("archive.json");
    fs::write(&archive_path, serde_json::to_string(&archive).unwrap()).unwrap();

    let error = registry
        .import_prompts(&archive_path)
        .unwrap_err()
        .to_string();

    assert!(error.contains("conflicts with nested prompt"));
    assert!(!temp.path().join("prompts/user/parent.md").exists());
    assert!(!temp.path().join("prompts/user/parent/child.md").exists());
}

#[test]
fn import_reports_overwritten_entries() {
    let temp = tempdir().unwrap();
    let registry = PromptRegistry::new(temp.path().to_path_buf(), None);
    registry
        .add_prompt(PromptAddRequest {
            name: "existing".into(),
            body: Some("old".into()),
            file: None,
            description: None,
            tags: Vec::new(),
        })
        .unwrap();
    let archive = PromptArchive {
        schema_version: ARCHIVE_SCHEMA_VERSION,
        prompts: vec![PromptArchiveEntry {
            namespace: "user".into(),
            pack: None,
            name: "existing".into(),
            description: None,
            tags: Vec::new(),
            version: 1,
            updated_at: None,
            body: "new".into(),
        }],
    };
    let archive_path = temp.path().join("archive.json");
    fs::write(&archive_path, serde_json::to_string(&archive).unwrap()).unwrap();

    let imported = registry.import_prompts(&archive_path).unwrap();

    assert_eq!(imported["imported"][0]["overwritten"], true);
    let rendered = registry
        .render_prompt(PromptRenderRequest {
            name: "existing".into(),
            vars: Vec::new(),
            raw: false,
        })
        .unwrap();
    assert_eq!(rendered, "new");
}

#[test]
fn import_rollback_restores_applied_files() {
    let temp = tempdir().unwrap();
    let existing = temp.path().join("existing.md");
    let created = temp.path().join("created.md");
    fs::write(&existing, "changed").unwrap();
    fs::write(&created, "new").unwrap();

    rollback_import(vec![
        AppliedImport {
            path: existing.clone(),
            original: Some(b"old".to_vec()),
        },
        AppliedImport {
            path: created.clone(),
            original: None,
        },
    ])
    .unwrap();

    assert_eq!(fs::read_to_string(existing).unwrap(), "old");
    assert!(!created.exists());
}

#[test]
fn add_rejects_file_directory_name_collisions() {
    let temp = tempdir().unwrap();
    let registry = PromptRegistry::new(temp.path().to_path_buf(), None);
    registry
        .add_prompt(PromptAddRequest {
            name: "parent".into(),
            body: Some("top".into()),
            file: None,
            description: None,
            tags: Vec::new(),
        })
        .unwrap();
    let nested_error = registry
        .add_prompt(PromptAddRequest {
            name: "parent/child".into(),
            body: Some("nested".into()),
            file: None,
            description: None,
            tags: Vec::new(),
        })
        .unwrap_err()
        .to_string();
    assert!(nested_error.contains("conflicts with existing prompt file"));

    let other = PromptRegistry::new(temp.path().join("other"), None);
    other
        .add_prompt(PromptAddRequest {
            name: "parent/child".into(),
            body: Some("nested".into()),
            file: None,
            description: None,
            tags: Vec::new(),
        })
        .unwrap();
    let parent_error = other
        .add_prompt(PromptAddRequest {
            name: "parent".into(),
            body: Some("top".into()),
            file: None,
            description: None,
            tags: Vec::new(),
        })
        .unwrap_err()
        .to_string();
    assert!(parent_error.contains("conflicts with existing prompt directory"));
}

#[test]
fn add_rejects_case_folded_filename_collision() {
    let temp = tempdir().unwrap();
    let registry = PromptRegistry::new(temp.path().to_path_buf(), None);
    registry
        .add_prompt(PromptAddRequest {
            name: "Review".into(),
            body: Some("upper".into()),
            file: None,
            description: None,
            tags: Vec::new(),
        })
        .unwrap();

    let error = registry
        .add_prompt(PromptAddRequest {
            name: "review".into(),
            body: Some("lower".into()),
            file: None,
            description: None,
            tags: Vec::new(),
        })
        .unwrap_err()
        .to_string();

    assert!(error.contains("conflicts with existing prompt file"));
}

#[test]
fn unqualified_get_reports_invalid_matching_file() {
    let temp = tempdir().unwrap();
    let registry = PromptRegistry::new(temp.path().to_path_buf(), None);
    fs::create_dir_all(temp.path().join("prompts/user")).unwrap();
    fs::write(
        temp.path().join("prompts/user/broken.md"),
        "---\nname: other\n---\nbody",
    )
    .unwrap();

    let error = registry
        .render_prompt(PromptRenderRequest {
            name: "broken".into(),
            vars: Vec::new(),
            raw: false,
        })
        .unwrap_err()
        .to_string();

    assert!(error.contains("does not match path name"));
}

#[test]
fn unqualified_get_reports_invalid_and_valid_matches() {
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let registry = PromptRegistry::new(temp.path().join("home"), Some(repo));
    fs::create_dir_all(temp.path().join("home/prompts/user")).unwrap();
    fs::write(
        temp.path().join("home/prompts/user/shared.md"),
        "---\nname: other\n---\nbody",
    )
    .unwrap();
    registry
        .add_prompt(PromptAddRequest {
            name: "repo:shared".into(),
            body: Some("repo".into()),
            file: None,
            description: None,
            tags: Vec::new(),
        })
        .unwrap();

    let error = registry
        .render_prompt(PromptRenderRequest {
            name: "shared".into(),
            vars: Vec::new(),
            raw: false,
        })
        .unwrap_err()
        .to_string();

    assert!(error.contains("invalid matching files"));
    assert!(error.contains("Valid matches also exist: repo:shared"));
}

#[test]
fn crlf_frontmatter_is_parsed() {
    let temp = tempdir().unwrap();
    let registry = PromptRegistry::new(temp.path().to_path_buf(), None);
    fs::create_dir_all(temp.path().join("prompts/user")).unwrap();
    fs::write(
        temp.path().join("prompts/user/windows.md"),
        "---\r\nname: windows\r\n---\r\nbody",
    )
    .unwrap();

    let rendered = registry
        .render_prompt(PromptRenderRequest {
            name: "windows".into(),
            vars: Vec::new(),
            raw: false,
        })
        .unwrap();

    assert_eq!(rendered, "body");
}

#[test]
fn template_vars_reject_invalid_keys_and_keep_equals_in_values() {
    let vars = parse_template_vars(&["key=value=more".into()]).unwrap();
    assert_eq!(vars["key"], "value=more");

    let error = parse_template_vars(&["1bad=value".into()])
        .unwrap_err()
        .to_string();
    assert!(error.contains("Invalid template variable key"));
}

#[test]
fn prompt_names_reject_markdown_extension() {
    let error = parse_selector("foo.md").unwrap_err().to_string();
    assert!(error.contains("should not include a .md extension"));
}

#[cfg(unix)]
#[test]
fn copy_reports_resolved_prompt_metadata() {
    let _env = lock_env();
    let temp = tempdir().unwrap();
    let clip_file = temp.path().join("clip.txt");
    let _clipboard = EnvVarGuard::set(
        "JIG_PROMPT_CLIPBOARD_COMMAND",
        format!("cat > {}", clip_file.display()),
    );
    let registry = PromptRegistry::new(temp.path().join("store"), None);
    registry
        .add_prompt(PromptAddRequest {
            name: "copy-me".into(),
            body: Some("copy body".into()),
            file: None,
            description: None,
            tags: Vec::new(),
        })
        .unwrap();

    let output = registry
        .copy_prompt(PromptRenderRequest {
            name: "user:copy-me".into(),
            vars: Vec::new(),
            raw: false,
        })
        .unwrap();

    assert_eq!(output["qualified_name"], "user:copy-me");
    assert_eq!(output["namespace"], "user");
    assert_eq!(output["name"], "copy-me");
    assert_eq!(fs::read_to_string(clip_file).unwrap(), "copy body");
}

#[cfg(unix)]
#[test]
fn editor_command_accepts_editor_with_arguments_and_removes_empty_new_prompt() {
    use std::os::unix::fs::PermissionsExt;

    let _env = lock_env();
    let temp = tempdir().unwrap();
    let editor = temp.path().join("editor.sh");
    fs::write(
        &editor,
        "#!/bin/sh\nif [ \"$1\" != \"--write\" ]; then exit 2; fi\nprintf edited >> \"$2\"\n",
    )
    .unwrap();
    let mut permissions = fs::metadata(&editor).unwrap().permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&editor, permissions).unwrap();
    let _visual = EnvVarGuard::remove("VISUAL");
    let _editor = EnvVarGuard::set("EDITOR", format!("{} --write", editor.display()));
    let registry = PromptRegistry::new(temp.path().join("store"), None);

    registry.edit_prompt("new-prompt").unwrap();

    let rendered = registry
        .render_prompt(PromptRenderRequest {
            name: "new-prompt".into(),
            vars: Vec::new(),
            raw: false,
        })
        .unwrap();
    assert_eq!(rendered, "edited");

    fs::write(&editor, "#!/bin/sh\n: > \"$1\"\n").unwrap();
    let _editor = EnvVarGuard::set("EDITOR", editor.as_os_str());
    let error = registry
        .edit_prompt("empty-prompt")
        .unwrap_err()
        .to_string();
    assert!(error.contains("was empty after edit"));
    assert!(
        !temp
            .path()
            .join("store/prompts/user/empty-prompt.md")
            .exists()
    );

    registry
        .add_prompt(PromptAddRequest {
            name: "existing-prompt".into(),
            body: Some("keep me".into()),
            file: None,
            description: None,
            tags: Vec::new(),
        })
        .unwrap();
    let error = registry
        .edit_prompt("existing-prompt")
        .unwrap_err()
        .to_string();
    assert!(error.contains("original prompt was restored"));
    assert_eq!(
        registry
            .render_prompt(PromptRenderRequest {
                name: "existing-prompt".into(),
                vars: Vec::new(),
                raw: false,
            })
            .unwrap(),
        "keep me"
    );
}

#[cfg(unix)]
#[test]
fn add_prompt_with_editor_seeds_metadata_and_saves_body() {
    use std::os::unix::fs::PermissionsExt;

    let _env = lock_env();
    let temp = tempdir().unwrap();
    let editor = temp.path().join("editor.sh");
    fs::write(
            &editor,
            "#!/bin/sh\ngrep -q 'description: Seeded' \"$1\" || exit 3\ngrep -q -- '- review' \"$1\" || exit 4\nprintf 'edited body\\n' >> \"$1\"\n",
        )
        .unwrap();
    let mut permissions = fs::metadata(&editor).unwrap().permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&editor, permissions).unwrap();
    let _visual = EnvVarGuard::remove("VISUAL");
    let _editor = EnvVarGuard::set("EDITOR", editor.as_os_str());
    let registry = PromptRegistry::new(temp.path().join("store"), None);

    let output = registry
        .add_prompt_with_editor(PromptAddRequest {
            name: "seeded-prompt".into(),
            body: None,
            file: None,
            description: Some("Seeded".into()),
            tags: vec!["review".into()],
        })
        .unwrap();

    assert_eq!(output["command"], "prompt add");
    assert_eq!(output["name"], "seeded-prompt");
    assert_eq!(output["overwritten"], false);
    assert_eq!(output["editor"], true);
    let rendered = registry
        .render_prompt(PromptRenderRequest {
            name: "seeded-prompt".into(),
            vars: Vec::new(),
            raw: false,
        })
        .unwrap();
    assert_eq!(rendered, "edited body");
}

#[test]
fn prompt_edit_target_reports_path_without_creating_new_prompt() {
    let temp = tempdir().unwrap();
    let registry = PromptRegistry::new(temp.path().join("store"), None);

    let target = registry.prompt_edit_target("new-prompt").unwrap();
    assert_eq!(target["name"], "new-prompt");
    assert_eq!(target["namespace"], "user");
    assert_eq!(target["editor"], false);
    assert_eq!(target["exists"], false);
    let path = target["path"].as_str().unwrap();
    assert!(path.ends_with("store/prompts/user/new-prompt.md"));
    assert!(!Path::new(path).exists());
    let human = format_prompt_human_output(&target).unwrap();
    assert!(human.contains("prompt edit: new-prompt"));
    assert!(human.contains(path));

    registry
        .add_prompt(PromptAddRequest {
            name: "new-prompt".into(),
            body: Some("body".into()),
            file: None,
            description: None,
            tags: Vec::new(),
        })
        .unwrap();

    let existing = registry.prompt_edit_target("new-prompt").unwrap();
    assert_eq!(existing["editor"], false);
    assert_eq!(existing["exists"], true);
    assert!(Path::new(existing["path"].as_str().unwrap()).exists());
}

#[test]
fn invalid_names_do_not_escape_storage() {
    let error = parse_selector("user:../bad").unwrap_err().to_string();
    assert!(error.contains("Invalid prompt name"));
}

#[cfg(unix)]
#[test]
fn prompt_home_env_rejects_empty_value() {
    let _env = lock_env();
    let _home = EnvVarGuard::set("JIG_PROMPT_HOME", "");

    let error = PromptRegistry::from_env(None).unwrap_err().to_string();

    assert!(error.contains("JIG_PROMPT_HOME cannot be empty"));
}
