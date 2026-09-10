use std::collections::HashSet;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

use crate::home_paths::{
    canonical_key, expand_tilde_path, has_tilde_prefix, home_name, home_name_matches,
    is_bare_home_name, prefixed_home, same_path,
};

pub(crate) const CONFIG_DIR_ENV: &str = "CLAUDE_CONFIG_DIR";

pub(crate) mod usage;

pub(crate) struct Homes {
    pub(crate) paths: Vec<PathBuf>,
    pub(crate) current: PathBuf,
    pub(crate) warnings: Vec<String>,
    default: PathBuf,
    current_is_default: bool,
}

#[derive(Clone)]
pub(crate) struct Home {
    pub(crate) path: PathBuf,
    pub(crate) default_config: bool,
}

fn user_home() -> Result<PathBuf> {
    dirs::home_dir().context("Could not determine the user home directory")
}

fn current_home() -> Result<PathBuf> {
    let path = match env::var_os(CONFIG_DIR_ENV).filter(|value| !value.is_empty()) {
        Some(path) => PathBuf::from(path),
        None => user_home()?.join(".claude"),
    };
    std::path::absolute(path).context("Failed to resolve CLAUDE_CONFIG_DIR")
}

pub(crate) fn discover_homes() -> Result<Homes> {
    let root = user_home()?;
    let current = current_home()?;
    let current_is_default = env::var_os(CONFIG_DIR_ENV).is_none_or(|value| value.is_empty());
    let default = root.join(".claude");
    let mut candidates = vec![default.clone(), current.clone()];
    let mut warnings = Vec::new();
    match fs::read_dir(&root) {
        Ok(entries) => {
            for entry in entries {
                match entry {
                    Ok(entry)
                        if entry
                            .file_name()
                            .as_encoded_bytes()
                            .starts_with(b".claude-") =>
                    {
                        candidates.push(entry.path());
                    }
                    Ok(_) => {}
                    Err(error) => {
                        warnings.push(format!("Failed to inspect a Claude home: {error}"))
                    }
                }
            }
        }
        Err(error) => warnings.push(format!("Failed to discover Claude homes: {error}")),
    }
    // Prefer the default home, then use stable path ordering for symlink aliases.
    candidates.sort();
    candidates.dedup();
    candidates.sort_by_key(|path| (path != &default, path.clone()));
    let mut seen = HashSet::new();
    let mut paths = Vec::new();
    for candidate in candidates {
        match fs::metadata(&candidate) {
            Ok(metadata) if metadata.is_dir() => {
                if seen.insert(canonical_key(&candidate)) {
                    paths.push(candidate);
                }
            }
            Ok(_)
                if (candidate == default || candidate == current)
                    && seen.insert(canonical_key(&candidate)) =>
            {
                warnings.push(format!(
                    "Claude configuration home is not a directory: {}",
                    candidate.display()
                ));
            }
            Ok(_) => {}
            Err(error)
                if candidate == default
                    && (current_is_default || current != default)
                    && error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => warnings.push(format!(
                "Failed to inspect Claude home {}: {error}",
                candidate.display()
            )),
        }
    }
    Ok(Homes {
        paths,
        current,
        warnings,
        default,
        current_is_default,
    })
}

pub(crate) fn validate_home(path: &Path) -> Result<Home> {
    if !path.is_dir() {
        bail!(
            "Claude home does not exist or is not a directory: {}",
            path.display()
        );
    }
    let path = path
        .canonicalize()
        .with_context(|| format!("Failed to resolve Claude home {}", path.display()))?;
    Ok(Home {
        path,
        default_config: false,
    })
}

