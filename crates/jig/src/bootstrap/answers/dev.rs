use std::collections::HashSet;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

use super::{
    DevApp, DevSettingsAnswers, FrontendApp, is_safe_frontend_app_name,
    is_supported_frontend_app_kind, validate_frontend_app_dir,
};
use crate::context::{DevConfig, config_app_dirs_match, validate_dev_proxy_settings};

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawDevAnswers {
    pub(super) proxy_port: Option<u16>,
    pub(super) https_port: Option<u16>,
    pub(super) https: Option<bool>,
    pub(super) http2: Option<bool>,
    pub(super) lan: Option<bool>,
    pub(super) tld: Option<String>,
    pub(super) workspace_discovery: Option<bool>,
    pub(super) apps: Option<Vec<DevApp>>,
}

impl RawDevAnswers {
    pub(super) fn into_parts(self) -> (DevSettingsAnswers, Vec<DevApp>) {
        (
            DevSettingsAnswers {
                proxy_port: self.proxy_port,
                https_port: self.https_port,
                https: self.https,
                http2: self.http2,
                lan: self.lan,
                tld: self.tld,
                workspace_discovery: self.workspace_discovery,
            },
            self.apps.unwrap_or_default(),
        )
    }

    pub(super) fn merge_settings(&mut self, settings: &DevSettingsAnswers) {
        super::merge_option(&mut self.proxy_port, settings.proxy_port);
        super::merge_option(&mut self.https_port, settings.https_port);
        super::merge_option(&mut self.https, settings.https);
        super::merge_option(&mut self.http2, settings.http2);
        super::merge_option(&mut self.lan, settings.lan);
        super::merge_option(&mut self.tld, settings.tld.clone());
        super::merge_option(&mut self.workspace_discovery, settings.workspace_discovery);
    }
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ResolvedDevSettings {
    pub(crate) proxy_port: u16,
    pub(crate) https_port: u16,
    https: bool,
    http2: bool,
    lan: bool,
    pub(crate) tld: String,
    workspace_discovery: bool,
}

pub(super) struct ResolvedDevApps {
    pub(super) settings: ResolvedDevSettings,
    pub(super) dev_apps: Vec<DevApp>,
    pub(super) generated_frontend_dev_apps: Vec<FrontendApp>,
}

pub(super) fn resolve(
    frontend_apps: &[FrontendApp],
    raw: Option<RawDevAnswers>,
) -> Result<ResolvedDevApps> {
    let (settings_answers, dev_apps) = raw.map_or_else(
        || (DevSettingsAnswers::default(), Vec::new()),
        RawDevAnswers::into_parts,
    );
    let settings = resolve_settings(Some(&settings_answers))?;
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
    jig_core::validate_dev_app_env_prefixes(
        dev_apps.iter().map(|app| app.name.as_str()).chain(
            generated_frontend_dev_apps
                .iter()
                .map(|app| app.name.as_str()),
        ),
        "dev apps",
    )
    .map_err(anyhow::Error::msg)?;
    Ok(ResolvedDevApps {
        settings,
        dev_apps,
        generated_frontend_dev_apps,
    })
}

pub(crate) fn resolve_settings(
    answers: Option<&DevSettingsAnswers>,
) -> Result<ResolvedDevSettings> {
    let answers = answers.cloned().unwrap_or_default();
    let defaults = DevConfig::default();
    let proxy_port = answers.proxy_port.unwrap_or(defaults.proxy_port);
    let https_port = answers
        .https_port
        .or(defaults.https_port)
        .ok_or_else(|| anyhow::anyhow!("default dev configuration must provide an HTTPS port"))?;
    let tld = answers.tld.unwrap_or(defaults.tld).to_ascii_lowercase();
    validate_dev_proxy_settings(proxy_port, Some(https_port), &tld, false)?;
    Ok(ResolvedDevSettings {
        proxy_port,
        https_port,
        https: answers.https.unwrap_or(defaults.https),
        http2: answers.http2.unwrap_or(defaults.http2),
        lan: answers.lan.unwrap_or(defaults.lan),
        tld,
        workspace_discovery: answers
            .workspace_discovery
            .unwrap_or(defaults.workspace_discovery),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dev_proxy_settings_reject_invalid_ports_and_tlds_during_answer_resolution() {
        let cases = [
            (
                RawDevAnswers {
                    proxy_port: Some(0),
                    ..RawDevAnswers::default()
                },
                "proxy HTTP port must be greater than 0",
            ),
            (
                RawDevAnswers {
                    https_port: Some(0),
                    ..RawDevAnswers::default()
                },
                "proxy HTTPS port must be greater than 0",
            ),
            (
                RawDevAnswers {
                    proxy_port: Some(2443),
                    https_port: Some(2443),
                    ..RawDevAnswers::default()
                },
                "proxy HTTP and HTTPS ports must be different",
            ),
            (
                RawDevAnswers {
                    tld: Some("example.com".into()),
                    ..RawDevAnswers::default()
                },
                "is not allowed",
            ),
            (
                RawDevAnswers {
                    tld: Some("test\nunsafe".into()),
                    ..RawDevAnswers::default()
                },
                "invalid hostname",
            ),
        ];

        for (raw, expected) in cases {
            let error = resolve(&[], Some(raw)).err().unwrap().to_string();
            assert!(
                error.contains(expected),
                "{error:?} did not contain {expected:?}"
            );
        }
    }

    #[test]
    fn dev_proxy_tld_is_rendered_in_runtime_normalized_form() {
        let resolved = resolve(
            &[],
            Some(RawDevAnswers {
                tld: Some("Example.TEST".into()),
                ..RawDevAnswers::default()
            }),
        )
        .unwrap();

        assert_eq!(resolved.settings.tld, "example.test");
    }
}
