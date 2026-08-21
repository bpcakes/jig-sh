use std::{collections::BTreeMap, error::Error, fmt, str::FromStr};

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, de};

const MAX_IDENTIFIER_BYTES: usize = 64;

/// Why a resolved contract field has its current value.
#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum FieldProvenance {
    Declared,
    Inferred,
    Inherited,
    Overridden,
}

/// A stable component identifier such as `api` or `web-app`.
#[derive(Clone, Debug, Eq, Hash, JsonSchema, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ComponentId(String);

/// A stable action identifier such as `test` or `format-check`.
#[derive(Clone, Debug, Eq, Hash, JsonSchema, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ActionId(String);

/// A stable profile identifier such as `verify` or `ci`.
#[derive(Clone, Debug, Eq, Hash, JsonSchema, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ProfileId(String);

macro_rules! identifier {
    ($type:ident, $kind:literal) => {
        impl $type {
            /// Parses and validates an identifier.
            ///
            /// # Errors
            ///
            /// Returns an error when the value is empty, longer than 64 bytes,
            /// or is not lowercase ASCII with only interior `-` and `_` separators.
            pub fn parse(value: impl Into<String>) -> Result<Self, IdentifierError> {
                let value = value.into();
                validate_identifier($kind, &value)?;
                Ok(Self(value))
            }

            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl AsRef<str> for $type {
            fn as_ref(&self) -> &str {
                self.as_str()
            }
        }

        impl fmt::Display for $type {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(self.as_str())
            }
        }

        impl FromStr for $type {
            type Err = IdentifierError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::parse(value)
            }
        }

        impl TryFrom<String> for $type {
            type Error = IdentifierError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::parse(value)
            }
        }

        impl TryFrom<&str> for $type {
            type Error = IdentifierError;

            fn try_from(value: &str) -> Result<Self, Self::Error> {
                Self::parse(value)
            }
        }

        impl<'de> Deserialize<'de> for $type {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::parse(value).map_err(de::Error::custom)
            }
        }
    };
}

identifier!(ComponentId, "component");
identifier!(ActionId, "action");
identifier!(ProfileId, "profile");

/// A validation failure for a component, action, or profile identifier.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IdentifierError {
    kind: &'static str,
    value: String,
    reason: &'static str,
}

impl IdentifierError {
    #[must_use]
    pub fn kind(&self) -> &'static str {
        self.kind
    }

    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }
}

impl fmt::Display for IdentifierError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "invalid {} id {:?}: {}",
            self.kind, self.value, self.reason
        )
    }
}

impl Error for IdentifierError {}

fn validate_identifier(kind: &'static str, value: &str) -> Result<(), IdentifierError> {
    let error = |reason| IdentifierError {
        kind,
        value: value.to_owned(),
        reason,
    };
    if value.is_empty() {
        return Err(error("the value must not be empty"));
    }
    if value.len() > MAX_IDENTIFIER_BYTES {
        return Err(error("the value must be at most 64 bytes"));
    }
    let bytes = value.as_bytes();
    if !bytes[0].is_ascii_lowercase() && !bytes[0].is_ascii_digit() {
        return Err(error(
            "the value must start with a lowercase ASCII letter or digit",
        ));
    }
    if !bytes[bytes.len() - 1].is_ascii_lowercase() && !bytes[bytes.len() - 1].is_ascii_digit() {
        return Err(error(
            "the value must end with a lowercase ASCII letter or digit",
        ));
    }
    if !bytes.iter().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
    }) {
        return Err(error(
            "the value may contain only lowercase ASCII letters, digits, '-' and '_'",
        ));
    }
    Ok(())
}

/// The structured identity of one executable component/action pair.
#[derive(
    Clone, Debug, Deserialize, Eq, Hash, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(deny_unknown_fields)]
pub struct TargetId {
    pub component: ComponentId,
    pub action: ActionId,
}

impl TargetId {
    #[must_use]
    pub const fn new(component: ComponentId, action: ActionId) -> Self {
        Self { component, action }
    }
}

impl fmt::Display for TargetId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}:{}", self.component, self.action)
    }
}

impl FromStr for TargetId {
    type Err = TargetIdError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let Some((component, action)) = value.split_once(':') else {
            return Err(TargetIdError::MissingSeparator(value.to_owned()));
        };
        if action.contains(':') {
            return Err(TargetIdError::TooManySeparators(value.to_owned()));
        }
        Ok(Self {
            component: ComponentId::parse(component).map_err(TargetIdError::Component)?,
            action: ActionId::parse(action).map_err(TargetIdError::Action)?,
        })
    }
}

/// A parse failure for the canonical `component:action` target syntax.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TargetIdError {
    MissingSeparator(String),
    TooManySeparators(String),
    Component(IdentifierError),
    Action(IdentifierError),
}

impl fmt::Display for TargetIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingSeparator(value) => {
                write!(
                    formatter,
                    "invalid target {value:?}: expected component:action"
                )
            }
            Self::TooManySeparators(value) => {
                write!(
                    formatter,
                    "invalid target {value:?}: expected exactly one ':'"
                )
            }
            Self::Component(error) | Self::Action(error) => error.fmt(formatter),
        }
    }
}

impl Error for TargetIdError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Component(error) | Self::Action(error) => Some(error),
            Self::MissingSeparator(_) | Self::TooManySeparators(_) => None,
        }
    }
}

