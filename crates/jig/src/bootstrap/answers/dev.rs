use std::collections::{BTreeMap, HashSet};

use anyhow::{Result, bail};
use serde::Deserialize;

use super::{
    DevApp, FrontendApp, is_safe_frontend_app_name, is_supported_frontend_app_kind,
    validate_frontend_app_dir,
};
use crate::context::config_app_dirs_match;

#[derive(Clone, Debug, Default, Deserialize)]
pub(super) struct RawDevAnswers {
    pub(super) apps: Option<Vec<DevApp>>,
}

pub(super) struct ResolvedDevApps {
    pub(super) dev_apps: Vec<DevApp>,
    pub(super) generated_frontend_dev_apps: Vec<FrontendApp>,
}

pub(super) fn resolve(
    frontend_apps: &[FrontendApp],
    raw: Option<RawDevAnswers>,
) -> Result<ResolvedDevApps> {
    let dev_apps = raw.and_then(|dev| dev.apps).unwrap_or_default();
    validate_dev_apps(&dev_apps)?;
    validate_matching_frontend_dev_app_dirs(frontend_apps, &dev_apps)?;
    let generated_frontend_dev_apps: Vec<FrontendApp> = frontend_apps
        .iter()
        .filter(|frontend_app| {
            !dev_apps
                .iter()
                .any(|dev_app| dev_app.name == frontend_app.name)
        })
        .cloned()
        .collect();
    validate_dev_app_env_prefixes(
        dev_apps.iter().map(|app| app.name.as_str()).chain(
            generated_frontend_dev_apps
                .iter()
                .map(|app| app.name.as_str()),
        ),
    )?;
    Ok(ResolvedDevApps {
        dev_apps,
        generated_frontend_dev_apps,
    })
}

fn validate_dev_apps(apps: &[DevApp]) -> Result<()> {
    let mut names = HashSet::new();
    for app in apps {
        if !is_safe_frontend_app_name(&app.name) {
            bail!(
                "Invalid dev app name '{}'. Use ASCII letters, numbers, '-' or '_'.",
                app.name
            );
        }
        if !names.insert(app.name.as_str()) {
            bail!("Duplicate dev app name '{}'", app.name);
        }
        if !is_supported_frontend_app_kind(&app.kind) {
            bail!(
                "Invalid dev app kind '{}'. Expected 'vite' or 'env-port'.",
                app.kind
            );
        }
        if let Some(dir) = &app.dir {
            validate_frontend_app_dir(&app.name, dir)?;
        }
        if app
            .command
            .as_ref()
            .is_some_and(|command| command.trim().is_empty())
        {
            bail!("dev app '{}' command must not be empty", app.name);
        }
        if app.command.is_some() && !app.argv.is_empty() {
            bail!("dev app '{}' must use command or argv, not both", app.name);
        }
        if app.command.is_none() && app.argv.is_empty() {
            bail!("dev app '{}' requires command or argv", app.name);
        }
        if app.port == Some(0) {
            bail!("dev app '{}' port must be greater than 0", app.name);
        }
    }
    Ok(())
}

fn validate_matching_frontend_dev_app_dirs(
    frontend_apps: &[FrontendApp],
    dev_apps: &[DevApp],
) -> Result<()> {
    for frontend_app in frontend_apps {
        let Some(dev_app) = dev_apps.iter().find(|app| app.name == frontend_app.name) else {
            continue;
        };
        match dev_app.dir.as_deref() {
            Some(dev_dir) if config_app_dirs_match(dev_dir, &frontend_app.dir) => {}
            Some(dev_dir) => {
                bail!(
                    "[dev.apps] entry '{}' uses dir '{}' but matching [[frontend_apps]] uses '{}'. Keep them aligned because [dev.apps] takes precedence for scripts/jig dev.",
                    frontend_app.name,
                    dev_dir,
                    frontend_app.dir
                );
            }
            None => {
                bail!(
                    "[dev.apps] entry '{}' matches [[frontend_apps]] and must set dir = '{}' because [dev.apps] takes precedence for scripts/jig dev.",
                    frontend_app.name,
                    frontend_app.dir
                );
            }
        }
    }
    Ok(())
}

fn validate_dev_app_env_prefixes<'a>(names: impl IntoIterator<Item = &'a str>) -> Result<()> {
    let mut prefixes = BTreeMap::new();
    for name in names {
        let prefix = jig_core::dev_app_env_prefix(name);
        if let Some(previous) = prefixes.insert(prefix.clone(), name) {
            bail!(
                "dev apps '{}' and '{}' share derived dev environment prefix {prefix}; rename one app so punctuation-normalized names are unique",
                previous,
                name
            );
        }
    }
    Ok(())
}
