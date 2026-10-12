use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

#[path = "build_identity.rs"]
mod build_identity;

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

fn detect_template_pin_policy(
    manifest_dir: &str,
    source_layout: &build_identity::BuildSourceLayout,
) -> TemplatePinPolicy {
    if !source_layout.is_checkout() {
        return TemplatePinPolicy::Unknown;
    }
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

fn emit_display_version(manifest_dir: &str, policy: TemplatePinPolicy) {
    let package_version = env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION is set by Cargo");
    let display_version = if policy == TemplatePinPolicy::Unreleased {
        git_development_provenance(manifest_dir).map_or_else(
            || package_version.clone(),
            |(commit_distance, revision)| {
                build_identity::development_display_version(
                    &package_version,
                    &commit_distance,
                    &revision,
                    !git_tree_is_clean(manifest_dir),
                )
            },
        )
    } else {
        package_version
    };
    println!("cargo:rustc-env=JIG_DISPLAY_VERSION={display_version}");
}

fn add_git_rerun_inputs(manifest_dir: &str, source_layout: &build_identity::BuildSourceLayout) {
    let mut paths = vec![
        Path::new(manifest_dir).join("build.rs"),
        Path::new(manifest_dir).join("build_identity.rs"),
        Path::new(manifest_dir).join("Cargo.toml"),
        Path::new(manifest_dir).join("Cargo.lock"),
        Path::new(manifest_dir).join("src"),
    ];
    if source_layout.is_checkout() {
        paths.extend([
            source_layout.root().join("Cargo.toml"),
            source_layout.root().join("Cargo.lock"),
            source_layout.root().join("crates"),
            source_layout.root().join("templates"),
        ]);
    }
    for path in paths {
        if path.exists() {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }

    if !source_layout.is_checkout() {
        return;
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

fn emit_build_identity(
    source_layout: &build_identity::BuildSourceLayout,
    template_pin_policy: TemplatePinPolicy,
) {
    let configuration =
        build_identity::configuration_from_environment(template_pin_policy.as_str())
            .unwrap_or_else(|error| panic!("failed to collect Jig build configuration: {error}"));
    for key in build_identity::cargo_rerun_environment_keys(&configuration) {
        println!("cargo:rerun-if-env-changed={key}");
    }
    // jig-bootstrap's build script embeds the templates; nothing is refreshed here.
    let identity =
        build_identity::compute_after_input_refresh(source_layout, &configuration, |_| {})
            .unwrap_or_else(|error| panic!("failed to compute Jig native build identity: {error}"));
    println!("cargo:rustc-env=JIG_BUILD_IDENTITY={identity}");
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

fn git_development_provenance(manifest_dir: &str) -> Option<(String, String)> {
    let revision = git_output(manifest_dir, &["rev-parse", "--short=8", "HEAD"])?;
    let nearest_tag = git_output(
        manifest_dir,
        &["describe", "--tags", "--abbrev=0", "--match", "v[0-9]*"],
    );
    let range = nearest_tag
        .as_deref()
        .map_or_else(|| "HEAD".to_string(), |tag| format!("{tag}..HEAD"));
    let commit_distance = git_output(manifest_dir, &["rev-list", "--count", &range])?;
    Some((commit_distance, revision))
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
    println!("cargo:rerun-if-env-changed=CI");

    let assume_release = env::var_os("JIG_ASSUME_RELEASE_BUILD").is_some();
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by Cargo");
    let source_layout = build_identity::resolve_source_layout(Path::new(&manifest_dir))
        .unwrap_or_else(|error| panic!("failed to resolve Jig build source layout: {error}"));
    add_git_rerun_inputs(&manifest_dir, &source_layout);
    let detected_policy = detect_template_pin_policy(&manifest_dir, &source_layout);
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

    emit_build_identity(&source_layout, policy);
    emit_display_version(&manifest_dir, policy);
    emit_template_pin_policy(policy);
}
