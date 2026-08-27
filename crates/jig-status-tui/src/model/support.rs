use serde_json::Value;

use jig_tui::sanitize_text;

pub(super) fn moved_index(current: usize, len: usize, delta: isize) -> usize {
    if len == 0 {
        return 0;
    }
    current
        .saturating_add_signed(delta)
        .min(len.saturating_sub(1))
}

pub(super) fn fallback(value: String, fallback: &str) -> String {
    nonempty(value).unwrap_or_else(|| fallback.to_owned())
}

pub(super) fn nonempty(value: String) -> Option<String> {
    (!value.trim().is_empty()).then_some(value)
}

pub(super) fn array_len(value: &Value, key: &str) -> usize {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0)
}

pub(super) fn sanitize_value(value: &mut Value) {
    match value {
        Value::String(text) => *text = sanitize_text(text),
        Value::Array(values) => values.iter_mut().for_each(sanitize_value),
        Value::Object(values) => values.values_mut().for_each(sanitize_value),
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}
