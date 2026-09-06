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
