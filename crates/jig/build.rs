use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TemplatePinPolicy {
    Released,
    Unreleased,
    Unknown,
}

impl TemplatePinPolicy {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Released => "released",
            Self::Unreleased => "unreleased",
            Self::Unknown => "unknown",
        }
    }
}

impl std::fmt::Display for TemplatePinPolicy {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

fn detect_template_pin_policy(manifest_dir: &str) -> TemplatePinPolicy {
    if !is_git_worktree(manifest_dir) {
        if warn_about_git_metadata_without_git(manifest_dir) {
            return TemplatePinPolicy::Unreleased;
        }
        // Published crates do not include .git metadata. Treat them as release
        // artifacts so crates.io installs can still use the version tag.
        return TemplatePinPolicy::Unknown;
    }

    let head_tags = git_head_tags(manifest_dir);
    let clean = git_tree_is_clean(manifest_dir);
    if exact_version_tag(&head_tags) && clean {
        TemplatePinPolicy::Released
    } else {
        warn_ci_about_unreleased_policy(&head_tags, clean);
        TemplatePinPolicy::Unreleased
    }
}

fn emit_template_pin_policy(policy: TemplatePinPolicy) {
    println!(
        "cargo:rustc-env=JIG_BUILD_OFFICIAL_TEMPLATE_PIN={}",
        policy.as_str()
    );
}

fn add_git_rerun_inputs(manifest_dir: &str) {
    for path in [
        Path::new(manifest_dir).join("build.rs"),
        Path::new(manifest_dir).join("Cargo.toml"),
        Path::new(manifest_dir).join("src"),
        Path::new(manifest_dir).join("../../Cargo.toml"),
        Path::new(manifest_dir).join("../../Cargo.lock"),
        Path::new(manifest_dir).join("../../templates"),
    ] {
        if path.exists() {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }

    for git_dir in git_dirs(manifest_dir) {
        for path in [
            git_dir.join("HEAD"),
            git_dir.join("index"),
            git_dir.join("packed-refs"),
            git_dir.join("refs/heads"),
            git_dir.join("refs/tags"),
            git_dir.join("commondir"),
        ] {
            if path.exists() {
                println!("cargo:rerun-if-changed={}", path.display());
            }
        }
    }
}

struct EmbeddedTemplateManifest<'a> {
    root: &'a str,
    output_file: &'a str,
    snapshot_file: &'a str,
    snapshot_dir: &'a str,
    static_name: &'a str,
    entry_type: &'a str,
    from_snapshot_const: &'a str,
    snapshot_comment: &'a str,
}

fn generate_embedded_template_manifest(manifest_dir: &str, manifest: EmbeddedTemplateManifest<'_>) {
    let template_root = Path::new(manifest_dir).join(manifest.root);
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is set by Cargo"));
    let output_path = out_dir.join(manifest.output_file);
    if env::var_os("JIG_EMBEDDED_TEMPLATE_SNAPSHOT").is_some() || !template_root.is_dir() {
        let snapshot = Path::new(manifest_dir).join(manifest.snapshot_file);
        let snapshot_dir = Path::new(manifest_dir).join(manifest.snapshot_dir);
        println!("cargo:rerun-if-changed={}", snapshot.display());
        println!("cargo:rerun-if-changed={}", snapshot_dir.display());
        fs::copy(&snapshot, &output_path).unwrap_or_else(|error| {
            panic!(
                "failed to copy embedded template snapshot {} to {}: {error}",
                snapshot.display(),
                output_path.display()
            )
        });
        return;
    }

    println!("cargo:rerun-if-changed={}", template_root.display());
    let mut templates = Vec::new();
    collect_template_files(&template_root, &template_root, &mut templates);
    templates.sort_by(|left, right| left.0.cmp(&right.0));

    if env::var_os("JIG_REFRESH_EMBEDDED_TEMPLATE_SNAPSHOT").is_some() {
        let snapshot = Path::new(manifest_dir).join(manifest.snapshot_file);
        let snapshot_dir = Path::new(manifest_dir).join(manifest.snapshot_dir);
        replace_snapshot_directory(&snapshot_dir, &templates);
        replace_file(
            &snapshot,
            render_embedded_template_snapshot(&templates, &manifest).as_bytes(),
        );
        println!(
            "cargo:warning=refreshed embedded template snapshot {}",
            snapshot.display()
        );
    }

    let output = render_embedded_template_entries(
        &templates,
        |_, path| format!("include_str!({:?})", path.display().to_string()),
        &manifest,
        false,
    );
    fs::write(&output_path, output).unwrap_or_else(|error| {
        panic!(
            "failed to write embedded template manifest {}: {error}",
            output_path.display()
        )
    });
}

fn generate_embedded_template_manifests(manifest_dir: &str) {
    generate_embedded_template_manifest(
        manifest_dir,
        EmbeddedTemplateManifest {
            root: "../../templates/project",
            output_file: "embedded_templates.rs",
            snapshot_file: "src/bootstrap/embedded_templates_snapshot.rs",
            snapshot_dir: "src/bootstrap/embedded_template_snapshots",
            static_name: "EMBEDDED_TEMPLATE_FILES",
            entry_type: "EmbeddedTemplateFile",
            from_snapshot_const: "EMBEDDED_TEMPLATE_FILES_FROM_SNAPSHOT",
            snapshot_comment: "// Generated from templates/project. Update with JIG_REFRESH_EMBEDDED_TEMPLATE_SNAPSHOT=1 cargo check -p jig-sh.\n",
        },
    );
    generate_embedded_template_manifest(
        manifest_dir,
        EmbeddedTemplateManifest {
            root: "../../templates/scaffolds",
            output_file: "embedded_scaffold_templates.rs",
            snapshot_file: "src/bootstrap/scaffold/embedded_templates_snapshot.rs",
            snapshot_dir: "src/bootstrap/scaffold/embedded_template_snapshots",
            static_name: "EMBEDDED_SCAFFOLD_TEMPLATE_FILES",
            entry_type: "EmbeddedScaffoldTemplateFile",
            from_snapshot_const: "EMBEDDED_SCAFFOLD_TEMPLATE_FILES_FROM_SNAPSHOT",
            snapshot_comment: "// Generated from templates/scaffolds. Update with JIG_REFRESH_EMBEDDED_TEMPLATE_SNAPSHOT=1 cargo check -p jig-sh.\n",
        },
    );
}

fn collect_template_files(root: &Path, current: &Path, templates: &mut Vec<(String, PathBuf)>) {
    println!("cargo:rerun-if-changed={}", current.display());
    let entries = fs::read_dir(current).unwrap_or_else(|error| {
        panic!(
            "failed to read template directory {}: {error}",
            current.display()
        )
    });
    for entry in entries {
        let entry = entry.unwrap_or_else(|error| {
            panic!(
                "failed to read template directory entry in {}: {error}",
                current.display()
            )
        });
        let path = entry.path();
        let file_type = entry.file_type().unwrap_or_else(|error| {
            panic!(
                "failed to inspect template path {}: {error}",
                path.display()
            )
        });
        if file_type.is_dir() {
            collect_template_files(root, &path, templates);
        } else if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(".jinja"))
        {
            println!("cargo:rerun-if-changed={}", path.display());
            let relative = path
                .strip_prefix(root)
                .unwrap_or_else(|error| {
                    panic!(
                        "template path {} was not under {}: {error}",
                        path.display(),
                        root.display()
                    )
                })
                .to_string_lossy()
                .replace('\\', "/");
            templates.push((relative, path));
        }
    }
}

