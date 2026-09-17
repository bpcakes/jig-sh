use super::*;
use crate::context::RepoContext;
use jig_contract::{ComponentSpec, ProfileSpec};
use serde_json::{Value, json};
use std::fs;
use std::io::Write;
use std::process::{Command, Output, Stdio};

struct Fixture {
    temp: tempfile::TempDir,
}

impl Fixture {
    fn new(
        command: &str,
        authored: Vec<ActionSpec>,
        resolved: Vec<ActionSpec>,
        inline: bool,
    ) -> Self {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir(temp.path().join(".agent")).unwrap();
        let mut repository = json!({
            "components": [ComponentSpec::new("workspace".parse().unwrap(), ".")],
            "actions": authored,
            "profiles": [ProfileSpec::new("verify".parse().unwrap(), vec!["workspace:fmt".parse().unwrap()])],
            "default_check_profile": "verify"
        });
        let mut config = json!({
            "_src_path": "embedded", "_commit": "example",
            "repo_name": "ExampleProject", "default_branch": "main",
            "commands": {"format_command": command, "unrelated_command": "echo preserved"},
            "repository": repository
        });
        let body = if inline {
            let source = config
                .as_object_mut()
                .unwrap()
                .remove("repository")
                .unwrap();
            format!(
                "{}\n[repository]\n{}\n",
                toml::to_string(&config).unwrap(),
                source
                    .as_object()
                    .unwrap()
                    .iter()
                    .map(|(key, value)| format!("{key} = {}", inline_toml(value)))
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        } else {
            toml::to_string(&config).unwrap()
        };
        fs::write(
            temp.path().join(".jig.toml"),
            format!("# Keep this owner comment.\n{body}"),
        )
        .unwrap();
        repository["actions"] = json!(resolved);
        repository["contract_version"] = json!(8);
        repository["tool_namespace"] = json!("jig");
        fs::write(
            temp.path().join(".agent/jig-contract.json"),
            serde_json::to_string_pretty(&repository).unwrap(),
        )
        .unwrap();
        Self { temp }
    }

    fn context(&self) -> RepoContext {
        RepoContext::load_from_root(self.temp.path().into()).unwrap()
    }

    fn contents(&self) -> [String; 2] {
        [".jig.toml", ".agent/jig-contract.json"]
            .map(|path| fs::read_to_string(self.temp.path().join(path)).unwrap())
    }

    fn apply(&self, patch: &str) {
        let result = self.try_apply(patch);
        assert!(
            result.status.success(),
            "{}\n{patch}",
            String::from_utf8_lossy(&result.stderr)
        );
    }

    fn try_apply(&self, patch: &str) -> Output {
        let initialized = Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(self.temp.path())
            .output()
            .unwrap();
        assert!(
            initialized.status.success(),
            "{}",
            String::from_utf8_lossy(&initialized.stderr)
        );
        let mut child = Command::new("git")
            .args(["apply", "--whitespace=error", "-"])
            .current_dir(self.temp.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(patch.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }
}

fn inline_toml(value: &Value) -> String {
    match value {
        Value::Object(fields) => format!(
            "{{ {} }}",
            fields
                .iter()
                .map(|(key, value)| format!("{key} = {}", inline_toml(value)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::Array(values) => format!(
            "[{}]",
            values
                .iter()
                .map(inline_toml)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        _ => value.to_string(),
    }
}

fn formatter() -> ActionSpec {
    let mut action = ActionSpec::new(
        "workspace:fmt".parse().unwrap(),
        ActionIntent::Check,
        ActionRunner::Shell {
            command: "format_command".into(),
            working_directory: None,
            environment: Default::default(),
        },
    );
    action.effects = vec![ActionEffect::ReadOnly, ActionEffect::Process];
    action.inputs = vec!["**/*.rs".into()];
    action
}

fn request() -> Request {
    Request {
        targets: vec!["workspace:fmt".parse().unwrap()],
        patch: true,
        ..Default::default()
    }
}

#[test]
fn paired_patch_is_read_only_deterministic_applicable_and_idempotent() {
    for inline in [false, true] {
        let action = formatter();
        let fixture = Fixture::new(
            "cargo fmt --all -- --check",
            vec![action.clone()],
            vec![action],
            inline,
        );
        let before = fixture.contents();
        let ctx = fixture.context();
        let request = Request {
            assert_worktree: true,
            ..request()
        };
        let result = preview(&ctx, &request).unwrap();
        assert_eq!(result, preview(&ctx, &request).unwrap());
        assert_eq!(fixture.contents(), before);
        assert!(!fixture.temp.path().join(".agent/state").exists());
        assert_eq!(result["changed_targets"].as_array().unwrap().len(), 1);
        let patch = result["patch"].as_str().unwrap();
        assert!(patch.contains("--- a/.jig.toml"), "{patch}");
        assert!(patch.contains("--- a/.agent/jig-contract.json"), "{patch}");
        fixture.apply(patch);
        let after = fixture.contents();
        assert!(after[0].contains("# Keep this owner comment."));
        let config: toml::Value = toml::from_str(&after[0]).unwrap();
        assert_eq!(
            config["commands"]["unrelated_command"].as_str(),
            Some("echo preserved")
        );
        let repeated = preview(&fixture.context(), &request).unwrap();
        assert_eq!(repeated["patch"], "");
        assert_eq!(repeated["changed_targets"], json!([]));
        assert_eq!(fixture.contents(), after);
    }
}

#[test]
fn conflicting_manifest_preimage_prevents_partial_paired_patch_application() {
    let mut action = formatter();
    action.source_state = Some(ActionSourceState::Git);
    let fixture = Fixture::new(
        "cargo fmt --all -- --check",
        vec![action.clone()],
        vec![action],
        false,
    );
    let request = Request {
        assert_worktree: true,
        ..request()
    };
    let result = preview(&fixture.context(), &request).unwrap();
    let patch = result["patch"].as_str().unwrap();
    assert!(patch.contains("--- a/.jig.toml"));
    assert!(patch.contains("--- a/.agent/jig-contract.json"));
    assert!(
        patch
            .lines()
            .any(|line| { line.starts_with('-') && line.contains("\"source_state\": \"git\"") })
    );

    let manifest_path = fixture.temp.path().join(".agent/jig-contract.json");
    let manifest = fs::read_to_string(&manifest_path).unwrap();
    let conflicting = manifest.replace(
        "\"source_state\": \"git\"",
        "\"source_state\": \"worktree\"",
    );
    assert_ne!(conflicting, manifest);
    fs::write(manifest_path, conflicting).unwrap();
    let before_apply = fixture.contents();

    let applied = fixture.try_apply(patch);
    assert!(
        !applied.status.success(),
        "conflicting patch unexpectedly applied"
    );
    assert_eq!(fixture.contents(), before_apply);
}

#[test]
fn automatic_preview_preserves_explicit_conservative_policies() {
    let mut action = formatter();
    action.source_state = Some(ActionSourceState::Git);
    action.inputs_policy = Some(ActionInputsPolicy::WholeRepository);
    let fixture = Fixture::new(
        "cargo fmt --all -- --check",
        vec![action.clone()],
        vec![action],
        false,
    );
    let before = fixture.contents();
    let result = preview(&fixture.context(), &request()).unwrap();
    assert_eq!(result["patch"], "");
    assert_eq!(result["changed_targets"], json!([]));
    assert_eq!(fixture.contents(), before);
}

#[test]
fn automatic_patch_does_not_promote_cargo_formatters_without_owner_assertions() {
    let action = formatter();
    let fixture = Fixture::new(
        "cargo fmt --all -- --check",
        vec![action.clone()],
        vec![action],
        false,
    );
    let request = Request {
        patch: true,
        ..Default::default()
    };
    let result = preview(&fixture.context(), &request).unwrap();
    assert_eq!(result["changed_targets"], json!([]));
    assert_eq!(result["patch"], "");
    assert_eq!(
        result["targets"][0]["reason"],
        "formatter_requires_assertion"
    );
}

#[test]
fn explicit_owner_assertions_append_inputs_and_preserve_unselected_policies() {
    let selected = formatter();
    let mut unrelated = formatter();
    unrelated.target = "workspace:custom".parse().unwrap();
    unrelated.source_state = Some(ActionSourceState::Git);
    unrelated.inputs_policy = Some(ActionInputsPolicy::Exhaustive);
    unrelated.inputs = vec!["fixtures/**".into()];
    let fixture = Fixture::new(
        "scripts/check-format.sh",
        vec![selected.clone(), unrelated.clone()],
        vec![selected, unrelated.clone()],
        false,
    );
    let ctx = fixture.context();
    let opaque = preview(&ctx, &request()).unwrap();
    assert_eq!(opaque["patch"], "");
    let request = Request {
        assert_worktree: true,
        assert_exhaustive: true,
        inputs: vec![
            "scripts/check-format.sh".into(),
            "rustfmt.toml".into(),
            "**/*.rs".into(),
        ],
        ..request()
    };
    let result = preview(&ctx, &request).unwrap();
    fixture.apply(result["patch"].as_str().unwrap());
    let updated = fixture.context();
    let actions = updated.authored_action_specs().unwrap();
    let selected = actions
        .iter()
        .find(|action| action.target.to_string() == "workspace:fmt")
        .unwrap();
    assert_eq!(selected.source_state, Some(ActionSourceState::Worktree));
    assert_eq!(selected.inputs_policy, Some(ActionInputsPolicy::Exhaustive));
    assert_eq!(
        selected.inputs,
        vec!["**/*.rs", "scripts/check-format.sh", "rustfmt.toml"]
    );
    assert_eq!(
        actions
            .iter()
            .find(|action| action.target == unrelated.target)
            .unwrap(),
        &unrelated
    );
    let repeated = preview(&updated, &request).unwrap();
    assert_eq!(repeated["patch"], "");
    assert_eq!(repeated["changed_targets"], json!([]));
}

#[test]
fn paired_patch_handles_normalized_default_omission_on_either_side() {
    for authored_explicit in [false, true] {
        let mut authored = formatter();
        let mut resolved = formatter();
        if authored_explicit {
            authored.source_state = Some(ActionSourceState::Git);
        } else {
            resolved.source_state = Some(ActionSourceState::Git);
        }
        let fixture = Fixture::new(
            "cargo fmt --all -- --check",
            vec![authored],
            vec![resolved],
            false,
        );
        let request = Request {
            assert_worktree: true,
            ..request()
        };
        let result = preview(&fixture.context(), &request).unwrap();
        fixture.apply(result["patch"].as_str().unwrap());
        assert_eq!(preview(&fixture.context(), &request).unwrap()["patch"], "");
    }
}

#[test]
fn invalid_adoption_requests_fail_without_writing_authority() {
    let action = formatter();
    let fixture = Fixture::new(
        "cargo fmt --all -- --check",
        vec![action.clone()],
        vec![action],
        false,
    );
    let before = fixture.contents();
    let ctx = fixture.context();
    let requests = [
        Request {
            assert_worktree: true,
            patch: true,
            ..Default::default()
        },
        Request {
            assert_exhaustive: true,
            patch: true,
            ..Default::default()
        },
        Request {
            targets: vec!["workspace:missing".parse().unwrap()],
            ..request()
        },
        Request {
            inputs: vec!["fixtures/**".into()],
            ..request()
        },
        Request {
            assert_exhaustive: true,
            inputs: vec!["../outside/**".into()],
            ..request()
        },
        Request {
            assert_exhaustive: true,
            inputs: vec!["/absolute/**".into()],
            ..request()
        },
        Request {
            assert_exhaustive: true,
            inputs: vec!["[".into()],
            ..request()
        },
    ];
    for request in requests {
        assert!(preview(&ctx, &request).is_err());
        assert_eq!(fixture.contents(), before);
    }
}

#[test]
fn native_runners_refuse_source_and_input_ownership_assertions() {
    let mut action = formatter();
    action.runner = ActionRunner::native("jig.file_budget");
    let fixture = Fixture::new(
        "cargo fmt --all -- --check",
        vec![action.clone()],
        vec![action],
        false,
    );
    let before = fixture.contents();
    let ctx = fixture.context();
    for request in [
        Request {
            assert_worktree: true,
            ..request()
        },
        Request {
            assert_exhaustive: true,
            ..request()
        },
    ] {
        assert!(preview(&ctx, &request).is_err());
        assert_eq!(fixture.contents(), before);
    }
}

#[test]
fn preview_refuses_authority_changes_after_context_load() {
    for path in [".jig.toml", ".agent/jig-contract.json"] {
        let action = formatter();
        let fixture = Fixture::new(
            "cargo fmt --all -- --check",
            vec![action.clone()],
            vec![action],
            false,
        );
        let ctx = fixture.context();
        let path = fixture.temp.path().join(path);
        let mut contents = fs::read_to_string(&path).unwrap();
        contents.push('\n');
        fs::write(path, contents).unwrap();
        let before = fixture.contents();
        assert!(preview(&ctx, &request()).is_err());
        assert_eq!(fixture.contents(), before);
    }
}
