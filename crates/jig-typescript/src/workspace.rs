use std::path::{Component, Path};

/// Reject workspace patterns with an absolute path, parent component, or
/// platform path prefix. Callers must strip a leading exclusion marker first.
///
/// This is a lexical check using host-platform path semantics. Callers still
/// own glob grammar, traversal, and canonical/symlink containment checks.
#[must_use]
pub fn glob_escapes_root(glob: &str) -> bool {
    let path = Path::new(glob);
    path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::Prefix(_)))
}

/// Match a complete workspace directory-name segment using literal text and `*`.
///
/// Each star matches zero or more characters. Prefix and suffix matches cannot
/// overlap. Directory traversal, `**` recursion, exclusions, and supported
/// pattern syntax remain the caller's responsibility.
#[must_use]
pub fn segment_matches(pattern: &str, name: &str) -> bool {
    if !pattern.contains('*') {
        return pattern == name;
    }
    let mut parts = pattern.split('*');
    let Some(remaining) = name.strip_prefix(parts.next().expect("split has a first part")) else {
        return false;
    };
    let Some(mut remaining) =
        remaining.strip_suffix(parts.next_back().expect("pattern has a star"))
    else {
        return false;
    };
    for literal in parts {
        let Some((_, rest)) = remaining.split_once(literal) else {
            return false;
        };
        remaining = rest;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexical_containment_rejects_parent_components_without_expanding_globs() {
        for (pattern, expected) in [
            ("apps/*", false),
            ("./apps/**", false),
            ("apps/.../*", false),
            ("apps/..hidden/*", false),
            ("", false),
            (".", false),
            ("..", true),
            ("../apps/*", true),
            ("apps/../web", true),
            ("apps/*/../../web", true),
        ] {
            assert_eq!(glob_escapes_root(pattern), expected, "{pattern}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn lexical_containment_rejects_absolute_unix_paths() {
        assert!(glob_escapes_root("/apps/*"));
        assert!(glob_escapes_root("//apps/**"));
    }

    #[test]
    fn complete_segments_preserve_literal_order_and_nonoverlapping_edges() {
        for (pattern, name, expected) in [
            ("web", "web", true),
            ("web", "web-app", false),
            ("*", "", true),
            ("**", "web", true),
            ("ab*bc", "abc", false),
            ("ab*bc", "abbc", true),
            ("ab*bc", "ab-middle-bc", true),
            ("pkg-*-*-ui", "pkg-a-b-ui", true),
            ("pkg-*-*-ui", "pkg-a-ui", false),
            ("app-**-web", "app-demo-web", true),
            ("*a*b", "b-a", false),
            ("*a*aa", "aa", false),
            ("*a*aa", "aaa", true),
            ("é*界", "é中界", true),
            ("é*é", "é", false),
            ("app?", "app1", false),
            ("[ab]*", "app", false),
            ("", "", true),
            ("", "web", false),
        ] {
            assert_eq!(
                segment_matches(pattern, name),
                expected,
                "{pattern:?} against {name:?}"
            );
        }
    }
}