fn render_embedded_template_snapshot(
    templates: &[(String, PathBuf)],
    manifest: &EmbeddedTemplateManifest<'_>,
) -> String {
    let mut output = String::new();
    output.push_str(manifest.snapshot_comment);
    output.push_str(&render_embedded_template_entries(
        templates,
        |relative, _| {
            let snapshot_path = format!("/{}/{}", manifest.snapshot_dir, relative);
            format!("include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), {snapshot_path:?}))")
        },
        manifest,
        true,
    ));
    output
}

fn render_embedded_template_entries(
    templates: &[(String, PathBuf)],
    mut contents_expr: impl FnMut(&str, &Path) -> String,
    manifest: &EmbeddedTemplateManifest<'_>,
    from_snapshot: bool,
) -> String {
    let mut output = String::new();
    // The generated snapshot modules include the same marker for fallback builds,
    // but only the live module marker is used by drift tests.
    writeln!(
        output,
        "#[cfg(test)]\n#[allow(dead_code)]\npub(super) const {}: bool = {};",
        manifest.from_snapshot_const, from_snapshot
    )
    .expect("writing generated template entries to string cannot fail");
    writeln!(
        output,
        "pub(super) static {}: &[{}] = &[",
        manifest.static_name, manifest.entry_type
    )
    .expect("writing generated template entries to string cannot fail");
    for (relative, path) in templates {
        writeln!(
            output,
            "    {} {{\n        relative_path: {relative:?},\n        contents: {},\n    }},",
            manifest.entry_type,
            contents_expr(relative, path),
        )
        .expect("writing generated template entries to string cannot fail");
    }
    output.push_str("];\n");
    output
}

