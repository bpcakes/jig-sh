use jig_contract::{FeatureContext, FeatureDescriptor, NativeToolDescriptor, NativeToolKind, tool};

const BOOTSTRAP_COMMAND: &str = "bootstrap_command";
const COMMAND_KEYS: &[&str] = &[BOOTSTRAP_COMMAND, "contract_check_command"];
const NATIVE_TOOLS: &[NativeToolDescriptor] = &[
    NativeToolDescriptor::new(tool::CONTRACT_CHECK, false, NativeToolKind::ContractCheck),
    NativeToolDescriptor::new(tool::MIGRATION_ADD, true, NativeToolKind::MigrationAdd),
];

pub const FEATURE: FeatureDescriptor = FeatureDescriptor::new(
    COMMAND_KEYS,
    NATIVE_TOOLS,
    required_tools,
    no_unavailable_tool_message,
);

fn required_tools(ctx: &dyn FeatureContext) -> Vec<&'static str> {
    let mut required = vec![tool::CONTRACT_CHECK];
    if ctx.contract_version() >= 2 || ctx.has_required_command(BOOTSTRAP_COMMAND) {
        required.push(tool::BOOTSTRAP);
    }
    if ctx.migration_authoring_enabled() {
        required.push(tool::MIGRATION_ADD);
    }
    required
}

fn no_unavailable_tool_message(ctx: &dyn FeatureContext, tool_name: &str) -> Option<String> {
    (tool_name == tool::MIGRATION_ADD && !ctx.migration_authoring_enabled()).then(|| {
        format!(
            "{tool_name} is not available because this repository has no configured migration backend. Enable SQLx or Go/PostgreSQL migrations, then run `jig update --recopy`, or remove this command/gate."
        )
    })
}

pub fn dev_app_env_prefix(name: &str) -> String {
    let mut prefix = String::from("JIG_DEV_");
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            prefix.push(ch.to_ascii_uppercase());
        } else {
            prefix.push('_');
        }
    }
    prefix
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct StubContext {
        contract_version: u32,
        required_commands: Vec<String>,
    }

    impl FeatureContext for StubContext {
        fn contract_version(&self) -> u32 {
            self.contract_version
        }

        fn required_commands(&self) -> &[String] {
            &self.required_commands
        }

        fn migration_authoring_enabled(&self) -> bool {
            false
        }

        fn sqlx_enabled(&self) -> bool {
            false
        }

        fn schema_dump_enabled(&self) -> bool {
            false
        }

        fn frontend_app_count(&self) -> usize {
            0
        }
    }

    #[test]
    fn core_contract_check_is_always_required() {
        let ctx = StubContext {
            contract_version: 1,
            ..StubContext::default()
        };

        assert_eq!((FEATURE.required_tools)(&ctx), vec![tool::CONTRACT_CHECK]);
    }

    #[test]
    fn core_bootstrap_is_required_for_command_backed_contracts() {
        let ctx = StubContext {
            contract_version: 2,
            ..StubContext::default()
        };

        assert_eq!(
            (FEATURE.required_tools)(&ctx),
            vec![tool::CONTRACT_CHECK, tool::BOOTSTRAP]
        );
    }

    #[test]
    fn core_bootstrap_is_required_when_command_is_declared() {
        let ctx = StubContext {
            required_commands: vec![BOOTSTRAP_COMMAND.to_string()],
            ..StubContext::default()
        };

        assert_eq!(
            (FEATURE.required_tools)(&ctx),
            vec![tool::CONTRACT_CHECK, tool::BOOTSTRAP]
        );
    }

    #[test]
    fn core_migration_tool_is_required_for_any_migration_backend() {
        struct MigrationContext(StubContext);

        impl FeatureContext for MigrationContext {
            fn contract_version(&self) -> u32 {
                self.0.contract_version()
            }

            fn required_commands(&self) -> &[String] {
                self.0.required_commands()
            }

            fn migration_authoring_enabled(&self) -> bool {
                true
            }

            fn sqlx_enabled(&self) -> bool {
                self.0.sqlx_enabled()
            }

            fn schema_dump_enabled(&self) -> bool {
                self.0.schema_dump_enabled()
            }

            fn frontend_app_count(&self) -> usize {
                self.0.frontend_app_count()
            }
        }

        assert_eq!(
            (FEATURE.required_tools)(&MigrationContext(StubContext::default())),
            vec![tool::CONTRACT_CHECK, tool::MIGRATION_ADD]
        );
    }

    #[test]
    fn dev_app_env_prefix_normalizes_punctuation() {
        assert_eq!(dev_app_env_prefix("api"), "JIG_DEV_API");
        assert_eq!(dev_app_env_prefix("web-app"), "JIG_DEV_WEB_APP");
        assert_eq!(dev_app_env_prefix("web_app"), "JIG_DEV_WEB_APP");
    }
}
