pub(crate) mod references;

use std::path::Component;

pub(crate) fn is_ignored_guide_component(component: Component<'_>) -> bool {
    // Agent guide scans ignore repository metadata and Rust build outputs at
    // any depth, including nested submodule metadata and packaged fixture trees
    // under target/package.
    matches!(component, Component::Normal(part) if part == ".git" || part == "target")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::is_ignored_guide_component;

    #[test]
    fn ignored_guide_components_match_only_named_directories() {
        assert!(
            Path::new(".git")
                .components()
                .next()
                .is_some_and(is_ignored_guide_component)
        );
        assert!(
            Path::new("target")
                .components()
                .next()
                .is_some_and(is_ignored_guide_component)
        );
        assert!(
            !Path::new("src")
                .components()
                .next()
                .is_some_and(is_ignored_guide_component)
        );
        assert!(
            !Path::new("/")
                .components()
                .next()
                .is_some_and(is_ignored_guide_component)
        );
    }
}
