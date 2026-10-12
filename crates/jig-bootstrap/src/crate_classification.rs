use std::path::Path;

const NON_PRODUCTION_SEGMENTS: &[&str] = &[
    "bench", "benches", "example", "examples", "fixture", "fixtures", "test", "tests",
];

pub(super) fn non_production_crate_reason(
    relative_crate_dir: &Path,
    package_name: Option<&str>,
) -> Option<String> {
    for segment in relative_crate_dir
        .components()
        .filter_map(|component| component.as_os_str().to_str().map(str::to_ascii_lowercase))
    {
        if NON_PRODUCTION_SEGMENTS.contains(&segment.as_str()) {
            return Some(format!(
                "crate path contains non-production segment '{segment}'"
            ));
        }
    }

    let package_name = package_name?.to_ascii_lowercase();
    NON_PRODUCTION_SEGMENTS
        .contains(&package_name.as_str())
        .then(|| format!("package name '{package_name}' is non-production"))
}
