//! Generates the template tables `jig init`, `adopt`, and `update` render from
//! when no `--template` is given. In a Jig source checkout they embed the live
//! `templates/` tree; packaged builds embed the checked-in snapshot. Release
//! identity (the official template pin and display version) belongs to the
//! `jig-sh` build script.

use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=JIG_EMBEDDED_TEMPLATE_SNAPSHOT");
    println!("cargo:rerun-if-env-changed=JIG_REFRESH_EMBEDDED_TEMPLATE_SNAPSHOT");
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by Cargo");
    let checkout_root = checkout_root(Path::new(&manifest_dir));
    generate_embedded_template_manifests(&manifest_dir, checkout_root.as_deref());
}

/// The Jig source checkout containing this package, if it is one. Packaged
/// builds have no surrounding workspace and fall back to the snapshot.
fn checkout_root(manifest_dir: &Path) -> Option<PathBuf> {
    let manifest_dir = fs::canonicalize(manifest_dir).ok()?;
    let candidate = manifest_dir.parent()?.parent()?;
    let is_checkout = candidate.join("Cargo.toml").is_file()
        && candidate
            .join("templates/project/.jig.toml.jinja")
            .is_file()
        && candidate.join("templates/scaffolds").is_dir()
        && fs::canonicalize(candidate.join("crates/jig-bootstrap")).ok()? == manifest_dir;
    is_checkout.then(|| candidate.to_path_buf())
}

struct EmbeddedTemplateManifest<'a> {
    template_subdirectory: &'a str,
    output_file: &'a str,
    snapshot_file: &'a str,
    snapshot_dir: &'a str,
    static_name: &'a str,
    entry_type: &'a str,
    from_snapshot_const: &'a str,
    snapshot_comment: &'a str,
}

fn generate_embedded_template_manifest(
    manifest_dir: &str,
    checkout_root: Option<&Path>,
    manifest: EmbeddedTemplateManifest<'_>,
) {
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is set by Cargo"));
    let output_path = out_dir.join(manifest.output_file);
    let template_root =
        checkout_root.map(|root| root.join("templates").join(manifest.template_subdirectory));
    if env::var_os("JIG_EMBEDDED_TEMPLATE_SNAPSHOT").is_some() || template_root.is_none() {
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
    let template_root = template_root.expect("live template root was checked above");
    assert!(
        template_root.is_dir(),
        "validated Jig checkout template root {} is missing",
        template_root.display()
    );

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

fn generate_embedded_template_manifests(manifest_dir: &str, checkout_root: Option<&Path>) {
    generate_embedded_template_manifest(
        manifest_dir,
        checkout_root,
        EmbeddedTemplateManifest {
            template_subdirectory: "project",
            output_file: "embedded_templates.rs",
            snapshot_file: "src/embedded_templates_snapshot.rs",
            snapshot_dir: "src/embedded_template_snapshots",
            static_name: "EMBEDDED_TEMPLATE_FILES",
            entry_type: "EmbeddedTemplateFile",
            from_snapshot_const: "EMBEDDED_TEMPLATE_FILES_FROM_SNAPSHOT",
            snapshot_comment: "// Generated from templates/project. Update with JIG_REFRESH_EMBEDDED_TEMPLATE_SNAPSHOT=1 cargo check -p jig-bootstrap.\n",
        },
    );
    generate_embedded_template_manifest(
        manifest_dir,
        checkout_root,
        EmbeddedTemplateManifest {
            template_subdirectory: "scaffolds",
            output_file: "embedded_scaffold_templates.rs",
            snapshot_file: "src/scaffold/embedded_templates_snapshot.rs",
            snapshot_dir: "src/scaffold/embedded_template_snapshots",
            static_name: "EMBEDDED_SCAFFOLD_TEMPLATE_FILES",
            entry_type: "EmbeddedScaffoldTemplateFile",
            from_snapshot_const: "EMBEDDED_SCAFFOLD_TEMPLATE_FILES_FROM_SNAPSHOT",
            snapshot_comment: "// Generated from templates/scaffolds. Update with JIG_REFRESH_EMBEDDED_TEMPLATE_SNAPSHOT=1 cargo check -p jig-bootstrap.\n",
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
