use super::*;

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

pub(super) fn parse_numeric_version_component(value: &str) -> Option<u64> {
    if value.is_empty()
        || !value.bytes().all(|byte| byte.is_ascii_digit())
        || (value.len() > 1 && value.starts_with('0'))
    {
        return None;
    }
    value.parse().ok()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AuthorityReadError {
    Inspect,
    NotRegular,
    EmptyOrOversized,
    Read,
    InvalidUtf8,
}

pub(super) fn read_bounded_authority(
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

pub(super) fn go_module_version_requirement(
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

pub(super) fn unquote_go_module_token(token: &str) -> Option<&str> {
    if token.starts_with('"') || token.ends_with('"') {
        token
            .strip_prefix('"')
            .and_then(|token| token.strip_suffix('"'))
            .filter(|token| !token.is_empty())
    } else {
        Some(token)
    }
}

pub(super) fn parse_go_module_version(
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

pub(super) fn rust_runtime_check(
    ctx: &RepoContext,
    environment: &DoctorEnvironment,
    process_control: DoctorProcessControl<'_>,
) -> Option<DoctorCheck> {
    let authority_path = ctx.root().join("Cargo.toml");
    let required = match cargo_rust_version_authority(&authority_path) {
        Ok(Some(required)) => required,
        Ok(None) => return None,
        Err(reason) => {
            return Some(
                check(
                    "rust_runtime",
                    "Rust runtime",
                    true,
                    false,
                    "invalid authority",
                    reason,
                )
                .with_fix(
                    "Correct the root Cargo.toml rust-version authority, then run `scripts/jig doctor`.",
                )
                .with_data(json!({ "authority": authority_path.display().to_string() })),
            );
        }
    };
    let Some(resolution) = resolve_program(ctx.root(), "rustc", environment.search_path.as_deref())
    else {
        return Some(
            check(
                "rust_runtime",
                "Rust runtime",
                true,
                false,
                "missing",
                format!("Rust {required} or newer is required, but rustc was not found on PATH"),
            )
            .with_fix(&rust_runtime_fix(required))
            .with_data(json!({
                "authority": authority_path.display().to_string(),
                "required": required.to_string(),
                "actual": null,
            })),
        );
    };
    if let Some(reason) = process_control.unavailable_reason {
        return Some(
            check(
                "rust_runtime",
                "Rust runtime",
                true,
                false,
                "unverified",
                format!("Could not verify Rust {required} or newer ({reason})"),
            )
            .with_fix("Run `scripts/jig doctor` again before starting Rust work.")
            .with_data(json!({
                "authority": authority_path.display().to_string(),
                "required": required.to_string(),
                "actual": null,
            })),
        );
    }
    let actual = match probe_rust_version(
        &resolution.path,
        ctx.root(),
        environment,
        process_control.cancellation,
    ) {
        Ok(actual) => actual,
        Err(reason) => {
            return Some(
                check(
                    "rust_runtime",
                    "Rust runtime",
                    true,
                    false,
                    "unverified",
                    format!("Could not verify Rust {required} or newer ({reason})"),
                )
                .with_fix(
                    "Run `rustc --version`, correct the active toolchain, then rerun `scripts/jig doctor`.",
                )
                .with_data(json!({
                    "authority": authority_path.display().to_string(),
                    "required": required.to_string(),
                    "actual": null,
                })),
            );
        }
    };
    let compatible = actual >= required;
    let detail = if compatible {
        format!("Rust {actual} satisfies the required version {required}")
    } else {
        format!("Rust {actual} is active, but this repository requires {required} or newer")
    };
    let check = check(
        "rust_runtime",
        "Rust runtime",
        true,
        compatible,
        if compatible {
            "compatible"
        } else {
            "incompatible"
        },
        detail,
    )
    .with_data(json!({
        "authority": authority_path.display().to_string(),
        "required": required.to_string(),
        "actual": actual.to_string(),
    }));
    Some(if compatible {
        check
    } else {
        check.with_fix(&rust_runtime_fix(required))
    })
}

pub(super) fn go_runtime_check(
    ctx: &RepoContext,
    environment: &DoctorEnvironment,
    process_control: DoctorProcessControl<'_>,
) -> Option<DoctorCheck> {
    let authority_paths = match ctx.go_module_authority_paths() {
        Ok(paths) => paths,
        Err(error) => {
            return Some(
                check(
                    "go_runtime",
                    "Go runtime",
                    true,
                    false,
                    "invalid authority",
                    format!("Could not resolve Go module authority: {error}"),
                )
                .with_fix(
                    "Correct the repository component roots, then rerun `scripts/jig doctor`.",
                )
                .with_data(json!({
                    "authority": "",
                    "authorities": [],
                })),
            );
        }
    };
    if authority_paths.is_empty() {
        return None;
    }
    let authorities = authority_paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>();
    let (authority_path, requirement) = match select_go_module_version_requirement(&authority_paths)
    {
        Ok(Some(selected)) => selected,
        Ok(None) => return None,
        Err(error) => {
            return Some(
                check(
                    "go_runtime",
                    "Go runtime",
                    true,
                    false,
                    "invalid authority",
                    error.reason,
                )
                .with_fix(&format!(
                    "Correct or restore {}, then rerun scripts/jig doctor.",
                    error.path.display()
                ))
                .with_data(json!({
                    "authority": error.path.display().to_string(),
                    "authorities": authorities,
                })),
            );
        }
    };
    let required = requirement.numeric;
    let Some(resolution) = resolve_program(ctx.root(), "go", environment.search_path.as_deref())
    else {
        return Some(
            check(
                "go_runtime",
                "Go runtime",
                true,
                false,
                "missing",
                format!("Go {required} or newer is required, but go was not found on PATH"),
            )
            .with_fix(&go_runtime_fix(required)),
        );
    };
    if let Some(reason) = process_control.unavailable_reason {
        return Some(check(
            "go_runtime",
            "Go runtime",
            true,
            false,
            "unverified",
            format!("Could not verify Go {required} or newer ({reason})"),
        ));
    }
    let actual = match probe_go_version(
        &resolution.path,
        ctx.root(),
        environment,
        process_control.cancellation,
    ) {
        Ok(actual) => actual,
        Err(reason) => {
            return Some(
                check(
                    "go_runtime",
                    "Go runtime",
                    true,
                    false,
                    "unverified",
                    format!("Could not verify Go {required} or newer ({reason})"),
                )
                .with_fix(
                    "Run go version, correct the active toolchain, then rerun scripts/jig doctor.",
                ),
            );
        }
    };
    let compatible = actual >= required;
    let result = check(
        "go_runtime",
        "Go runtime",
        true,
        compatible,
        if compatible {
            "compatible"
        } else {
            "incompatible"
        },
        format!("Go {actual} is active; this repository requires {required} or newer"),
    )
    .with_data(json!({
        "authority": authority_path.display().to_string(),
        "authorities": authorities,
        "required": required.to_string(),
        "actual": actual.to_string(),
    }));
    Some(if compatible {
        result
    } else {
        result.with_fix(&go_runtime_fix(required))
    })
}
