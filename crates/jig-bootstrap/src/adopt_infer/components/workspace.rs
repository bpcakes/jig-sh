use std::path::Path;

use globset::{GlobBuilder, GlobMatcher};

use super::super::scan::read_toml_for_inference;

#[derive(Default)]
pub(super) struct CargoWorkspace {
    members: Vec<GlobMatcher>,
    exclusions: Vec<GlobMatcher>,
    membership_known: bool,
}

impl CargoWorkspace {
    pub(super) fn read(root: &Path, warnings: &mut Vec<String>) -> Self {
        let manifest = root.join("Cargo.toml");
        if !manifest.is_file() {
            return Self::default();
        }
        let Some(parsed) = read_toml_for_inference(&manifest, warnings) else {
            return Self::default();
        };
        let Some(workspace) = parsed.get("workspace") else {
            return Self::default();
        };
        let (members, members_known) = patterns(workspace.get("members"), "members", warnings);
        let (exclusions, exclusions_known) =
            patterns(workspace.get("exclude"), "exclude", warnings);
        Self {
            members,
            exclusions,
            membership_known: members_known && exclusions_known,
        }
    }

    pub(super) fn includes(&self, root: &str) -> bool {
        self.membership_known
            && self.members.iter().any(|pattern| pattern.is_match(root))
            && !self.excludes(root)
    }

    pub(super) fn excludes(&self, root: &str) -> bool {
        self.exclusions.iter().any(|pattern| pattern.is_match(root))
    }
}

fn patterns(
    value: Option<&toml::Value>,
    label: &str,
    warnings: &mut Vec<String>,
) -> (Vec<GlobMatcher>, bool) {
    let Some(value) = value else {
        return (Vec::new(), true);
    };
    let Some(items) = value.as_array() else {
        warnings.push(format!(
            "Cargo workspace {label} is not an array; members need review"
        ));
        return (Vec::new(), false);
    };
    let mut known = true;
    let patterns = items
        .iter()
        .filter_map(|item| {
            let Some(pattern) = item.as_str() else {
                warnings.push(format!(
                    "Cargo workspace {label} contains a non-string entry; members need review"
                ));
                known = false;
                return None;
            };
            if jig_typescript::workspace::glob_escapes_root(pattern)
                || pattern.contains(['{', '}', '\\'])
            {
                warnings.push(format!(
                    "unsupported Cargo workspace {label} pattern '{pattern}'; members need review"
                ));
                known = false;
                return None;
            }
            let pattern = pattern.trim_start_matches("./").trim_end_matches('/');
            match GlobBuilder::new(pattern)
                .literal_separator(true)
                .backslash_escape(false)
                .build()
            {
                Ok(glob) => Some(glob.compile_matcher()),
                Err(_) => {
                    warnings.push(format!(
                        "invalid Cargo workspace {label} pattern '{pattern}'; members need review"
                    ));
                    known = false;
                    None
                }
            }
        })
        .collect();
    (patterns, known)
}
