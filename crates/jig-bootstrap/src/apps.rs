//! Frontend and development-server app declarations in the answers.

use anyhow::Result;
use jig_context::frontend_metadata::resolve_frontend_metadata;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct FrontendApp {
    pub name: String,
    pub dir: String,
    pub coverage_threshold: u32,
    pub kind: String,
    pub role: String,
}

impl<'de> Deserialize<'de> for FrontendApp {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct FrontendAppFields {
            name: String,
            dir: String,
            coverage_threshold: u32,
            #[serde(default)]
            kind: Option<String>,
            #[serde(default)]
            role: Option<String>,
        }

        let fields = FrontendAppFields::deserialize(deserializer)?;
        let metadata = resolve_frontend_metadata(
            &fields.name,
            fields.kind.as_deref(),
            fields.role.as_deref(),
            None,
        );
        Ok(Self {
            name: fields.name,
            dir: fields.dir,
            coverage_threshold: fields.coverage_threshold,
            kind: metadata.kind.into(),
            role: metadata.role.into(),
        })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DevApp {
    pub name: String,
    #[serde(default)]
    pub dir: Option<String>,
    #[serde(default = "default_dev_app_kind")]
    pub kind: String,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub argv: Vec<String>,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default = "default_true")]
    pub proxy: bool,
}

pub(super) fn parse_frontend_app(value: &str) -> Result<FrontendApp, String> {
    let parts = value.split(':').collect::<Vec<_>>();
    if !(3..=5).contains(&parts.len()) {
        return Err("expected <name>:<dir>:<coverage_threshold>[:kind[:role]]".into());
    }

    let coverage_threshold = parts[2]
        .parse::<u32>()
        .map_err(|error| format!("coverage_threshold must be a non-negative integer: {error}"))?;

    let metadata =
        resolve_frontend_metadata(parts[0], parts.get(3).copied(), parts.get(4).copied(), None);
    let app = FrontendApp {
        name: parts[0].to_string(),
        dir: parts[1].to_string(),
        coverage_threshold,
        kind: metadata.kind.to_string(),
        role: metadata.role.to_string(),
    };
    super::answers::validate_frontend_apps(std::slice::from_ref(&app))
        .map_err(|error| error.to_string())?;
    Ok(app)
}

fn default_dev_app_kind() -> String {
    "env-port".into()
}

const fn default_true() -> bool {
    true
}
