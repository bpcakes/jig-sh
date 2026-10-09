//! Version requirements read from bounded authority files such as `go.mod` and `Cargo.toml`.

use std::fs;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use jig_context::RepoContext;

const VERSION_AUTHORITY_MAX_BYTES: u64 = 128;

pub(super) const GO_MODULE_AUTHORITY_MAX_BYTES: u64 = 1024 * 1024;

const CARGO_MANIFEST_AUTHORITY_MAX_BYTES: u64 = 1024 * 1024;

pub(crate) fn go_version_selector(ctx: &RepoContext) -> Result<String> {
    let authority_paths = ctx
        .go_module_authority_paths()
        .context("Could not resolve Go module authority")?;
    let (_, requirement) = select_go_module_version_requirement(&authority_paths)
        .map_err(|error| anyhow!(error.reason))?
        .context("This repository does not declare a Go module authority")?;
    Ok(requirement.selector)
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct NumericVersion {
    pub(super) major: u64,
    pub(super) minor: u64,
    pub(super) patch: u64,
}

impl std::fmt::Display for NumericVersion {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

pub(super) fn parse_numeric_version(
    value: &str,
    allow_v_prefix: bool,
    allow_missing_patch: bool,
) -> Option<NumericVersion> {
    let value = if allow_v_prefix {
        value.strip_prefix('v').unwrap_or(value)
    } else {
        value
    };
    let mut components = value.split('.');
    let major = parse_numeric_version_component(components.next()?)?;
    let minor = parse_numeric_version_component(components.next()?)?;
    let patch = match components.next() {
        Some(patch) => parse_numeric_version_component(patch)?,
        None if allow_missing_patch => 0,
        None => return None,
    };
    components.next().is_none().then_some(NumericVersion {
        major,
        minor,
        patch,
    })
}

fn parse_numeric_version_component(value: &str) -> Option<u64> {
    if value.is_empty()
        || !value.bytes().all(|byte| byte.is_ascii_digit())
        || (value.len() > 1 && value.starts_with('0'))
    {
        return None;
    }
    value.parse().ok()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AuthorityReadError {
    Inspect,
    NotRegular,
    EmptyOrOversized,
    Read,
    InvalidUtf8,
}

fn read_bounded_authority(
    path: &Path,
    max_bytes: u64,
) -> std::result::Result<Option<String>, AuthorityReadError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(AuthorityReadError::Inspect),
    };
    if !metadata.file_type().is_file() {
        return Err(AuthorityReadError::NotRegular);
    }
    if metadata.len() == 0 || metadata.len() > max_bytes {
        return Err(AuthorityReadError::EmptyOrOversized);
    }
    let file = fs::File::open(path).map_err(|_| AuthorityReadError::Read)?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(max_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| AuthorityReadError::Read)?;
    if bytes.is_empty() || bytes.len() as u64 > max_bytes {
        return Err(AuthorityReadError::EmptyOrOversized);
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| AuthorityReadError::InvalidUtf8)
}

pub(super) fn numeric_version_authority(
    path: &Path,
    product: &str,
    allow_missing_patch: bool,
    example: &str,
) -> std::result::Result<Option<NumericVersion>, String> {
    let contents =
        read_bounded_authority(path, VERSION_AUTHORITY_MAX_BYTES).map_err(|error| match error {
            AuthorityReadError::Inspect => format!(
                "Could not inspect the {product} version authority at {}",
                path.display()
            ),
            AuthorityReadError::NotRegular => format!(
                "{product} version authority {} must be a real regular file",
                path.display()
            ),
            AuthorityReadError::EmptyOrOversized => format!(
                "{product} version authority {} must contain exactly one bounded version token",
                path.display()
            ),
            AuthorityReadError::Read => format!(
                "Could not read the {product} version authority at {}",
                path.display()
            ),
            AuthorityReadError::InvalidUtf8 => format!(
                "{product} version authority {} must contain valid UTF-8",
                path.display()
            ),
        })?;
    let Some(contents) = contents else {
        return Ok(None);
    };
    let mut tokens = contents.split_ascii_whitespace();
    let Some(token) = tokens.next() else {
        return Err(format!(
            "{product} version authority {} is empty",
            path.display()
        ));
    };
    if tokens.next().is_some() {
        return Err(format!(
            "{product} version authority {} must contain exactly one version token",
            path.display()
        ));
    }
    parse_numeric_version(token, false, allow_missing_patch)
        .map(Some)
        .ok_or_else(|| {
            format!(
                "{product} version authority {} must contain an exact numeric version such as {example}",
                path.display()
            )
        })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct GoModuleVersionRequirement {
    pub(super) numeric: NumericVersion,
    pub(super) selector: String,
    latest_compatible_patch: bool,
}

#[derive(Debug)]
pub(super) struct GoModuleAuthorityError {
    pub(super) path: PathBuf,
    pub(super) reason: String,
}

#[cfg(test)]
pub(super) fn go_module_version_authority(
    path: &Path,
) -> std::result::Result<Option<NumericVersion>, String> {
    go_module_version_requirement(path)
        .map(|requirement| requirement.map(|requirement| requirement.numeric))
}

fn go_module_version_requirement(
    path: &Path,
) -> std::result::Result<Option<GoModuleVersionRequirement>, String> {
    let contents = read_bounded_authority(path, GO_MODULE_AUTHORITY_MAX_BYTES).map_err(
        |error| match error {
            AuthorityReadError::Inspect => {
                format!("Could not inspect Go module {}", path.display())
            }
            AuthorityReadError::NotRegular => format!(
                "Go module authority {} must be a real regular file",
                path.display()
            ),
            AuthorityReadError::EmptyOrOversized => format!(
                "Go module authority {} must be a non-empty bounded go.mod file",
                path.display()
            ),
            AuthorityReadError::Read => {
                format!("Could not read Go module authority at {}", path.display())
            }
            AuthorityReadError::InvalidUtf8 => format!(
                "Go module authority {} must contain valid UTF-8",
                path.display()
            ),
        },
    )?;
    let Some(contents) = contents else {
        return Ok(None);
    };

    let mut go_version = None;
    let mut toolchain_version = None;
    let mut toolchain_seen = false;
    for raw_line in contents.lines() {
        let line = raw_line.split_once("//").map_or(raw_line, |(code, _)| code);
        let mut tokens = line.split_ascii_whitespace();
        let Some(directive) = tokens.next() else {
            continue;
        };
        if !matches!(directive, "go" | "toolchain") {
            continue;
        }
        let token = tokens.next().ok_or_else(|| {
            format!(
                "Go module authority {} has an empty {directive} directive",
                path.display()
            )
        })?;
        if tokens.next().is_some() {
            return Err(format!(
                "Go module authority {} has an invalid {directive} directive",
                path.display()
            ));
        }
        let token = unquote_go_module_token(token).ok_or_else(|| {
            format!(
                "Go module authority {} has an invalid {directive} version token",
                path.display()
            )
        })?;
        match directive {
            "go" => {
                if go_version.is_some() {
                    return Err(format!(
                        "Go module authority {} declares the go version more than once",
                        path.display()
                    ));
                }
                go_version = Some(GoModuleVersionRequirement {
                    numeric: parse_go_module_version(token, "go", path)?,
                    selector: token.to_owned(),
                    latest_compatible_patch: token.matches('.').count() == 1,
                });
            }
            "toolchain" => {
                if toolchain_seen {
                    return Err(format!(
                        "Go module authority {} declares the toolchain more than once",
                        path.display()
                    ));
                }
                toolchain_seen = true;
                toolchain_version = if token == "default" {
                    None
                } else {
                    let version = token.strip_prefix("go").ok_or_else(|| {
                        format!(
                            "Go module authority {} toolchain must be default or an exact go version such as go1.26.0",
                            path.display()
                        )
                    })?;
                    Some(GoModuleVersionRequirement {
                        numeric: parse_go_module_version(version, "toolchain", path)?,
                        selector: version.to_owned(),
                        latest_compatible_patch: version.matches('.').count() == 1,
                    })
                };
            }
            _ => unreachable!(),
        }
    }

    let go_version = go_version.ok_or_else(|| {
        format!(
            "Go module authority {} must declare one numeric go version such as go 1.26.0",
            path.display()
        )
    })?;
    if let Some(toolchain_version) = toolchain_version {
        if toolchain_version.numeric < go_version.numeric {
            return Err(format!(
                "Go module authority {} declares toolchain {} below required go version {}",
                path.display(),
                toolchain_version.numeric,
                go_version.numeric,
            ));
        }
        Ok(Some(toolchain_version))
    } else {
        Ok(Some(go_version))
    }
}

pub(super) fn select_go_module_version_requirement(
    authority_paths: &[PathBuf],
) -> std::result::Result<Option<(PathBuf, GoModuleVersionRequirement)>, GoModuleAuthorityError> {
    let mut selected = None::<(PathBuf, GoModuleVersionRequirement)>;
    for path in authority_paths {
        let requirement = go_module_version_requirement(path)
            .map_err(|reason| GoModuleAuthorityError {
                path: path.clone(),
                reason,
            })?
            .ok_or_else(|| GoModuleAuthorityError {
                path: path.clone(),
                reason: format!("Go version authority {} is missing", path.display()),
            })?;
        let replace = selected.as_ref().is_none_or(|(_, current)| {
            requirement.numeric > current.numeric
                || (requirement.numeric == current.numeric
                    && requirement.latest_compatible_patch
                    && !current.latest_compatible_patch)
        });
        if replace {
            selected = Some((path.clone(), requirement));
        }
    }
    Ok(selected)
}

fn unquote_go_module_token(token: &str) -> Option<&str> {
    if token.starts_with('"') || token.ends_with('"') {
        token
            .strip_prefix('"')
            .and_then(|token| token.strip_suffix('"'))
            .filter(|token| !token.is_empty())
    } else {
        Some(token)
    }
}

fn parse_go_module_version(
    token: &str,
    directive: &str,
    path: &Path,
) -> std::result::Result<NumericVersion, String> {
    parse_numeric_version(token, false, true).ok_or_else(|| {
        format!(
            "Go module authority {} {directive} directive must use an exact numeric version such as 1.26.0",
            path.display()
        )
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct VersionSeries {
    pub(super) major: u64,
    pub(super) minor: u64,
}

impl VersionSeries {
    pub(super) const fn contains(self, version: NumericVersion) -> bool {
        self.major == version.major && self.minor == version.minor
    }
}

impl std::fmt::Display for VersionSeries {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}.{}", self.major, self.minor)
    }
}

fn root_cargo_manifest(path: &Path) -> std::result::Result<Option<toml::Value>, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(format!("Could not inspect {}", path.display())),
    };
    if !metadata.file_type().is_file() {
        return Err(format!("{} must be a real regular file", path.display()));
    }
    if metadata.len() == 0 || metadata.len() > CARGO_MANIFEST_AUTHORITY_MAX_BYTES {
        return Err(format!(
            "{} must be a non-empty bounded Cargo manifest",
            path.display()
        ));
    }
    let contents = fs::read_to_string(path)
        .map_err(|_| format!("{} must contain valid UTF-8", path.display()))?;
    toml::from_str::<toml::Value>(&contents)
        .map(Some)
        .map_err(|error| format!("Could not parse {}: {error}", path.display()))
}

pub(super) fn cargo_rust_version_authority(
    path: &Path,
) -> std::result::Result<Option<NumericVersion>, String> {
    let Some(manifest) = root_cargo_manifest(path)? else {
        return Ok(None);
    };
    let value = manifest
        .get("workspace")
        .and_then(|workspace| workspace.get("package"))
        .and_then(|package| package.get("rust-version"))
        .or_else(|| {
            manifest
                .get("package")
                .and_then(|package| package.get("rust-version"))
        });
    let Some(value) = value else {
        return Ok(None);
    };
    let version = value.as_str().ok_or_else(|| {
        format!(
            "Rust version authority in {} must be a string",
            path.display()
        )
    })?;
    parse_numeric_version(version, false, true)
        .map(Some)
        .ok_or_else(|| {
            format!(
                "Rust version authority in {} must be numeric, such as 1.94",
                path.display()
            )
        })
}

pub(super) fn cargo_sqlx_version_authority(
    path: &Path,
) -> std::result::Result<Option<VersionSeries>, String> {
    let Some(manifest) = root_cargo_manifest(path)? else {
        return Ok(None);
    };
    let value = manifest
        .get("workspace")
        .and_then(|workspace| workspace.get("dependencies"))
        .and_then(|dependencies| dependencies.get("sqlx"))
        .or_else(|| {
            manifest
                .get("dependencies")
                .and_then(|dependencies| dependencies.get("sqlx"))
        });
    let Some(value) = value else {
        return Ok(None);
    };
    let requirement = value
        .as_str()
        .or_else(|| value.get("version").and_then(toml::Value::as_str))
        .ok_or_else(|| {
            format!(
                "SQLx dependency authority in {} must declare a version",
                path.display()
            )
        })?;
    let requirement = requirement
        .strip_prefix('=')
        .or_else(|| requirement.strip_prefix('^'))
        .unwrap_or(requirement);
    let version = parse_numeric_version(requirement, false, true).ok_or_else(|| {
        format!(
            "SQLx dependency authority in {} must use one numeric minor line, such as 0.9",
            path.display()
        )
    })?;
    Ok(Some(VersionSeries {
        major: version.major,
        minor: version.minor,
    }))
}