fn replace_file(path: &Path, contents: &[u8]) {
    let tmp_path = path.with_extension(format!("tmp.{}", std::process::id()));
    fs::write(&tmp_path, contents).unwrap_or_else(|error| {
        panic!(
            "failed to write temporary embedded template snapshot {}: {error}",
            tmp_path.display()
        )
    });
    fs::rename(&tmp_path, path).unwrap_or_else(|error| {
        let _ = fs::remove_file(&tmp_path);
        panic!(
            "failed to replace embedded template snapshot {}: {error}",
            path.display()
        )
    });
}

fn git_dirs(manifest_dir: &str) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    for args in [
        ["rev-parse", "--git-dir"],
        ["rev-parse", "--git-common-dir"],
    ] {
        if let Some(path) =
            git_output(manifest_dir, &args).map(|path| absolute_git_path(manifest_dir, path))
            && !dirs.contains(&path)
        {
            dirs.push(path);
        }
    }
    dirs
}

fn absolute_git_path(manifest_dir: &str, path: String) -> PathBuf {
    let path = PathBuf::from(path);
    if path.is_absolute() {
        path
    } else {
        Path::new(manifest_dir).join(path)
    }
}

fn replace_snapshot_directory(path: &Path, templates: &[(String, PathBuf)]) {
    let process_id = std::process::id();
    let staged = path.with_extension(format!("tmp.{process_id}"));
    let displaced = path.with_extension(format!("old.{process_id}"));
    for temporary in [&staged, &displaced] {
        if temporary.exists() {
            fs::remove_dir_all(temporary).unwrap_or_else(|error| {
                panic!(
                    "failed to remove stale embedded template snapshot directory {}: {error}",
                    temporary.display()
                )
            });
        }
    }

    fs::create_dir_all(&staged).unwrap_or_else(|error| {
        panic!(
            "failed to create staged embedded template snapshot directory {}: {error}",
            staged.display()
        )
    });
    for (relative, source) in templates {
        let destination = staged.join(relative);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).unwrap_or_else(|error| {
                panic!(
                    "failed to create embedded template snapshot directory {}: {error}",
                    parent.display()
                )
            });
        }
        fs::copy(source, &destination).unwrap_or_else(|error| {
            panic!(
                "failed to stage embedded template snapshot {} at {}: {error}",
                source.display(),
                destination.display()
            )
        });
    }

    if path.exists() {
        fs::rename(path, &displaced).unwrap_or_else(|error| {
            panic!(
                "failed to preserve prior embedded template snapshot directory {}: {error}",
                path.display()
            )
        });
    }
    if let Err(error) = fs::rename(&staged, path) {
        if displaced.exists() {
            let _ = fs::rename(&displaced, path);
        }
        let _ = fs::remove_dir_all(&staged);
        panic!(
            "failed to publish embedded template snapshot directory {}: {error}",
            path.display()
        );
    }
    if displaced.exists() {
        fs::remove_dir_all(&displaced).unwrap_or_else(|error| {
            panic!(
                "failed to remove prior embedded template snapshot directory {}: {error}",
                displaced.display()
            )
        });
    }
}

