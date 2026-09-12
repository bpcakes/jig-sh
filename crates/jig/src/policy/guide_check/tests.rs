use std::fs;
use std::path::Path;

use serde_json::{Value, json};
use tempfile::tempdir;

use super::check;
use crate::agent_guides::references::{
    Destination, GuideFiles, MAX_GUIDE_BYTES, markdown_references, resolve_reference,
};
use crate::context::RepoContext;
use crate::test_env::TestRepoBuilder;

fn fixture(root: &Path, language: &str, guidance: Option<&str>) -> RepoContext {
    let mut component = json!({"id":"example", "root":".", "adapters":[language]});
    if let Some(guidance) = guidance {
        component["guidance"] = json!(guidance);
    }
    let repository = json!({"components":[component], "actions":[], "profiles":[{"id":"verify","targets":[]}], "default_check_profile":"verify"});
    fs::create_dir_all(root.join(".agent")).unwrap();
    let config = json!({"_src_path":"embedded:jig-sh", "_commit":"example", "default_branch":"main", "repo_name":"ExampleProject", "repository":repository});
    fs::write(root.join(".jig.toml"), toml::to_string(&config).unwrap()).unwrap();
    fs::write(
        root.join(".agent/jig-contract.json"),
        serde_json::to_vec(&json!({
            "contract_version":8, "tool_namespace":"jig", "required_commands":[], "tools":[],
            "components":repository["components"], "actions":[], "profiles":repository["profiles"], "default_check_profile":"verify"
        }))
        .unwrap(),
    )
    .unwrap();
    RepoContext::load_from(root).unwrap()
}

