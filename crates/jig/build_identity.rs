use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

const DOMAIN: &[u8] = b"jig-native-build-identity-v3\0";
pub(crate) const FIXED_BUILD_ENVIRONMENT_KEYS: &[&str] = &[
    "TARGET",
    "HOST",
    "PROFILE",
    "OPT_LEVEL",
    "DEBUG",
    "RUSTC",
    "RUSTC_WRAPPER",
    "RUSTC_WORKSPACE_WRAPPER",
    "CARGO_ENCODED_RUSTFLAGS",
    "JIG_EMBEDDED_TEMPLATE_SNAPSHOT",
];

#[derive(Clone, Debug)]
pub(crate) struct BuildSourceLayout {
    root: PathBuf,
    checkout: bool,
}

impl BuildSourceLayout {
    #[allow(dead_code)]
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    #[allow(dead_code)]
    pub(crate) const fn is_checkout(&self) -> bool {
        self.checkout
    }

    pub(crate) fn live_template_root(&self, subdirectory: &str) -> Option<PathBuf> {
        self.checkout
            .then(|| self.root.join("templates").join(subdirectory))
    }
}

pub(crate) fn cargo_rerun_environment_keys(
    build_configuration: &[(String, String)],
) -> Vec<String> {
    let mut keys = FIXED_BUILD_ENVIRONMENT_KEYS
        .iter()
        .map(|key| (*key).to_string())
        .collect::<std::collections::BTreeSet<_>>();
    for (key, _) in build_configuration {
        if !matches!(
            key.as_str(),
            "JIG_BUILD_OFFICIAL_TEMPLATE_PIN" | "RUSTC_VERSION_VERBOSE"
        ) {
            keys.insert(key.clone());
        }
    }
    keys.into_iter().collect()
}

