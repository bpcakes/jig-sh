use crate::test_tempdir as tempdir;

use super::*;

#[test]
fn discovers_package_json_workspaces_with_dev_scripts() {
    let temp = tempdir().unwrap();
    fs::write(
        temp.path().join("package.json"),
        r#"{"workspaces":["apps/*"]}"#,
    )
    .unwrap();
    fs::create_dir_all(temp.path().join("apps/web")).unwrap();
    fs::write(
        temp.path().join("apps/web/package.json"),
        r#"{"name":"@demo/web","scripts":{"dev":"vite"}}"#,
    )
    .unwrap();

    let specs = discover(temp.path(), "repo", "localhost", "npm").unwrap();
    assert_eq!(specs.len(), 1);
    assert_eq!(specs[0].name, "@demo/web");
    assert_eq!(specs[0].hostname, "demo-web.repo.localhost");
    assert_eq!(specs[0].kind, AppKind::Vite);
}

#[test]
fn discovery_requires_workspace_at_repo_root() {
    let parent = tempdir().unwrap();
    fs::write(
        parent.path().join("package.json"),
        r#"{"workspaces":["repos/*"]}"#,
    )
    .unwrap();
    let repo = parent.path().join("repos/demo");
    fs::create_dir_all(&repo).unwrap();

    let specs = discover(&repo, "repo", "localhost", "npm").unwrap();
    assert!(specs.is_empty());
}

#[test]
fn double_star_workspace_globs_recurse() {
    let temp = tempdir().unwrap();
    fs::write(
        temp.path().join("package.json"),
        r#"{"workspaces":["apps/**"]}"#,
    )
    .unwrap();
    fs::create_dir_all(temp.path().join("apps/team/web")).unwrap();
    fs::write(
        temp.path().join("apps/team/web/package.json"),
        r#"{"name":"web","scripts":{"dev":"vite"}}"#,
    )
    .unwrap();

    let specs = discover(temp.path(), "repo", "localhost", "npm").unwrap();
    assert_eq!(specs.len(), 1);
    assert_eq!(
        specs[0].dir,
        temp.path().join("apps/team/web").canonicalize().unwrap()
    );
}

#[test]
fn non_vite_dev_scripts_remain_env_port_apps() {
    let temp = tempdir().unwrap();
    fs::write(
        temp.path().join("package.json"),
        r#"{"workspaces":["apps/*"]}"#,
    )
    .unwrap();
    fs::create_dir_all(temp.path().join("apps/api")).unwrap();
    fs::write(
        temp.path().join("apps/api/package.json"),
        r#"{"name":"api","scripts":{"dev":"node server.js"}}"#,
    )
    .unwrap();

    let specs = discover(temp.path(), "repo", "localhost", "npm").unwrap();
    assert_eq!(specs.len(), 1);
    assert_eq!(specs[0].kind, AppKind::EnvPort);
}

#[test]
fn vite_build_scripts_are_not_treated_as_dev_servers() {
    assert!(!script_looks_like_vite("vite build && vite preview"));
    assert!(script_looks_like_vite(
        "cross-env NODE_ENV=dev vite --host 127.0.0.1"
    ));
}

