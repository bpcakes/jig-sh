use std::path::{Component, Path};

use anyhow::{Result, bail};

const POSTGRES_IDENTIFIER_LIMIT: usize = 63;
const DNS_LABEL_LIMIT: usize = 63;
const HASH_SUFFIX_LENGTH: usize = 17;
const RUST_REACT_PACKAGE_STEM_LIMIT: usize = 216;

pub(super) fn bounded_postgres_identifier(value: &str) -> String {
    if value.len() <= POSTGRES_IDENTIFIER_LIMIT {
        return value.to_string();
    }

    let hash = stable_hash(value.as_bytes());
    let max_prefix_bytes = POSTGRES_IDENTIFIER_LIMIT - HASH_SUFFIX_LENGTH;
    let prefix_end = value
        .char_indices()
        .map(|(index, _)| index)
        .take_while(|index| *index <= max_prefix_bytes)
        .last()
        .unwrap_or(0);
    let prefix_end = if value.is_char_boundary(max_prefix_bytes) {
        max_prefix_bytes
    } else {
        prefix_end
    };
    format!("{}_{hash:016x}", &value[..prefix_end])
}

pub(super) fn default_repo_name(destination: &Path) -> String {
    destination
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("app")
        .to_string()
}

pub(super) fn sanitize_package_name(value: &str) -> Result<String> {
    let mut package = String::new();
    let mut previous_dash = false;
    for ch in value.chars() {
        let mapped = if ch.is_ascii_alphanumeric() {
            ch.to_ascii_lowercase()
        } else {
            '-'
        };
        if mapped == '-' {
            if previous_dash {
                continue;
            }
            previous_dash = true;
        } else {
            previous_dash = false;
        }
        package.push(mapped);
    }
    let mut package = package.trim_matches('-').to_string();
    if package.is_empty() {
        bail!("Could not derive a Rust package name from '{value}'");
    }
    if !is_valid_rust_crate_identifier(&package.replace('-', "_")) {
        package = format!("app-{package}");
    }
    Ok(package)
}

pub(super) fn normalize_rust_react_package_name(value: &str) -> Result<String> {
    let package = sanitize_package_name(value)?;
    if package.len() > RUST_REACT_PACKAGE_STEM_LIMIT {
        bail!(
            "Rust-react repo name normalizes to a {}-byte Cargo package stem, but generated workspaces support at most {RUST_REACT_PACKAGE_STEM_LIMIT} bytes. Shorten --repo-name so Cargo can create the generated '<stem>-test-support' crate artifact (lib<stem>_test_support-<hash>.rmeta) within a filesystem component.",
            package.len()
        );
    }
    Ok(package)
}

pub(super) fn validate_go_module(value: &str) -> Result<()> {
    let domain = value.split('/').next().unwrap_or_default();
    if value.is_empty() || value.trim() != value || !value.contains('/') || !domain.contains('.') {
        bail!(
            "Invalid --go-module '{value}'. Use a module path with a domain and repository segment, for example github.com/acme/my-app"
        );
    }
    if value.starts_with('/')
        || value.ends_with('/')
        || value
            .split('/')
            .any(|segment| segment.is_empty() || matches!(segment, "." | ".."))
    {
        bail!(
            "Invalid --go-module '{value}'. Use ASCII module path segments without spaces, empty components, '.', or '..'"
        );
    }
    if value
        .split('/')
        .any(|segment| segment.starts_with('.') || segment.ends_with('.'))
    {
        bail!(
            "Invalid --go-module '{value}'. Go module path segments cannot start or end with '.'"
        );
    }
    if !value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '-' | '_' | '~'))
    {
        bail!(
            "Invalid --go-module '{value}'. Use ASCII letters, digits, '/', '.', '-', '_', or '~'"
        );
    }
    Ok(())
}

pub(super) fn rust_react_repo_dns_label(package_name: &str) -> String {
    if package_name.len() <= DNS_LABEL_LIMIT {
        return package_name.to_string();
    }

    debug_assert!(package_name.is_ascii());
    package_name[..DNS_LABEL_LIMIT]
        .trim_end_matches('-')
        .to_string()
}

fn stable_hash(value: &[u8]) -> u64 {
    value.iter().fold(0xcbf29ce484222325_u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}

pub(super) fn validate_scaffold_name(label: &str, value: &str) -> Result<()> {
    if !value.chars().any(|ch| ch.is_ascii_alphanumeric()) {
        bail!("Scaffold {label} must contain at least one ASCII letter or digit");
    }
    if !value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
    {
        bail!("Scaffold {label} contains unsupported characters: {value}");
    }
    Ok(())
}

fn is_valid_rust_crate_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };

    (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        && !is_rust_keyword(value)
}

