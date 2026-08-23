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

pub(super) fn normalize_package_name(value: &str) -> Result<String> {
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
    let package = package.trim_matches('-').to_string();
    if package.is_empty() {
        bail!("Could not derive a package name from '{value}'");
    }
    Ok(package)
}

pub(super) fn normalize_rust_react_package_name(value: &str) -> Result<String> {
    let mut package = normalize_package_name(value)?;
    if !is_valid_rust_crate_identifier(&package.replace('-', "_")) {
        package = format!("app-{package}");
    }
    if package.len() > RUST_REACT_PACKAGE_STEM_LIMIT {
        bail!(
            "Rust-react repo name normalizes to a {}-byte Cargo package stem, but generated workspaces support at most {RUST_REACT_PACKAGE_STEM_LIMIT} bytes. Shorten --repo-name so Cargo can create the generated '<stem>-test-support' crate artifact (lib<stem>_test_support-<hash>.rmeta) within a filesystem component.",
            package.len()
        );
    }
    Ok(package)
}

pub(super) fn default_go_module(value: &str) -> String {
    let stem = normalize_package_name(value).unwrap_or_else(|_| "app".into());
    let mut module = format!("example.com/{stem}");
    if validate_go_module(&module).is_err() {
        module = format!("example.com/app-{stem}");
    }
    debug_assert!(validate_go_module(&module).is_ok());
    module
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
    if value.split('/').any(|segment| segment.contains("..")) {
        bail!(
            "Invalid --go-module '{value}'. Go module path segments cannot contain consecutive '.' characters"
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
    if domain.starts_with('-')
        || !domain
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '.' | '-'))
    {
        bail!(
            "Invalid --go-module '{value}'. The domain must use lowercase ASCII letters, digits, '.', or '-', and cannot start with '-'"
        );
    }
    for segment in value.split('/') {
        let short = segment.split('.').next().unwrap_or_default();
        if is_windows_reserved_name(short) {
            bail!(
                "Invalid --go-module '{value}'. Path element component '{short}' is not portable"
            );
        }
        if let Some((_, suffix)) = short.rsplit_once('~')
            && !suffix.is_empty()
            && suffix.chars().all(|ch| ch.is_ascii_digit())
        {
            bail!(
                "Invalid --go-module '{value}'. Path elements cannot end with a tilde followed by digits"
            );
        }
    }
    if !has_valid_go_module_version_suffix(value) {
        bail!(
            "Invalid --go-module '{value}'. Major-version suffixes must use /v2 or later without leading zeroes or dots"
        );
    }
    Ok(())
}

fn is_windows_reserved_name(value: &str) -> bool {
    let value = value.to_ascii_lowercase();
    matches!(value.as_str(), "con" | "prn" | "aux" | "nul")
        || value
            .strip_prefix("com")
            .or_else(|| value.strip_prefix("lpt"))
            .is_some_and(
                |suffix| matches!(suffix.as_bytes(), [digit] if (b'1'..=b'9').contains(digit)),
            )
}

fn has_valid_go_module_version_suffix(value: &str) -> bool {
    if value.starts_with("gopkg.in/") {
        let value = value.strip_suffix("-unstable").unwrap_or(value);
        let Some((_, version)) = value.rsplit_once(".v") else {
            return false;
        };
        return !version.is_empty()
            && version.chars().all(|ch| ch.is_ascii_digit())
            && (!version.starts_with('0') || version == "0");
    }

    let segment = value.rsplit('/').next().unwrap_or_default();
    let Some(version) = segment.strip_prefix('v') else {
        return true;
    };
    if version.is_empty() || !version.chars().all(|ch| ch.is_ascii_digit() || ch == '.') {
        return true;
    }
    !version.contains('.')
        && !version.starts_with('0')
        && version != "1"
        && version.chars().all(|ch| ch.is_ascii_digit())
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
    fn derived_go_modules_are_valid_by_construction() {
        for (repo_name, expected) in [
            (".ExampleProject", "example.com/exampleproject"),
            ("ExampleProject.", "example.com/exampleproject"),
            ("CON", "example.com/app-con"),
            ("...", "example.com/app"),
        ] {
            let module = default_go_module(repo_name);
            assert_eq!(module, expected);
            validate_go_module(&module).unwrap();
        }
    }

    #[test]
    fn go_module_rejects_platform_reserved_elements_and_short_names() {
        for module in [
            "example.com/con",
            "example.com/ExampleVault/COM1.txt",
            "example.com/vault-consumer-fixture/foo~1",
        ] {
            let error = validate_go_module(module).unwrap_err().to_string();
            assert!(error.contains("Invalid --go-module"), "{module}: {error}");
        }
    }

    #[test]
    fn go_module_enforces_domain_and_major_version_rules() {
        for module in [
            "Example.com/ExampleProject",
            "example.com/ExampleProject/v1",
            "example.com/ExampleProject/v01",
            "example.com/ExampleProject/v2.0",
        ] {
            let error = validate_go_module(module).unwrap_err().to_string();
            assert!(error.contains("Invalid --go-module"), "{module}: {error}");
        }
        for module in [
            "example.com/ExampleProject/v2",
            "gopkg.in/yaml.v2",
            "gopkg.in/check.v1",
        ] {
            validate_go_module(module).unwrap();
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
    fn package_normalization_applies_rust_identifier_rules_only_to_rust() {
        for name in ["loop", "123-project"] {
            assert_eq!(normalize_package_name(name).unwrap(), name);
            assert_eq!(
                normalize_rust_react_package_name(name).unwrap(),
                format!("app-{name}")
            );
        }
        assert_eq!(
            normalize_package_name("Example Project").unwrap(),
            "example-project"
        );
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