#[test]
fn null_workspaces_field_is_not_a_workspace_root() {
    let temp = tempdir().unwrap();
    fs::write(temp.path().join("package.json"), r#"{"workspaces":null}"#).unwrap();

    assert!(!package_json_has_workspaces(temp.path()));
    assert!(
        discover(temp.path(), "repo", "localhost", "npm")
            .unwrap()
            .is_empty()
    );
}

#[test]
fn yaml_inline_comment_parser_handles_escaped_backslashes() {
    assert_eq!(
        strip_inline_yaml_comment(r#""apps\\web" # comment"#),
        r#""apps\\web""#
    );
    assert_eq!(
        strip_inline_yaml_comment(r#""apps\"web" # comment"#),
        r#""apps\"web""#
    );
}

#[test]
fn double_star_workspace_globs_skip_node_modules() {
    let temp = tempdir().unwrap();
    fs::write(temp.path().join("package.json"), r#"{"workspaces":["**"]}"#).unwrap();
    fs::create_dir_all(temp.path().join("apps/web")).unwrap();
    fs::write(
        temp.path().join("apps/web/package.json"),
        r#"{"name":"web","scripts":{"dev":"vite"}}"#,
    )
    .unwrap();
    fs::create_dir_all(temp.path().join("node_modules/pkg")).unwrap();
    fs::write(
        temp.path().join("node_modules/pkg/package.json"),
        r#"{"name":"pkg","scripts":{"dev":"vite"}}"#,
    )
    .unwrap();

    let specs = discover(temp.path(), "repo", "localhost", "npm").unwrap();
    assert_eq!(specs.len(), 1);
    assert_eq!(specs[0].name, "web");
}

#[test]
fn double_star_workspace_globs_do_not_include_workspace_root() {
    let temp = tempdir().unwrap();
    fs::write(
        temp.path().join("package.json"),
        r#"{"name":"root","workspaces":["**"],"scripts":{"dev":"vite"}}"#,
    )
    .unwrap();
    fs::create_dir_all(temp.path().join("apps/web")).unwrap();
    fs::write(
        temp.path().join("apps/web/package.json"),
        r#"{"name":"web","scripts":{"dev":"vite"}}"#,
    )
    .unwrap();

    let specs = discover(temp.path(), "repo", "localhost", "npm").unwrap();
    assert_eq!(specs.len(), 1);
    assert_eq!(specs[0].name, "web");
}

#[test]
fn double_star_workspace_globs_do_not_include_current_base() {
    let temp = tempdir().unwrap();
    fs::write(
        temp.path().join("package.json"),
        r#"{"workspaces":["apps/**"]}"#,
    )
    .unwrap();
    fs::create_dir_all(temp.path().join("apps/web")).unwrap();
    fs::write(
        temp.path().join("apps/package.json"),
        r#"{"name":"apps","scripts":{"dev":"vite"}}"#,
    )
    .unwrap();
    fs::write(
        temp.path().join("apps/web/package.json"),
        r#"{"name":"web","scripts":{"dev":"vite"}}"#,
    )
    .unwrap();

    let specs = discover(temp.path(), "repo", "localhost", "npm").unwrap();
    assert_eq!(specs.len(), 1);
    assert_eq!(specs[0].name, "web");
}

#[test]
fn workspace_negation_globs_exclude_matching_packages() {
    let temp = tempdir().unwrap();
    fs::write(
        temp.path().join("package.json"),
        r#"{"workspaces":["apps/*","!apps/private"]}"#,
    )
    .unwrap();
    fs::create_dir_all(temp.path().join("apps/web")).unwrap();
    fs::write(
        temp.path().join("apps/web/package.json"),
        r#"{"name":"web","scripts":{"dev":"vite"}}"#,
    )
    .unwrap();
    fs::create_dir_all(temp.path().join("apps/private")).unwrap();
    fs::write(
        temp.path().join("apps/private/package.json"),
        r#"{"name":"private","scripts":{"dev":"vite"}}"#,
    )
    .unwrap();

    let specs = discover(temp.path(), "repo", "localhost", "npm").unwrap();

    assert_eq!(specs.len(), 1);
    assert_eq!(specs[0].name, "web");
}

#[test]
fn workspace_negation_globs_exclude_descendants() {
    let temp = tempdir().unwrap();
    fs::write(
        temp.path().join("package.json"),
        r#"{"workspaces":["apps/**","!apps/private"]}"#,
    )
    .unwrap();
    fs::create_dir_all(temp.path().join("apps/web")).unwrap();
    fs::write(
        temp.path().join("apps/web/package.json"),
        r#"{"name":"web","scripts":{"dev":"vite"}}"#,
    )
    .unwrap();
    fs::create_dir_all(temp.path().join("apps/private/nested")).unwrap();
    fs::write(
        temp.path().join("apps/private/nested/package.json"),
        r#"{"name":"nested","scripts":{"dev":"vite"}}"#,
    )
    .unwrap();

    let specs = discover(temp.path(), "repo", "localhost", "npm").unwrap();

    assert_eq!(specs.len(), 1);
    assert_eq!(specs[0].name, "web");
}

#[test]
fn workspace_negation_globs_cannot_escape_root() {
    let temp = tempdir().unwrap();
    fs::write(
        temp.path().join("package.json"),
        r#"{"workspaces":["apps/**","!../private"]}"#,
    )
    .unwrap();

    let error = discover(temp.path(), "repo", "localhost", "npm")
        .unwrap_err()
        .to_string();

    assert!(error.contains("must stay within the repo root"));
}

#[cfg(unix)]
#[test]
fn workspace_exclusions_fail_closed_on_broken_symlinks() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().unwrap();
    let broken = temp.path().join("broken");
    symlink(temp.path().join("missing"), &broken).unwrap();

    let error = canonicalize_excluded_paths(&[broken])
        .unwrap_err()
        .to_string();

    assert!(error.contains("Failed to canonicalize workspace exclusion path"));
}

#[test]
fn workspace_config_size_is_capped() {
    let temp = tempdir().unwrap();
    fs::write(
        temp.path().join("package.json"),
        vec![b' '; (MAX_WORKSPACE_FILE_BYTES + 1) as usize],
    )
    .unwrap();

    assert!(!package_json_has_workspaces(temp.path()));
}

#[test]
fn workspace_glob_match_cap_fails_closed() {
    let mut matches = (0..MAX_WORKSPACE_GLOB_MATCHES)
        .map(|index| PathBuf::from(format!("pkg-{index}")))
        .collect::<Vec<_>>();

    let error = push_workspace_match(&mut matches, Path::new("overflow"))
        .unwrap_err()
        .to_string();

    assert!(error.contains("exceeded"));
}

#[test]
fn workspace_positive_globs_cannot_escape_root() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("repo");
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("package.json"),
        r#"{"workspaces":["apps/*","../outside"]}"#,
    )
    .unwrap();

    let error = discover(&root, "repo", "localhost", "npm")
        .unwrap_err()
        .to_string();

    assert!(error.contains("must stay within the repo root"));
}

#[test]
fn workspace_absolute_globs_cannot_escape_root() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("repo");
    fs::create_dir_all(&root).unwrap();
    let absolute = temp.path().join("outside").display().to_string();

    let error = expand_globs(&root, &[absolute]).unwrap_err().to_string();

    assert!(error.contains("must stay within the repo root"));
}

