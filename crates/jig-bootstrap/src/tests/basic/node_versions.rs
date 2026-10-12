use super::*;

fn numeric_semver_major_minor(version: &str) -> (u64, u64) {
    let components = version
        .split('.')
        .map(|component| component.parse::<u64>().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        components.len(),
        3,
        "generated Node versions must be exact numeric semver pins"
    );
    (components[0], components[1])
}

#[test]
fn generated_node_typings_do_not_exceed_the_runtime_floor() {
    let (runtime_major, runtime_minor) = numeric_semver_major_minor(GENERATED_NODE_VERSION);
    let (types_major, types_minor) = numeric_semver_major_minor(GENERATED_NODE_TYPES_VERSION);

    assert_eq!(
        types_major, runtime_major,
        "generated Node typings must match the runtime major"
    );
    assert!(
        types_minor <= runtime_minor,
        "generated Node typings must not expose APIs newer than the minimum runtime"
    );
}
