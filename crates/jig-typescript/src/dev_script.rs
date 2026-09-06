/// Recognize Vite executable tokens after a caller has removed any path prefix.
#[must_use]
pub fn is_vite_token(token: &str) -> bool {
    token == "vite" || token.starts_with("vite@")
}

/// Heuristically recognize a Vite development command in a package script.
///
/// This preserves the shared discovery/launch heuristic, not full shell parsing:
/// any `build`, `preview`, or `optimize` token after the first Vite token excludes
/// the command. Callers retain their own framework flags and launch policy.
#[must_use]
pub fn script_looks_like_vite(value: &str) -> bool {
    let mut tokens = value
        .split(|ch: char| ch.is_whitespace() || matches!(ch, '&' | '|' | ';' | '(' | ')'))
        .filter_map(normalized_token);
    while let Some(token) = tokens.next() {
        if is_vite_token(token) {
            return !tokens.any(|token| matches!(token, "build" | "preview" | "optimize"));
        }
    }
    false
}

fn normalized_token(token: &str) -> Option<&str> {
    let token = token.trim_matches(['"', '\'']);
    if token.is_empty() {
        return None;
    }
    Some(token.rsplit('/').next().unwrap_or(token))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn development_commands_keep_wrapper_version_and_path_support() {
        for command in [
            "vite",
            "bunx vite",
            "npx vite@latest",
            "pnpm exec vite",
            "cross-env NODE_ENV=dev vite --host 127.0.0.1",
            "'./node_modules/.bin/vite'",
            "echo build && vite",
            "(vite --port 5173)",
        ] {
            assert!(script_looks_like_vite(command), "{command}");
        }
        for command in [
            "",
            "vitest",
            "node server.js",
            "vite build",
            "vite preview",
            "vite optimize",
            "npx vite@latest build",
            "vite build && vite preview",
            "vite && echo build",
        ] {
            assert!(!script_looks_like_vite(command), "{command}");
        }
    }
}