fn codes(output: &Value, severity: &str) -> Vec<String> {
    output["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|d| d["severity"] == severity)
        .map(|d| d["code"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn concise_rust_and_go_guides_and_missing_optional_guides_pass() {
    for language in ["rust", "go"] {
        let temp = tempdir().unwrap();
        let ctx = fixture(temp.path(), language, None);
        let empty = check(&ctx).unwrap();
        assert_eq!(empty["ok"], true);
        assert_eq!(empty["guide_count"], 0);
        fs::create_dir_all(temp.path().join("src/nested")).unwrap();
        fs::write(
            temp.path().join("src/nested/AGENTS.md"),
            "# Ownership\nKeep the public parser stable. [Source](../source.txt#parser)\n",
        )
        .unwrap();
        fs::write(temp.path().join("src/source.txt"), "Example source\n").unwrap();
        let result = check(&ctx).unwrap();
        assert_eq!(result["ok"], true, "{result}");
        assert_eq!(result["guide_count"], 1);
        assert_eq!(codes(&result, "warning"), ["guide_structure"]);
        for field in ["missing_guides", "missing_sections", "missing_entry_ref"] {
            assert_eq!(result[field], json!([]));
        }
        assert!(result["missing_guides_note"].is_string());
        assert_eq!(check(&ctx).unwrap(), result);
    }
}

#[test]
fn structure_advice_applies_only_to_nested_agents_guides() {
    let root = tempdir().unwrap();
    let ctx = fixture(root.path(), "rust", Some("docs/owner.md"));
    fs::create_dir(root.path().join("docs")).unwrap();
    fs::write(root.path().join("AGENTS.md"), "<!-- BEGIN JIG MANAGED BLOCK -->\n# Repository Guidelines\n<!-- END JIG MANAGED BLOCK -->\n").unwrap();
    fs::write(
        root.path().join("docs/owner.md"),
        "# Owner\nKeep APIs stable.\n",
    )
    .unwrap();
    let report = check(&ctx).unwrap();
    assert_eq!(report["ok"], true);
    assert!(report["diagnostics"].as_array().unwrap().is_empty());

    fs::write(root.path().join("docs/AGENTS.md"), "# Docs\n").unwrap();
    let report = check(&ctx).unwrap();
    assert_eq!(codes(&report, "warning"), ["guide_structure"]);
    assert_eq!(report["diagnostics"][0]["guide"], "docs/AGENTS.md");

    // Exemption from style advice must not bypass reference validation.
    fs::write(root.path().join("AGENTS.md"), "[Root](missing-root.md)\n").unwrap();
    fs::write(
        root.path().join("docs/owner.md"),
        "[Owner](missing-owner.md)\n",
    )
    .unwrap();
    let report = check(&ctx).unwrap();
    assert_eq!(report["ok"], false);
    assert_eq!(
        codes(&report, "error"),
        ["reference_missing", "reference_missing"]
    );
    assert_eq!(codes(&report, "warning"), ["guide_structure"]);
}

#[test]
fn explicit_owner_guides_are_checked_with_component_identity_for_both_languages() {
    for language in ["rust", "go"] {
        let temp = tempdir().unwrap();
        let ctx = fixture(temp.path(), language, Some("docs/owner.md"));
        let missing = check(&ctx).unwrap();
        assert_eq!(missing["ok"], false);
        let diagnostic = &missing["diagnostics"][0];
        assert_eq!(diagnostic["code"], "owner_guide_missing");
        assert_eq!(diagnostic["component"], "example");
        assert_eq!(diagnostic["reference"], "docs/owner.md");
        fs::create_dir_all(temp.path().join("docs")).unwrap();
        fs::write(
            temp.path().join("docs/owner.md"),
            "Read [implementation](../src.txt).\n",
        )
        .unwrap();
        assert_eq!(codes(&check(&ctx).unwrap(), "error"), ["reference_missing"]);
        fs::write(temp.path().join("src.txt"), "Example implementation\n").unwrap();
        assert_eq!(check(&ctx).unwrap()["ok"], true);
    }
}

#[test]
fn invalid_owner_paths_and_directory_owners_fail() {
    for path in [
        "../outside.md",
        "/outside.md",
        "C:/outside.md",
        "https://example.invalid/guide.md",
        "docs\\owner.md",
        "",
        "docs/\u{1b}owner.md",
    ] {
        let temp = tempdir().unwrap();
        let ctx = fixture(temp.path(), "go", Some(path));
        assert_eq!(
            codes(&check(&ctx).unwrap(), "error"),
            ["owner_guide_invalid"],
            "{path:?}"
        );
    }
    let temp = tempdir().unwrap();
    fs::create_dir(temp.path().join("docs")).unwrap();
    let ctx = fixture(temp.path(), "rust", Some("docs"));
    assert_eq!(
        codes(&check(&ctx).unwrap(), "error"),
        ["owner_guide_unreadable"]
    );
}

#[test]
fn legacy_epochs_keep_result_fields_and_accept_nonstandard_guides() {
    for epoch in [2, 3, 4, 5] {
        let temp = tempdir().unwrap();
        TestRepoBuilder::new(temp.path())
            .contract_version(epoch)
            .required_commands(Vec::<String>::new())
            .write();
        fs::write(
            temp.path().join("AGENTS.md"),
            "# Working here\nKeep changes small.\n",
        )
        .unwrap();
        let result = check(&RepoContext::load_from(temp.path()).unwrap()).unwrap();
        assert_eq!(result["ok"], true, "epoch {epoch}: {result}");
        assert_eq!(result["guide_count"], 1);
        for field in ["missing_guides", "missing_sections", "missing_entry_ref"] {
            assert!(result[field].as_array().unwrap().is_empty());
        }
    }
}

#[test]
fn markdown_parser_handles_references_and_ignores_code_examples() {
    let text = "# Example\n[inline](file(a).md \"title\")\n[ref][owner]\n![image](image.png)\n\n[owner]: <docs/owner guide.md>\n\n`[code](missing.md)`\n\n```md\n[code](missing.md)\n```\n\n    [code](missing.md)\n\n<!-- [example](missing.md) -->\n\\[escaped](missing.md)\n";
    let refs = markdown_references(text);
    assert_eq!(
        refs.iter()
            .map(|r| (r.line, r.target.as_str()))
            .collect::<Vec<_>>(),
        [
            (2, "file(a).md"),
            (3, "docs/owner guide.md"),
            (4, "image.png")
        ]
    );
}

#[test]
fn links_resolve_relative_encoded_root_and_fragment_destinations() {
    for (raw, expected) in [
        ("../owner.md#topic", "docs/owner.md"),
        ("/AGENTS.md", "AGENTS.md"),
        ("file%20name.md", "docs/area/file name.md"),
        ("../../", "."),
        ("file.md?view=raw#topic", "docs/area/file.md"),
        ("./caf%C3%A9.md", "docs/area/café.md"),
    ] {
        assert_eq!(
            resolve_reference(Path::new("docs/area/AGENTS.md"), raw).unwrap(),
            Destination::Local(expected.into())
        );
    }
    for raw in ["#arbitrary-heading", ""] {
        assert_eq!(
            resolve_reference(Path::new("AGENTS.md"), raw).unwrap(),
            Destination::Fragment
        );
    }
    for raw in [
        "https://example.invalid/x",
        "mailto:example@example.invalid",
        "//example.invalid/x",
        "file:///outside.md",
    ] {
        assert_eq!(
            resolve_reference(Path::new("AGENTS.md"), raw).unwrap(),
            Destination::External
        );
    }
}

#[test]
fn invalid_links_fail_before_filesystem_access() {
    for raw in [
        "../outside.md",
        "%2e%2e/outside.md",
        "%2e%2e%2foutside.md",
        "bad%escape.md",
        "bad%00.md",
        "bad%ff.md",
        "C:/outside.md",
        "\\\\host\\guide",
        "bad\\guide.md",
    ] {
        assert!(
            resolve_reference(Path::new("AGENTS.md"), raw).is_err(),
            "{raw}"
        );
    }
}

#[test]
fn reports_precise_link_locations_and_unverified_external_references() {
    let temp = tempdir().unwrap();
    let ctx = fixture(temp.path(), "rust", None);
    fs::write(temp.path().join("AGENTS.md"), "# Guide\n[broken](missing.md#section)\n[escape](../outside.md)\n[external](https://example.invalid/no-network)\n[fragment](#anything)\n").unwrap();
    let result = check(&ctx).unwrap();
    assert_eq!(result["ok"], false);
    assert_eq!(
        codes(&result, "error"),
        ["reference_missing", "reference_invalid"]
    );
    assert_eq!(codes(&result, "info"), ["external_reference"]);
    assert_eq!(result["diagnostics"][0]["guide"], "AGENTS.md");
    assert_eq!(result["diagnostics"][0]["line"], 2);
    assert_eq!(result["diagnostics"][0]["reference"], "missing.md#section");
}

#[cfg(unix)]
#[test]
fn symlink_guides_targets_and_ancestors_never_read_outside_content() {
    use std::os::unix::fs::symlink;
    let temp = tempdir().unwrap();
    let outside = tempdir().unwrap();
    fs::write(
        outside.path().join("guide.md"),
        "[private-sentinel](outside-only-missing.md)\n",
    )
    .unwrap();
    let ctx = fixture(temp.path(), "go", Some("linked/guide.md"));
    symlink(outside.path(), temp.path().join("linked")).unwrap();
    symlink(
        outside.path().join("guide.md"),
        temp.path().join("AGENTS.md"),
    )
    .unwrap();
    let result = check(&ctx).unwrap();
    assert_eq!(result["ok"], false);
    assert_eq!(
        codes(&result, "error"),
        ["guide_unreadable", "owner_guide_unreadable"]
    );
    assert!(!result.to_string().contains("outside-only-missing"));
    fs::remove_file(temp.path().join("AGENTS.md")).unwrap();
    fs::write(temp.path().join("AGENTS.md"), "[unsafe](linked/guide.md)\n").unwrap();
    assert!(codes(&check(&ctx).unwrap(), "error").contains(&"reference_unsafe".into()));
    let files = GuideFiles::new(temp.path()).unwrap();
    symlink(outside.path().join("guide.md"), temp.path().join("leaf.md")).unwrap();
    assert!(files.check_target("leaf.md", false).is_err());
    assert!(files.read("leaf.md").is_err());
    assert_eq!(
        fs::read_to_string(outside.path().join("guide.md")).unwrap(),
        "[private-sentinel](outside-only-missing.md)\n"
    );
}

#[test]
fn guide_reads_are_bounded_and_reject_invalid_utf8() {
    let temp = tempdir().unwrap();
    let ctx = fixture(temp.path(), "rust", None);
    let guide = temp.path().join("AGENTS.md");
    fs::File::create(&guide)
        .unwrap()
        .set_len(MAX_GUIDE_BYTES + 1)
        .unwrap();
    assert_eq!(codes(&check(&ctx).unwrap(), "error"), ["guide_unreadable"]);
    fs::write(guide, [0xff]).unwrap();
    assert_eq!(codes(&check(&ctx).unwrap(), "error"), ["guide_unreadable"]);
}

#[test]
fn undefined_explicit_reference_links_fail_but_prose_and_code_do_not() {
    let text = "[owner][missing]\n`[code][missing]`\n\n[plain prose]\n";
    let references = markdown_references(text);
    assert_eq!(references.len(), 1);
    assert_eq!(references[0].target, "missing");
    assert_eq!(references[0].line, 1);
    assert!(references[0].problem.is_some());
    let root = tempdir().unwrap();
    let ctx = fixture(root.path(), "rust", None);
    fs::write(root.path().join("AGENTS.md"), text).unwrap();
    assert_eq!(codes(&check(&ctx).unwrap(), "error"), ["reference_invalid"]);
}

#[test]
fn deleting_a_tracked_optional_guide_does_not_create_a_placeholder_obligation() {
    use std::process::Command;
    let root = tempdir().unwrap();
    let ctx = fixture(root.path(), "go", None);
    fs::write(root.path().join("AGENTS.md"), "Example guidance\n").unwrap();
    for args in [["init", "-q"], ["add", "AGENTS.md"]] {
        assert!(
            Command::new("git")
                .args(args)
                .current_dir(root.path())
                .status()
                .unwrap()
                .success()
        );
    }
    fs::remove_file(root.path().join("AGENTS.md")).unwrap();
    let result = check(&ctx).unwrap();
    assert_eq!(result["ok"], true, "{result}");
    assert_eq!(result["guide_count"], 0);
}