pub(crate) fn resolve_home(input: &Path) -> Result<Home> {
    if input.as_os_str().is_empty() {
        bail!("Claude home must not be empty");
    }
    if input.is_absolute() {
        return validate_home(input);
    }
    if has_tilde_prefix(input) {
        let candidate = expand_tilde_path(input, &user_home()?)
            .expect("tilde prefix was checked before expansion");
        return validate_home(&candidate);
    }
    if !is_bare_home_name(input) {
        return validate_home(input);
    }
    let requested = input.as_os_str();
    let root = user_home()?;
    if requested == "claude" || requested == "default" {
        return Ok(Home {
            path: canonical_key(&root.join(".claude")),
            default_config: true,
        });
    }
    let conventional = prefixed_home(&root, requested, "claude-");
    if conventional.is_dir() {
        return validate_home(&conventional);
    }
    let homes = discover_homes()?;
    let matches = homes
        .paths
        .iter()
        .filter(|path| home_name_matches(path, requested))
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [path] => validate_home(path),
        [_, _, ..] => bail!("Claude home name is ambiguous; pass an explicit path"),
        [] => bail!(
            "Claude home '{}' was not found at {}; use `jig claude homes` to list homes or pass an explicit path",
            requested.to_string_lossy(),
            conventional.display()
        ),
    }
}

fn claude_bin() -> OsString {
    env::var_os("JIG_CLAUDE_BIN").unwrap_or_else(|| "claude".into())
}

impl Homes {
    pub(crate) fn is_current(&self, home: &Home) -> bool {
        home.default_config == self.current_is_default && same_path(&home.path, &self.current)
    }

    pub(crate) fn selections(&self) -> Vec<Home> {
        // Native configuration is a launch mode, available even before Claude creates its directory.
        let mut homes = vec![Home {
            path: self.default.clone(),
            default_config: true,
        }];
        homes.extend(
            self.paths
                .iter()
                .filter(|path| *path != &self.default)
                .map(|path| Home {
                    path: path.clone(),
                    default_config: false,
                }),
        );
        // The same directory with an explicit override uses a different global config file.
        if !self.current_is_default
            && self.current.is_dir()
            && same_path(&self.current, &self.default)
        {
            homes.push(Home {
                path: self.current.clone(),
                default_config: false,
            });
        }
        homes
    }

    pub(crate) fn report(&self) -> Value {
        let homes = self.selections();
        json!({
            "schema_version": 1,
            "ok": true,
            "command": "claude homes",
            "outcome": if self.warnings.is_empty() { "complete" } else { "partial" },
            "current_home": self.current.to_string_lossy(),
            "representation_lossy": self.current.to_str().is_none() || homes.iter().any(|home| home.path.to_str().is_none()),
            "homes": homes.iter().map(|home| json!({
                "name": home_name(&home.path),
                "path": home.path.to_string_lossy(),
                "default_config": home.default_config,
                "current": self.is_current(home),
            })).collect::<Vec<_>>(),
            "warnings": self.warnings,
        })
    }
}

pub(crate) fn dry_run_report(home: &Home, args: &[OsString]) -> Value {
    let bin = claude_bin();
    json!({
        "schema_version": 1,
        "ok": true,
        "command": "claude launch",
        "dry_run": true,
        "home": home.path.to_string_lossy(),
        "config_dir": if home.default_config { None } else { Some(home.path.to_string_lossy()) },
        "claude_bin": bin.to_string_lossy(),
        "args": args.iter().map(|arg| arg.to_string_lossy()).collect::<Vec<_>>(),
        "representation_lossy": home.path.to_str().is_none() || bin.to_str().is_none() || args.iter().any(|arg| arg.to_str().is_none()),
    })
}

pub(crate) fn launch(home: &Home, args: &[OsString]) -> Result<()> {
    let mut command = Command::new(claude_bin());
    command.args(args);
    if home.default_config {
        command.env_remove(CONFIG_DIR_ENV);
    } else {
        command.env(CONFIG_DIR_ENV, &home.path);
    }
    crate::agent_launch::launch(&mut command, "Claude", || {
        "Failed to launch Claude; install claude or set JIG_CLAUDE_BIN".to_owned()
    })
}
