use super::*;

pub(super) fn work_check_input_schema() -> Value {
    let mut focus =
        repository::schema_value::<std::collections::BTreeMap<String, jig_contract::RustFocusV1>>();
    let definitions = focus
        .as_object_mut()
        .and_then(|value| value.remove("$defs"));
    focus
        .as_object_mut()
        .expect("map schema is an object")
        .remove("$schema");
    focus["maxProperties"] = json!(32);
    focus["propertyNames"] = json!({"pattern":"^[a-z0-9](?:[a-z0-9_-]{0,62}[a-z0-9])?:[a-z0-9](?:[a-z0-9_-]{0,62}[a-z0-9])?$"});
    let mut schema = object_schema(
        &[
            (args::PLAN_ID, string_schema()),
            (
                args::GATES,
                json!({
                    "type": "array",
                    "items": { "type": "string" }
                }),
            ),
            (
                args::TOOLS,
                json!({
                    "type": "array",
                    "items": { "type": "string" }
                }),
            ),
            (
                args::PHASE,
                json!({
                    "type": ["string", "null"],
                    "enum": ["iteration", "final", null]
                }),
            ),
            (args::EXPLAIN, json!({ "type": ["boolean", "null"] })),
            ("rust_focus", json!({"anyOf": [focus, {"type": "null"}]})),
        ],
        &[args::PLAN_ID],
    );
    if let Some(definitions) = definitions {
        schema["$defs"] = definitions;
    }
    schema["allOf"] = json!([{
        "if": {"required": ["rust_focus"], "properties": {"rust_focus": {"type": "object", "minProperties": 1}}},
        "then": {"required": ["phase"], "properties": {"phase": {"const": "iteration"}}}
    }]);
    schema["not"] = json!({
        "anyOf": [
            {
                "allOf": [
                    {
                        "required": [args::GATES],
                        "properties": { "gates": { "minItems": 1 } }
                    },
                    {
                        "required": [args::TOOLS],
                        "properties": { "tools": { "minItems": 1 } }
                    }
                ]
            },
            {
                "allOf": [
                    { "required": [args::PHASE] },
                    { "properties": { "phase": { "type": "string" } } },
                    {
                        "anyOf": [
                            {
                                "required": [args::GATES],
                                "properties": { "gates": { "minItems": 1 } }
                            },
                            {
                                "required": [args::TOOLS],
                                "properties": { "tools": { "minItems": 1 } }
                            }
                        ]
                    }
                ]
            }
        ]
    });
    schema
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_accepts_typed_iteration_focus_and_rejects_invalid_scope() {
        let schema = work_check_input_schema();
        let validator = jsonschema::validator_for(&schema).unwrap();
        for value in [
            json!({"plan_id":"plan_1", "rust_focus":null}),
            json!({"plan_id":"plan_1", "phase":"iteration", "rust_focus":{"api:test":{"kind":"automatic"}}}),
            json!({"plan_id":"plan_1", "phase":"iteration", "rust_focus":{"api:test":{"kind":"explicit","packages":["example-api@0.1.0"],"targets":[{"kind":"lib"}]}}}),
        ] {
            assert!(validator.is_valid(&value), "{value}");
        }
        for value in [
            json!({"plan_id":"plan_1", "phase":"final", "rust_focus":{"api:test":{"kind":"automatic"}}}),
            json!({"plan_id":"plan_1", "phase":"iteration", "rust_focus":{"invalid":{"kind":"automatic"}}}),
            json!({"plan_id":"plan_1", "rust_focus":{"api:test":{"kind":"automatic"}}}),
            json!({"plan_id":"plan_1", "phase":"iteration", "rust_focus":{"api:test":{"kind":"automatic","unknown":true}}}),
            json!({"plan_id":"plan_1", "phase":"iteration", "rust_focus":{"api:test":{"kind":"unknown"}}}),
        ] {
            assert!(!validator.is_valid(&value), "{value}");
        }
    }
}