pub(crate) fn configuration_from_environment(
    template_pin_policy: &str,
) -> Result<Vec<(String, String)>, String> {
    let mut configuration = BTreeMap::new();
    for key in FIXED_BUILD_ENVIRONMENT_KEYS {
        if let Some(value) = env::var_os(key) {
            configuration.insert(
                (*key).to_string(),
                value.into_string().map_err(|_| {
                    format!("build configuration environment variable {key} is not UTF-8")
                })?,
            );
        }
    }
    for (key, value) in env::vars_os() {
        let Some(key) = key.to_str() else {
            continue;
        };
        if !key.starts_with("CARGO_CFG_")
            && !key.starts_with("CARGO_FEATURE_")
            && !key.starts_with("CARGO_PKG_")
        {
            continue;
        }
        configuration.insert(
            key.to_string(),
            value.into_string().map_err(|_| {
                format!("build configuration environment variable {key} is not UTF-8")
            })?,
        );
    }
    if let Some(rustc) = configuration.get("RUSTC") {
        let output = Command::new(rustc).arg("-vV").output().map_err(|error| {
            format!("failed to inspect configured Rust compiler {rustc}: {error}")
        })?;
        if !output.status.success() {
            return Err(format!(
                "configured Rust compiler {rustc} -vV failed with status {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        let version = String::from_utf8(output.stdout)
            .map_err(|_| format!("configured Rust compiler {rustc} -vV output is not UTF-8"))?;
        configuration.insert("RUSTC_VERSION_VERBOSE".into(), version.trim().into());
    }
    configuration.insert(
        "JIG_BUILD_OFFICIAL_TEMPLATE_PIN".into(),
        template_pin_policy.into(),
    );
    Ok(configuration.into_iter().collect())
}

pub(crate) fn compute_after_input_refresh(
    layout: &BuildSourceLayout,
    build_configuration: &[(String, String)],
    refresh_inputs: impl FnOnce(&BuildSourceLayout),
) -> Result<String, String> {
    // Validate every current source and snapshot input before a refresh is
    // allowed to mutate checked-in snapshot files. Recollect afterward so the
    // emitted identity covers the refreshed fixed point.
    identity_inputs(layout)?;
    refresh_inputs(layout);
    compute_for_layout(layout, build_configuration)
}

// The library test module exercises direct identity computation, while the
// build-script inclusion uses the refresh-aware entrypoint above.
#[allow(dead_code)]
pub(crate) fn compute(
    manifest_dir: &Path,
    build_configuration: &[(String, String)],
) -> Result<String, String> {
    let layout = resolve_source_layout(manifest_dir)?;
    compute_for_layout(&layout, build_configuration)
}

pub(crate) fn resolve_source_layout(manifest_dir: &Path) -> Result<BuildSourceLayout, String> {
    let manifest_dir = fs::canonicalize(manifest_dir).map_err(|error| {
        format!(
            "failed to resolve Jig manifest directory {}: {error}",
            manifest_dir.display()
        )
    })?;
    if let Some(workspace) = checkout_workspace(&manifest_dir)? {
        Ok(BuildSourceLayout {
            root: workspace,
            checkout: true,
        })
    } else {
        Ok(BuildSourceLayout {
            root: manifest_dir,
            checkout: false,
        })
    }
}

fn compute_for_layout(
    layout: &BuildSourceLayout,
    build_configuration: &[(String, String)],
) -> Result<String, String> {
    let root = &layout.root;
    let mut inputs = identity_inputs(layout)?;
    inputs.sort();
    inputs.dedup();

    let mut digest = Sha256::new();
    digest.update(DOMAIN);
    let mut build_configuration = build_configuration.to_vec();
    build_configuration.sort();
    build_configuration.dedup();
    hash_field(
        &mut digest,
        &(build_configuration.len() as u64).to_be_bytes(),
    );
    for (key, value) in build_configuration {
        hash_field(&mut digest, key.as_bytes());
        hash_field(&mut digest, value.as_bytes());
    }
    for path in inputs {
        let label = path
            .strip_prefix(root)
            .map_err(|_| {
                format!(
                    "build identity input {} escaped root {}",
                    path.display(),
                    root.display()
                )
            })?
            .to_string_lossy();
        let contents = fs::read(&path).map_err(|error| {
            format!(
                "failed to read build identity input {}: {error}",
                path.display()
            )
        })?;
        hash_field(&mut digest, label.as_bytes());
        hash_field(&mut digest, &contents);
    }
    Ok(format!("sha256:{:x}", digest.finalize()))
}

fn identity_inputs(layout: &BuildSourceLayout) -> Result<Vec<PathBuf>, String> {
    let root = &layout.root;
    if layout.checkout {
        let mut inputs = required_inputs(root, &["Cargo.toml", "Cargo.lock"])?;
        collect_files(&root.join("crates"), &mut inputs)?;
        collect_files(&root.join("templates"), &mut inputs)?;
        Ok(inputs)
    } else {
        let mut inputs = required_inputs(root, &["Cargo.toml", "build.rs", "build_identity.rs"])?;
        let lock = root.join("Cargo.lock");
        if let Some(lock) = optional_regular_input(&lock)? {
            inputs.push(lock);
        }
        collect_files(&root.join("src"), &mut inputs)?;
        Ok(inputs)
    }
}

fn checkout_workspace(manifest_dir: &Path) -> Result<Option<PathBuf>, String> {
    let Some(candidate) = manifest_dir.parent().and_then(Path::parent) else {
        return Ok(None);
    };
    let workspace_manifest = candidate.join("Cargo.toml");
    let workspace_lock = candidate.join("Cargo.lock");
    let checkout_package = candidate.join("crates/jig");
    let jig_workspace_sentinels = [
        candidate.join("templates/project/.jig.toml.jinja"),
        candidate.join("crates/jig-core/Cargo.toml"),
        candidate.join("crates/jig-features/Cargo.toml"),
    ];
    if !workspace_manifest.is_file()
        || !workspace_lock.is_file()
        || !checkout_package.is_dir()
        || !candidate.join("templates/scaffolds").is_dir()
        || jig_workspace_sentinels
            .iter()
            .any(|sentinel| !sentinel.is_file())
    {
        return Ok(None);
    }
    let checkout_package = fs::canonicalize(&checkout_package).map_err(|error| {
        format!(
            "failed to resolve checkout Jig package {}: {error}",
            checkout_package.display()
        )
    })?;
    if checkout_package != manifest_dir {
        return Ok(None);
    }
    fs::canonicalize(candidate)
        .map(Some)
        .map_err(|error| format!("failed to resolve Jig workspace: {error}"))
}

fn required_inputs(root: &Path, relative: &[&str]) -> Result<Vec<PathBuf>, String> {
    relative
        .iter()
        .map(|relative| {
            let path = root.join(relative);
            match fs::symlink_metadata(&path) {
                Ok(metadata) if metadata.file_type().is_symlink() => Err(format!(
                    "required build identity input {} must not be a symlink",
                    path.display()
                )),
                Ok(metadata) if metadata.is_file() => Ok(path),
                Ok(_) => Err(format!(
                    "required build identity input {} is not a regular file",
                    path.display()
                )),
                Err(error) => Err(format!(
                    "failed to inspect required build identity input {}: {error}",
                    path.display()
                )),
            }
        })
        .collect()
}

fn optional_regular_input(path: &Path) -> Result<Option<PathBuf>, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(format!(
            "optional build identity input {} must not be a symlink",
            path.display()
        )),
        Ok(metadata) if metadata.is_file() => Ok(Some(path.to_path_buf())),
        Ok(_) => Err(format!(
            "optional build identity input {} is not a regular file",
            path.display()
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!(
            "failed to inspect optional build identity input {}: {error}",
            path.display()
        )),
    }
}

fn collect_files(path: &Path, inputs: &mut Vec<PathBuf>) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        format!(
            "failed to inspect required build identity path {}: {error}",
            path.display()
        )
    })?;
    if metadata.file_type().is_symlink() {
        return Err(format!(
            "required build identity path {} must not be a symlink",
            path.display()
        ));
    }
    if metadata.is_file() {
        inputs.push(path.to_path_buf());
        return Ok(());
    }
    if !metadata.is_dir() {
        return Err(format!(
            "required build identity path {} is not a regular file or directory",
            path.display()
        ));
    }
    let mut entries = fs::read_dir(path)
        .map_err(|error| {
            format!(
                "failed to read build identity directory {}: {error}",
                path.display()
            )
        })?
        .map(|entry| {
            entry.map_err(|error| {
                format!(
                    "failed to read build identity entry in {}: {error}",
                    path.display()
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let file_type = entry.file_type().map_err(|error| {
            format!(
                "failed to inspect build identity input {}: {error}",
                entry.path().display()
            )
        })?;
        if file_type.is_dir() {
            collect_files(&entry.path(), inputs)?;
        } else if file_type.is_file() {
            inputs.push(entry.path());
        } else if file_type.is_symlink() {
            return Err(format!(
                "required build identity path {} must not be a symlink",
                entry.path().display()
            ));
        } else {
            return Err(format!(
                "required build identity path {} is not a regular file or directory",
                entry.path().display()
            ));
        }
    }
    Ok(())
}

fn hash_field(digest: &mut Sha256, value: &[u8]) {
    digest.update((value.len() as u64).to_be_bytes());
    digest.update(value);
}
