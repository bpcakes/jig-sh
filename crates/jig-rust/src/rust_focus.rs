//! Process-free normalization and literal Cargo Nextest invocation policy.
use jig_contract::{CargoImpactContextV1, RustFocusV1, RustNextestConfigV1, RustTargetV1};

pub fn validate_config(config: &RustNextestConfigV1) -> Result<(), &'static str> {
    let path = &config.workspace_manifest;
    if path.is_empty()
        || path.len() > 4096
        || path.starts_with(['/', '-'])
        || path.contains(['\\', '\0', ':'])
        || path
            .split('/')
            .any(|part| part.is_empty() || part == ".." || part == ".")
        || !path.ends_with("Cargo.toml")
    {
        return Err(
            "Rust workspace_manifest must be a portable repository-relative Cargo.toml path",
        );
    }
    validate_context(&config.context)?;
    for value in [&config.cargo_profile, &config.nextest_profile]
        .into_iter()
        .flatten()
    {
        token(value)?;
    }
    Ok(())
}

pub fn normalize_focus(focus: &mut RustFocusV1) -> Result<(), &'static str> {
    match focus {
        RustFocusV1::Automatic { plan_id } => {
            if let Some(id) = plan_id {
                token(id)?;
            }
        }
        RustFocusV1::Explicit {
            packages,
            targets,
            features,
            filter,
        } => {
            if packages.is_empty() || packages.len() > 32 || targets.len() > 32 {
                return Err("Rust focus requires 1..32 packages and at most 32 target selectors");
            }
            for package in packages.iter() {
                token(package)?;
                let Some((name, version)) = package.split_once('@') else {
                    return Err("Rust focus packages require exact name@version selectors");
                };
                if name.is_empty()
                    || version.is_empty()
                    || package.contains(['/', ':', '#', '*'])
                    || version.contains('@')
                {
                    return Err("Rust focus packages require portable name@version selectors");
                }
            }
            packages.sort();
            packages.dedup();
            for target in targets.iter() {
                if let Some(name) = target_name(target) {
                    token(name)?;
                }
            }
            targets.sort();
            targets.dedup();
            if let Some(features) = features {
                let mut context = CargoImpactContextV1 {
                    features: features.features.clone(),
                    all_features: features.all_features,
                    no_default_features: features.no_default_features,
                    ..Default::default()
                };
                validate_context(&context)?;
                context.features.sort();
                context.features.dedup();
                features.features = context.features;
            }
            if filter
                .as_ref()
                .is_some_and(|s| s.is_empty() || s.len() > 4096 || s.contains('\0'))
            {
                return Err("Rust test filter must contain 1..4096 NUL-free bytes");
            }
        }
    }
    Ok(())
}

pub fn validate_context(context: &CargoImpactContextV1) -> Result<(), &'static str> {
    if context
        .target
        .as_ref()
        .is_some_and(|target| target.contains(['/', '\\', ':']))
    {
        return Err(
            "Rust target platform must be a portable target triple, not a target-specification path",
        );
    }
    if context.metadata_format_version != 1
        || context.features.len() > 256
        || context.features.iter().map(String::len).sum::<usize>() > 65536
        || (context.all_features && (context.no_default_features || !context.features.is_empty()))
    {
        return Err("Rust feature context is unsupported or has incompatible feature flags");
    }
    for value in context.features.iter().chain(context.target.iter()) {
        token(value)?;
    }
    Ok(())
}

fn token(value: &str) -> Result<(), &'static str> {
    if value.is_empty()
        || value.len() > 4096
        || value.starts_with('-')
        || value
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || c == ',')
    {
        Err("Rust selector must be a bounded, nonempty literal token, not an option")
    } else {
        Ok(())
    }
}

pub fn target_name(target: &RustTargetV1) -> Option<&str> {
    match target {
        RustTargetV1::Lib {} => None,
        RustTargetV1::Bin { name }
        | RustTargetV1::Test { name }
        | RustTargetV1::Example { name }
        | RustTargetV1::Bench { name } => Some(name),
    }
}

pub fn target_matches(target: &RustTargetV1, name: &str, kinds: &[String]) -> bool {
    let kind = match target {
        RustTargetV1::Lib {} => {
            return kinds.iter().any(|kind| {
                matches!(
                    kind.as_str(),
                    "lib" | "rlib" | "dylib" | "cdylib" | "staticlib" | "proc-macro"
                )
            });
        }
        RustTargetV1::Bin { .. } => "bin",
        RustTargetV1::Test { .. } => "test",
        RustTargetV1::Example { .. } => "example",
        RustTargetV1::Bench { .. } => "bench",
    };
    target_name(target) == Some(name) && kinds.iter().any(|value| value == kind)
}

/// All strings occupy whole argv positions; no shell interprets any value.
pub fn nextest_args(
    config: &RustNextestConfigV1,
    context: &CargoImpactContextV1,
    packages: &[String],
    targets: &[RustTargetV1],
    filter: Option<&str>,
) -> Vec<String> {
    let mut args = vec![
        "nextest".into(),
        "run".into(),
        "--manifest-path".into(),
        config.workspace_manifest.clone(),
        "--no-tests=fail".into(),
    ];
    // Pin the profile default rather than allowing NEXTEST_PROFILE to change test scope.
    if config.nextest_profile.is_none() {
        args.extend(["--profile".into(), "default".into()]);
    }
    if packages.is_empty() {
        args.push("--workspace".into());
    }
    for package in packages {
        args.extend(["--package".into(), package.clone()]);
    }
    if targets.is_empty() {
        args.push("--all-targets".into());
    }
    for target in targets {
        let flag = match target {
            RustTargetV1::Lib {} => "--lib",
            RustTargetV1::Bin { .. } => "--bin",
            RustTargetV1::Test { .. } => "--test",
            RustTargetV1::Example { .. } => "--example",
            RustTargetV1::Bench { .. } => "--bench",
        };
        args.push(flag.into());
        if let Some(name) = target_name(target) {
            args.push(name.into());
        }
    }
    for (flag, value) in [
        ("--target", context.target.as_ref()),
        ("--cargo-profile", config.cargo_profile.as_ref()),
        ("--profile", config.nextest_profile.as_ref()),
    ] {
        if let Some(value) = value {
            args.extend([flag.into(), value.clone()]);
        }
    }
    for feature in &context.features {
        args.extend(["--features".into(), feature.clone()]);
    }
    for (flag, enabled) in [
        ("--no-default-features", context.no_default_features),
        ("--all-features", context.all_features),
        ("--locked", context.locked),
        ("--offline", context.offline),
    ] {
        if enabled {
            args.push(flag.into());
        }
    }
    if let Some(filter) = filter {
        args.extend(["--filter-expr".into(), filter.into()]);
    }
    args
}

#[cfg(test)]
mod tests;