#[test]
fn workspace_globs_return_canonical_paths() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("repo");
    let app = root.join("apps/web");
    fs::create_dir_all(&app).unwrap();
    fs::write(app.join("package.json"), "{}").unwrap();

    let paths = expand_globs(&root, &["apps/*".into()]).unwrap();

    assert_eq!(paths, vec![app.canonicalize().unwrap()]);
}

#[cfg(unix)]
#[test]
fn workspace_globs_skip_symlinked_directories() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().unwrap();
    fs::write(temp.path().join("package.json"), r#"{"workspaces":["**"]}"#).unwrap();
    fs::create_dir_all(temp.path().join("apps/web")).unwrap();
    fs::write(
        temp.path().join("apps/web/package.json"),
        r#"{"name":"web","scripts":{"dev":"vite"}}"#,
    )
    .unwrap();
    symlink(temp.path(), temp.path().join("apps/loop")).unwrap();

    let specs = discover(temp.path(), "repo", "localhost", "npm").unwrap();

    assert_eq!(specs.len(), 1);
    assert_eq!(specs[0].name, "web");
}

#[cfg(unix)]
#[test]
fn workspace_config_reads_reject_symlinks() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().unwrap();
    let target = temp.path().join("target-package.json");
    let link = temp.path().join("package.json");
    fs::write(&target, r#"{"workspaces":["apps/*"]}"#).unwrap();
    symlink(&target, &link).unwrap();

    let error = workspace_globs(temp.path()).unwrap_err().to_string();

    assert!(error.contains("must not be a symlink"));
}

#[test]
fn pnpm_workspace_without_packages_key_is_ignored() {
    let globs = parse_pnpm_workspace("catalog:\n  react: 19\n").unwrap();

    assert!(globs.is_empty());
}

#[test]
fn pnpm_workspace_multiline_flow_packages_are_rejected() {
    let error = parse_pnpm_workspace("packages: [\n  \"apps/*\"\n]\n")
        .unwrap_err()
        .to_string();

    assert!(error.contains("multi-line flow-style"));
}

#[test]
fn pnpm_workspace_scalar_packages_are_rejected() {
    let error = parse_pnpm_workspace("packages: 'apps/*'\n")
        .unwrap_err()
        .to_string();

    assert!(error.contains("unsupported inline packages value"));
}

#[test]
fn pnpm_workspace_mapping_packages_are_rejected() {
    let error = parse_pnpm_workspace("packages:\n  app: apps/*\n")
        .unwrap_err()
        .to_string();

    assert!(error.contains("unsupported non-list packages entry"));
}

#[test]
fn pnpm_workspace_inline_comments_are_ignored() {
    let globs = parse_pnpm_workspace("packages: # workspace globs\n  - 'apps/*' # apps\n").unwrap();

    assert_eq!(globs, vec!["apps/*"]);
}

#[test]
fn workspace_discovery_requires_nonoverlapping_glob_edges() {
    let temp = tempdir().unwrap();
    fs::write(
        temp.path().join("package.json"),
        r#"{"workspaces":["apps/ab*bc"]}"#,
    )
    .unwrap();
    for name in ["abc", "abbc"] {
        let dir = temp.path().join("apps").join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("package.json"),
            serde_json::json!({
                "name": name, "scripts": {"dev": "vite"}
            })
            .to_string(),
        )
        .unwrap();
    }
    let specs = discover(temp.path(), "example", "localhost", "npm").unwrap();
    assert_eq!(
        specs
            .iter()
            .map(|spec| spec.name.as_str())
            .collect::<Vec<_>>(),
        ["abbc"]
    );
}

#[test]
fn proxy_workspace_segments_keep_the_restricted_wildcard_grammar() {
    assert!(segment_matches("pkg-**-ui", "pkg-a-b-ui"));
    assert!(!segment_matches("pkg-*-*-ui", "pkg-a-b-ui"));
}
