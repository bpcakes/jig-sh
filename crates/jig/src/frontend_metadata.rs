pub(crate) const DEFAULT_FRONTEND_APP_KIND: &str = "vite";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ResolvedFrontendMetadata<'a> {
    pub(crate) kind: &'a str,
    pub(crate) role: &'a str,
}

pub(crate) fn default_frontend_app_role_for_kind(kind: &str) -> &'static str {
    if kind == "env-port" { "astro" } else { "spa" }
}

/// Resolves metadata while retaining the distinction between omitted and
/// explicitly configured fields. Older generated repositories only recorded
/// the matching dev-app kind, while old admin entries relied on their name.
pub(crate) fn resolve_frontend_metadata<'a>(
    name: &str,
    configured_kind: Option<&'a str>,
    configured_role: Option<&'a str>,
    matching_dev_kind: Option<&'a str>,
) -> ResolvedFrontendMetadata<'a> {
    let kind = configured_kind
        .or(matching_dev_kind)
        .unwrap_or(DEFAULT_FRONTEND_APP_KIND);
    let role = configured_role.unwrap_or_else(|| {
        if kind == "env-port" {
            default_frontend_app_role_for_kind(kind)
        } else if matches!(name, "admin" | "admin-panel") {
            "admin"
        } else {
            default_frontend_app_role_for_kind(kind)
        }
    });

    ResolvedFrontendMetadata { kind, role }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_values_win_over_legacy_inference() {
        assert_eq!(
            resolve_frontend_metadata("marketing", Some("vite"), Some("admin"), Some("env-port")),
            ResolvedFrontendMetadata {
                kind: "vite",
                role: "admin"
            }
        );
        assert_eq!(
            resolve_frontend_metadata("marketing", Some("vite"), None, Some("env-port")),
            ResolvedFrontendMetadata {
                kind: "vite",
                role: "spa"
            }
        );
        assert_eq!(
            resolve_frontend_metadata("admin-panel", Some("vite"), None, None),
            ResolvedFrontendMetadata {
                kind: "vite",
                role: "admin"
            }
        );
    }

    #[test]
    fn omitted_values_recover_exact_dev_kind_then_legacy_admin() {
        assert_eq!(
            resolve_frontend_metadata("docs", None, None, Some("env-port")),
            ResolvedFrontendMetadata {
                kind: "env-port",
                role: "astro"
            }
        );
        assert_eq!(
            resolve_frontend_metadata("admin-panel", None, None, Some("vite")),
            ResolvedFrontendMetadata {
                kind: "vite",
                role: "admin"
            }
        );
        assert_eq!(
            resolve_frontend_metadata("web", None, None, None),
            ResolvedFrontendMetadata {
                kind: "vite",
                role: "spa"
            }
        );
    }
}
