use super::*;

pub(super) fn settings_without_context(
    opts: &ProxyRuntimeOptions,
) -> Result<jig_dev_proxy::ProxySettings> {
    let defaults = jig_dev_proxy::ProxySettings::default();
    build_settings(
        opts,
        SettingsDefaults {
            http_port: defaults.http_port,
            https_port: defaults.https_port,
            https: defaults.https,
            http2: defaults.http2,
            lan: defaults.lan,
            tld: defaults.tld,
        },
        |_| Ok(Vec::new()),
    )
}

pub(super) fn settings_existing_state_dir_without_context(
    opts: &ProxyRuntimeOptions,
) -> Result<jig_dev_proxy::ProxySettings> {
    require_existing_state_dir(settings_without_context(opts)?)
}

pub(super) fn service_status_settings_without_context(
    opts: &ProxyRuntimeOptions,
) -> Result<jig_dev_proxy::ProxySettings> {
    settings_without_context(opts)
}

pub(super) struct SettingsDefaults {
    pub(super) http_port: u16,
    pub(super) https_port: Option<u16>,
    pub(super) https: bool,
    pub(super) http2: bool,
    pub(super) lan: bool,
    pub(super) tld: String,
}

pub(super) fn build_settings(
    opts: &ProxyRuntimeOptions,
    defaults: SettingsDefaults,
    additional_dns_names: impl FnOnce(&str) -> Result<Vec<String>>,
) -> Result<jig_dev_proxy::ProxySettings> {
    let tld = opts
        .tld
        .clone()
        .unwrap_or(defaults.tld)
        .to_ascii_lowercase();
    let http_port = opts.http_port.unwrap_or(defaults.http_port);
    let https_port = opts.https_port.or(defaults.https_port);
    crate::context::validate_dev_proxy_settings(
        http_port,
        https_port,
        &tld,
        opts.http_port == Some(0),
    )?;
    let additional_dns_names = additional_dns_names(&tld)?;
    Ok(jig_dev_proxy::ProxySettings {
        state_dir: Some(jig_dev_proxy::resolve_state_dir(opts.state_dir.clone())?),
        http_port,
        https_port,
        https: flag_override(defaults.https, opts.https, opts.no_https),
        http2: flag_override(defaults.http2, opts.http2, opts.no_http2),
        lan: flag_override(defaults.lan, opts.lan, opts.no_lan),
        tld,
        additional_dns_names,
    })
}

pub(super) const fn flag_override(default: bool, enable: bool, disable: bool) -> bool {
    match (enable, disable) {
        (true, false) => true,
        (false, true) => false,
        _ => default,
    }
}
