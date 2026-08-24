use jig_contract::{
    FeatureContext, FeatureDescriptor, NativeToolDescriptor, NativeToolKind,
    RepositoryAdapterDescriptor,
};

// Feature order is not contractual; set-like public results sort and dedup.
const FEATURES: &[FeatureDescriptor] = &[
    jig_core::FEATURE,
    jig_go::FEATURE,
    jig_rust::FEATURE,
    jig_sqlx::FEATURE,
    jig_typescript::FEATURE,
];

pub fn supported_command_keys() -> Vec<&'static str> {
    let mut keys = FEATURES
        .iter()
        .flat_map(|feature| feature.command_keys.iter().copied())
        .collect::<Vec<_>>();
    keys.sort_unstable();
    keys.dedup();
    keys
}

pub fn is_supported_command_key(key: &str) -> bool {
    FEATURES
        .iter()
        .any(|feature| feature.command_keys.contains(&key))
}

pub fn is_supported_native_tool(tool_name: &str) -> bool {
    native_tool(tool_name).is_some()
}

pub fn native_tool_requires_name(tool_name: &str) -> bool {
    native_tool(tool_name)
        .map(|tool| tool.requires_name)
        .unwrap_or(false)
}

pub fn native_tool_kind(tool_name: &str) -> Option<NativeToolKind> {
    native_tool(tool_name).map(|tool| tool.kind)
}

pub fn required_contract_tools(ctx: &dyn FeatureContext) -> Vec<&'static str> {
    let mut required = FEATURES
        .iter()
        .flat_map(|feature| (feature.required_tools)(ctx))
        .collect::<Vec<_>>();
    required.sort_unstable();
    required.dedup();
    required
}

pub fn unavailable_tool_message(ctx: &dyn FeatureContext, tool_name: &str) -> Option<String> {
    FEATURES
        .iter()
        .find_map(|feature| (feature.unavailable_tool_message)(ctx, tool_name))
}

pub fn repository_adapters() -> Vec<&'static RepositoryAdapterDescriptor> {
    let mut adapters = FEATURES
        .iter()
        .flat_map(|feature| feature.repository_adapters.iter())
        .collect::<Vec<_>>();
    adapters.sort_unstable_by_key(|adapter| adapter.id);
    adapters
}

pub fn repository_adapter(id: &str) -> Option<&'static RepositoryAdapterDescriptor> {
    FEATURES
        .iter()
        .flat_map(|feature| feature.repository_adapters.iter())
        .find(|adapter| adapter.id == id)
}

pub fn tool_admission_error(ctx: &dyn FeatureContext, tool_name: &str) -> Option<String> {
    FEATURES
        .iter()
        .find_map(|feature| (feature.tool_admission_error)(ctx, tool_name))
}

fn native_tool(tool_name: &str) -> Option<&'static NativeToolDescriptor> {
    FEATURES
        .iter()
        .flat_map(|feature| feature.native_tools.iter())
        .find(|tool| tool.name == tool_name)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use jig_contract::tool;

    #[test]
    fn registry_exposes_feature_command_and_native_tool_keys() {
        assert!(is_supported_command_key("typescript_lint_command"));
        assert!(is_supported_command_key("sqlx_check_command"));
        assert!(is_supported_native_tool(tool::CONTRACT_CHECK));
        assert!(is_supported_native_tool(tool::MIGRATION_ADD));
        assert!(native_tool_requires_name(tool::MIGRATION_ADD));
        assert!(!native_tool_requires_name(tool::CONTRACT_CHECK));
        assert_eq!(
            native_tool_kind(tool::CONTRACT_CHECK),
            Some(NativeToolKind::ContractCheck)
        );
    }

    #[test]
    fn registry_command_and_native_tool_keys_are_unique() {
        let mut command_keys = HashSet::new();
        let mut native_tools = HashSet::new();
        let mut repository_adapters = HashSet::new();

        for feature in FEATURES {
            for key in feature.command_keys {
                assert!(command_keys.insert(*key), "duplicate command key: {key}");
            }
            for tool in feature.native_tools {
                assert!(
                    native_tools.insert(tool.name),
                    "duplicate native tool: {}",
                    tool.name
                );
            }
            for adapter in feature.repository_adapters {
                assert!(
                    repository_adapters.insert(adapter.id),
                    "duplicate repository adapter: {}",
                    adapter.id
                );
                let mut action_ids = HashSet::new();
                for action in adapter.actions {
                    assert!(
                        action_ids.insert(action.id),
                        "duplicate action '{}' in repository adapter '{}'",
                        action.id,
                        adapter.id
                    );
                }
            }
        }
    }

    #[test]
    fn registry_exposes_stack_adapters_and_component_actions() {
        assert_eq!(
            repository_adapters()
                .into_iter()
                .map(|adapter| adapter.id)
                .collect::<Vec<_>>(),
            vec!["go", "go-postgres", "jig", "rust", "sqlx", "typescript"]
        );
        let go = repository_adapter("go").unwrap();
        assert!(go.actions.iter().any(|action| action.id == "test"));
        let typescript = repository_adapter("typescript").unwrap();
        assert!(
            typescript
                .actions
                .iter()
                .any(|action| action.id == "typecheck")
        );
    }

    #[test]
    fn supported_command_keys_matches_registered_feature_keys() {
        let mut expected = FEATURES
            .iter()
            .flat_map(|feature| feature.command_keys.iter().copied())
            .collect::<Vec<_>>();
        expected.sort_unstable();
        expected.dedup();

        assert_eq!(supported_command_keys(), expected);
    }
}
