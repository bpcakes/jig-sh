use super::*;

pub(super) fn normalize_generated_gate_root(value: &str, label: &str) -> Result<String> {
    let normalized = normalize_portable_repo_path(value, label)?;
    if normalized.chars().any(|character| {
        character.is_control() || matches!(character, '*' | '?' | '[' | ']' | '{' | '}')
    }) {
        bail!(
            "{label} '{value}' cannot be represented safely as a literal generated gate path; control characters and glob metacharacters (*, ?, [, ], {{, }}) are unsupported"
        );
    }
    let pattern = if normalized == "." {
        "**".to_string()
    } else {
        format!("{normalized}/**")
    };
    validate_gate_path_pattern("generated-policy", label, &pattern).with_context(|| {
        format!("{label} '{value}' cannot be represented safely as a generated gate path")
    })?;
    Ok(normalized)
}

pub(in crate::bootstrap) fn frontend_gate_key(name: &str) -> String {
    name.to_ascii_lowercase().replace('-', "_")
}