fn is_rust_keyword(value: &str) -> bool {
    matches!(
        value,
        "Self"
            | "abstract"
            | "as"
            | "async"
            | "await"
            | "become"
            | "box"
            | "break"
            | "const"
            | "continue"
            | "crate"
            | "do"
            | "dyn"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "final"
            | "fn"
            | "for"
            | "gen"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "macro"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "override"
            | "priv"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "try"
            | "type"
            | "typeof"
            | "union"
            | "unsafe"
            | "unsized"
            | "use"
            | "virtual"
            | "where"
            | "while"
            | "yield"
    )
}

pub(super) fn validate_scaffold_relative_path(label: &str, value: &str) -> Result<()> {
    if value.is_empty() {
        bail!("Scaffold {label} must be a non-empty relative path");
    }
    if !value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '-' | '_'))
    {
        bail!("Scaffold {label} contains unsupported characters: {value}");
    }
    if value.contains("//") {
        bail!("Scaffold {label} must not contain empty path segments: {value}");
    }
    let path = Path::new(value);
    if path.is_absolute() {
        bail!("Scaffold {label} must be relative: {value}");
    }
    for component in path.components() {
        match component {
            Component::Normal(part) if !part.is_empty() => {}
            Component::CurDir | Component::ParentDir => {
                bail!("Scaffold {label} must not contain '.' or '..': {value}");
            }
            _ => bail!("Scaffold {label} must be relative: {value}"),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn go_module_rejects_relative_path_segments() {
        validate_go_module("github.com/acme/demo").unwrap();

        for module in [
            "github.com/acme/./demo",
            "github.com/acme/../demo",
            "github.com/./demo",
        ] {
            let error = validate_go_module(module).unwrap_err().to_string();
            assert!(
                error.contains("empty components, '.', or '..'"),
                "{module}: {error}"
            );
        }
    }

    #[test]
    fn go_module_rejects_dot_edges_and_unsupported_punctuation() {
        for module in [
            "example.com/.ExampleProject",
            "example.com/ExampleProject.",
            "example.com/Example+Project",
        ] {
            let error = validate_go_module(module).unwrap_err().to_string();
            assert!(error.contains("Invalid --go-module"), "{module}: {error}");
        }
    }

    #[test]
    fn rust_react_package_stem_limit_applies_after_normalization() {
        let accepted = format!("1{}", "a".repeat(211));
        let rejected = format!("1{}", "a".repeat(212));

        let normalized = normalize_rust_react_package_name(&accepted).unwrap();
        assert_eq!(normalized.len(), RUST_REACT_PACKAGE_STEM_LIMIT);
        assert!(normalized.starts_with("app-1"));

        let error = normalize_rust_react_package_name(&rejected)
            .unwrap_err()
            .to_string();
        assert!(error.contains("217-byte Cargo package stem"));
        assert!(error.contains("at most 216 bytes"));
        assert!(error.contains("<stem>-test-support"));
        assert!(error.contains("lib<stem>_test_support-<hash>.rmeta"));
    }

    #[test]
    fn rust_react_repo_dns_label_matches_proxy_label_rules() {
        let vectors = [
            ("my-app".to_string(), "my-app".to_string()),
            ("a".repeat(64), "a".repeat(63)),
            (
                format!("{}project", "project-".repeat(26)),
                "project-project-project-project-project-project-project-project".to_string(),
            ),
        ];

        for (input, expected) in vectors {
            let label = rust_react_repo_dns_label(&input);
            assert_eq!(label, expected);
            assert!(label.len() <= DNS_LABEL_LIMIT);
            assert!(
                label
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
            );
        }

        assert_eq!(
            rust_react_repo_dns_label(&format!("{}-", "a".repeat(63))),
            "a".repeat(63)
        );
    }

    #[cfg(feature = "dev-proxy")]
    #[test]
    fn rust_react_repo_dns_label_equals_dev_proxy_output() {
        for package_name in [
            "my-app".to_string(),
            "a".repeat(64),
            format!("{}project", "project-".repeat(26)),
            "a".repeat(RUST_REACT_PACKAGE_STEM_LIMIT),
        ] {
            assert_eq!(
                rust_react_repo_dns_label(&package_name),
                jig_dev_proxy::dns_label(&package_name).unwrap()
            );
        }
    }

    #[test]
    fn postgres_identifiers_are_short_stable_and_collision_resistant() {
        assert_eq!(bounded_postgres_identifier("demo_dev"), "demo_dev");

        let shared_prefix = "project".repeat(12);
        let first = bounded_postgres_identifier(&format!("{shared_prefix}_one_dev"));
        let second = bounded_postgres_identifier(&format!("{shared_prefix}_two_dev"));
        assert_eq!(first.len(), POSTGRES_IDENTIFIER_LIMIT);
        assert_eq!(second.len(), POSTGRES_IDENTIFIER_LIMIT);
        assert_eq!(
            first,
            bounded_postgres_identifier(&format!("{shared_prefix}_one_dev"))
        );
        assert_ne!(first, second);

        let unicode = bounded_postgres_identifier(&"é".repeat(40));
        assert!(unicode.len() <= POSTGRES_IDENTIFIER_LIMIT);
    }
}
