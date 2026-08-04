use std::borrow::Cow;
#[cfg(test)]
use std::cell::Cell;
use std::path::Path;

use anyhow::{Context, Result, bail};

use super::template_source::{self, EMBEDDED_TEMPLATE_SOURCE};
use super::{
    BUILD_TEMPLATE_PIN_RELEASED, BUILD_TEMPLATE_PIN_UNRELEASED, OFFICIAL_TEMPLATE_SOURCE,
    REMOTE_TEMPLATE_MODE_ERROR, TemplateMode,
};

#[derive(Debug)]
pub(super) struct InitialTemplateRequest<'a> {
    pub(super) template: &'a str,
    pub(super) vcs_ref: Option<Cow<'a, str>>,
    pub(super) used_default: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum BuildTemplatePinPolicy {
    Released,
    Unreleased,
    Unknown,
}

#[cfg(test)]
thread_local! {
    pub(super) static TEST_BUILD_TEMPLATE_PIN_POLICY: Cell<Option<BuildTemplatePinPolicy>> = const { Cell::new(None) };
}

pub(super) fn resolve_initial_template_request<'a>(
    template: Option<&'a str>,
    vcs_ref: &'a Option<String>,
) -> Result<InitialTemplateRequest<'a>> {
    resolve_initial_template_request_with_policy(
        template,
        vcs_ref,
        current_build_template_pin_policy(),
    )
}

pub(super) fn resolve_initial_template_request_with_policy<'a>(
    template: Option<&'a str>,
    vcs_ref: &'a Option<String>,
    pin_policy: BuildTemplatePinPolicy,
) -> Result<InitialTemplateRequest<'a>> {
    match template {
        Some(template) if is_official_template_source(template) => {
            official_initial_template_request(vcs_ref, pin_policy)
        }
        Some(template) => Ok(InitialTemplateRequest {
            template,
            vcs_ref: vcs_ref.as_deref().map(Cow::Borrowed),
            used_default: false,
        }),
        None => default_initial_template_request(vcs_ref, pin_policy),
    }
}

fn default_initial_template_request(
    vcs_ref: &Option<String>,
    pin_policy: BuildTemplatePinPolicy,
) -> Result<InitialTemplateRequest<'_>> {
    if vcs_ref.is_none() && pin_policy == BuildTemplatePinPolicy::Unreleased {
        // Omitted template on local builds is offline-friendly; explicitly naming
        // the official URL still means "use remote official template code".
        return Ok(InitialTemplateRequest {
            template: EMBEDDED_TEMPLATE_SOURCE,
            vcs_ref: None,
            used_default: true,
        });
    }

    official_initial_template_request(vcs_ref, pin_policy)
}

fn official_initial_template_request(
    vcs_ref: &Option<String>,
    pin_policy: BuildTemplatePinPolicy,
) -> Result<InitialTemplateRequest<'_>> {
    if vcs_ref.is_none() && pin_policy == BuildTemplatePinPolicy::Unreleased {
        bail!(
            "This jig binary was built from unreleased or dirty local source version {}.\nThe default official template pin {} may not match this binary.\nTo render from your checkout, pass --template /path/to/jig-sh --template-mode committed.\nTo use official remote template code, pass --vcs-ref <ref>.",
            env!("CARGO_PKG_VERSION"),
            official_template_ref(),
        );
    }

    Ok(InitialTemplateRequest {
        template: OFFICIAL_TEMPLATE_SOURCE,
        // The release workflow tags the whole workspace as vVERSION. Keep the
        // default template pinned to the installed jig binary's workspace version.
        vcs_ref: Some(
            vcs_ref
                .as_deref()
                .map(Cow::Borrowed)
                .unwrap_or_else(|| Cow::Owned(official_template_ref())),
        ),
        used_default: true,
    })
}

fn current_build_template_pin_policy() -> BuildTemplatePinPolicy {
    #[cfg(test)]
    {
        TEST_BUILD_TEMPLATE_PIN_POLICY
            .with(Cell::get)
            .unwrap_or(BuildTemplatePinPolicy::Released)
    }

    #[cfg(not(test))]
    {
        build_template_pin_policy_from_env(option_env!("JIG_BUILD_OFFICIAL_TEMPLATE_PIN"))
    }
}

pub(super) fn build_template_pin_policy_from_env(value: Option<&str>) -> BuildTemplatePinPolicy {
    match value {
        Some(BUILD_TEMPLATE_PIN_RELEASED) => BuildTemplatePinPolicy::Released,
        Some(BUILD_TEMPLATE_PIN_UNRELEASED) => BuildTemplatePinPolicy::Unreleased,
        // Published crates do not carry .git metadata, so build.rs emits
        // unknown. Missing or unrecognized values keep the same release-pin
        // behavior rather than failing crates.io and packaged installs.
        _ => BuildTemplatePinPolicy::Unknown,
    }
}

pub(super) fn is_official_template_source(template: &str) -> bool {
    canonical_template_source(template) == canonical_template_source(OFFICIAL_TEMPLATE_SOURCE)
}

fn canonical_template_source(template: &str) -> &str {
    template.strip_suffix(".git").unwrap_or(template)
}

pub(super) fn official_template_ref() -> String {
    // The published binary and the template tag share the workspace version.
    official_template_ref_for_version(env!("CARGO_PKG_VERSION"))
}

pub(super) fn official_template_ref_for_version(version: &str) -> String {
    format!("v{version}")
}

pub(super) fn prepare_initial_template_source(
    request: &InitialTemplateRequest<'_>,
    template_mode: Option<TemplateMode>,
    path_base: &Path,
) -> Result<template_source::PreparedTemplateSource> {
    if request.used_default && template_mode.is_some() {
        // Keep local-only mode errors direct; wrapping them as default-source
        // resolution failures would incorrectly suggest a network or tag issue.
        bail!(REMOTE_TEMPLATE_MODE_ERROR);
    }

    let result = template_source::prepare_template_source_from_base(
        request.template,
        template_mode,
        request.vcs_ref.as_deref(),
        path_base,
    );
    if request.used_default {
        result.with_context(|| default_template_failure_context(request))
    } else {
        result
    }
}

pub(super) fn default_template_failure_context(request: &InitialTemplateRequest<'_>) -> String {
    let Some(vcs_ref) = request.vcs_ref.as_deref() else {
        return format!(
            "Failed to resolve the official Jig template {}. For offline use, pass --template <local-path>. To use a specific official ref such as main, pass --vcs-ref <ref>.",
            request.template
        );
    };
    let ref_requirement = if vcs_ref == official_template_ref() {
        "network access and a matching release tag. If this Jig binary was built from a prerelease or development version, that tag may not exist yet"
    } else {
        "network access and the selected ref must exist"
    };
    format!(
        "Failed to resolve the official Jig template {} at {}. The official template requires {}. For offline use, pass --template <local-path>. To use a different official ref such as main, pass --vcs-ref <ref>.",
        request.template, vcs_ref, ref_requirement
    )
}