/// An addressable software unit in the workspace.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ComponentSpec {
    pub id: ComponentId,
    pub root: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depends_on: Vec<ComponentId>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub propagate_affected_to_dependents: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub adapters: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guidance: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub provenance: BTreeMap<String, FieldProvenance>,
}

impl ComponentSpec {
    #[must_use]
    pub fn new(id: ComponentId, root: impl Into<String>) -> Self {
        Self {
            id,
            root: root.into(),
            description: None,
            tags: Vec::new(),
            depends_on: Vec::new(),
            propagate_affected_to_dependents: false,
            adapters: Vec::new(),
            guidance: None,
            provenance: BTreeMap::new(),
        }
    }
}

/// The semantic intent of an action.
#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ActionIntent {
    Check,
    Generate,
    Serve,
    Operate,
}

/// A class of effect an action may produce.
#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ActionEffect {
    ReadOnly,
    Worktree,
    Process,
    External,
}

/// The checked-in implementation selected for an action.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ActionRunner {
    Command {
        command: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        working_directory: Option<String>,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        environment: BTreeMap<String, String>,
    },
    Native {
        operation: String,
    },
}

impl ActionRunner {
    #[must_use]
    pub fn command(command: impl Into<String>) -> Self {
        Self::Command {
            command: command.into(),
            working_directory: None,
            environment: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn native(operation: impl Into<String>) -> Self {
        Self::Native {
            operation: operation.into(),
        }
    }
}

/// How an action's output is normalized into findings.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResultParser {
    #[default]
    ExitCode,
    JsonLines,
}

/// One typed capability offered by a component.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ActionSpec {
    pub target: TargetId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub intent: ActionIntent,
    pub effects: Vec<ActionEffect>,
    pub runner: ActionRunner,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depends_on: Vec<TargetId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<u64>,
    #[serde(default, skip_serializing_if = "is_default_result_parser")]
    pub result_parser: ResultParser,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub legacy_aliases: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub provenance: BTreeMap<String, FieldProvenance>,
}

impl ActionSpec {
    #[must_use]
    pub fn new(target: TargetId, intent: ActionIntent, runner: ActionRunner) -> Self {
        Self {
            target,
            description: None,
            intent,
            effects: Vec::new(),
            runner,
            inputs: Vec::new(),
            depends_on: Vec::new(),
            timeout_seconds: None,
            result_parser: ResultParser::default(),
            legacy_aliases: Vec::new(),
            provenance: BTreeMap::new(),
        }
    }
}

/// A checked-in named selection of targets.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileSpec {
    pub id: ProfileId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub targets: Vec<TargetId>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub provenance: BTreeMap<String, FieldProvenance>,
}

impl ProfileSpec {
    #[must_use]
    pub const fn new(id: ProfileId, targets: Vec<TargetId>) -> Self {
        Self {
            id,
            description: None,
            targets,
            provenance: BTreeMap::new(),
        }
    }
}

const fn is_false(value: &bool) -> bool {
    !*value
}

fn is_default_result_parser(value: &ResultParser) -> bool {
    *value == ResultParser::default()
}

#[cfg(test)]
mod tests {
    use super::{
        ActionEffect, ActionId, ActionIntent, ActionRunner, ActionSpec, ComponentId, ProfileId,
        ResultParser, TargetId,
    };

    #[test]
    fn identifiers_accept_stable_lowercase_addresses() {
        assert_eq!(ComponentId::parse("api-v2").unwrap().as_str(), "api-v2");
        assert_eq!(
            ActionId::parse("test_locked").unwrap().as_str(),
            "test_locked"
        );
        assert_eq!(ProfileId::parse("ci").unwrap().as_str(), "ci");
    }

    #[test]
    fn identifiers_reject_values_that_conflict_with_selector_syntax() {
        for value in ["", "Web", "web:*", "-web", "web-", "web app"] {
            assert!(ComponentId::parse(value).is_err(), "{value:?} must fail");
        }
    }

    #[test]
    fn target_text_round_trips_without_changing_structured_json() {
        let target: TargetId = "api:test".parse().unwrap();
        assert_eq!(target.to_string(), "api:test");
        assert_eq!(
            serde_json::to_value(&target).unwrap(),
            serde_json::json!({"component": "api", "action": "test"})
        );
        assert_eq!(
            serde_json::from_value::<TargetId>(serde_json::json!({
                "component": "api",
                "action": "test"
            }))
            .unwrap(),
            target
        );
    }

    #[test]
    fn action_json_keeps_intent_effects_and_runner_independent() {
        let mut action = ActionSpec::new(
            "api:test".parse().unwrap(),
            ActionIntent::Check,
            ActionRunner::command("go_test_command"),
        );
        action.effects = vec![ActionEffect::ReadOnly, ActionEffect::Process];

        let value = serde_json::to_value(action).unwrap();
        assert_eq!(value["intent"], "check");
        assert_eq!(
            value["effects"],
            serde_json::json!(["read_only", "process"])
        );
        assert_eq!(
            value["runner"],
            serde_json::json!({"kind": "command", "command": "go_test_command"})
        );
        assert_eq!(value.get("result_parser"), None);
        assert_eq!(ResultParser::default(), ResultParser::ExitCode);
    }

    #[test]
    fn identifier_deserialization_enforces_validation() {
        let error = serde_json::from_str::<ActionId>(r#""Test""#)
            .unwrap_err()
            .to_string();
        assert!(error.contains("invalid action id"));
    }
}