fn is_git_worktree(manifest_dir: &str) -> bool {
    git_output(manifest_dir, &["rev-parse", "--is-inside-work-tree"]).as_deref() == Some("true")
}

fn exact_version_tag(head_tags: &[String]) -> bool {
    let version = env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION is set by Cargo");
    let expected_tag = format!("v{version}");
    head_tags.iter().any(|tag| tag == &expected_tag)
}

fn git_head_tags(manifest_dir: &str) -> Vec<String> {
    git_output(manifest_dir, &["tag", "--points-at", "HEAD"])
        .map(|tags| {
            tags.lines()
                .map(str::trim)
                .filter(|tag| !tag.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn warn_ci_about_unreleased_policy(head_tags: &[String], clean: bool) {
    if env::var_os("CI").is_none() {
        return;
    }
    if head_tags.is_empty() {
        println!(
            "cargo:warning=Jig build found no git tags pointing at HEAD; release builds need fetched tags or JIG_ASSUME_RELEASE_BUILD=1 after version/tag validation."
        );
    }
    if !clean {
        println!(
            "cargo:warning=Jig build found tracked working-tree changes; release builds need a clean checkout or JIG_ASSUME_RELEASE_BUILD=1 after version/tag validation."
        );
    }
}

fn warn_about_git_metadata_without_git(manifest_dir: &str) -> bool {
    if find_git_marker(manifest_dir).is_some() {
        println!(
            "cargo:warning=Jig build found .git metadata but could not query git; default template pin policy will be treated as unreleased."
        );
        return true;
    }
    false
}

fn find_git_marker(manifest_dir: &str) -> Option<PathBuf> {
    let mut path = Some(Path::new(manifest_dir));
    while let Some(current) = path {
        let marker = current.join(".git");
        if marker.exists() {
            return Some(marker);
        }
        path = current.parent();
    }
    None
}

fn git_tree_is_clean(manifest_dir: &str) -> bool {
    git_output(
        manifest_dir,
        &["status", "--porcelain", "--untracked-files=no"],
    )
    .is_some_and(|status| status.trim().is_empty())
}

fn git_output(manifest_dir: &str, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(manifest_dir)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn main() {
    println!("cargo:rerun-if-env-changed=JIG_ASSUME_RELEASE_BUILD");
    println!("cargo:rerun-if-env-changed=JIG_EMBEDDED_TEMPLATE_SNAPSHOT");
    println!("cargo:rerun-if-env-changed=JIG_REFRESH_EMBEDDED_TEMPLATE_SNAPSHOT");
    println!("cargo:rerun-if-env-changed=CI");

    let assume_release = env::var_os("JIG_ASSUME_RELEASE_BUILD").is_some();
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by Cargo");
    add_git_rerun_inputs(&manifest_dir);
    generate_embedded_template_manifests(&manifest_dir);

    let detected_policy = detect_template_pin_policy(&manifest_dir);
    let policy = if assume_release {
        if detected_policy != TemplatePinPolicy::Released {
            println!(
                "cargo:warning=JIG_ASSUME_RELEASE_BUILD is overriding Jig's detected {detected_policy} build policy; use this only after version/tag validation."
            );
        }
        TemplatePinPolicy::Released
    } else {
        detected_policy
    };

    emit_template_pin_policy(policy);
}
