//! Go map key order for JSON objects.
//!
//! `serde_json` is built with `preserve_order` so a payload ported from a Go
//! *struct* keeps that struct's declaration order (Go marshals struct fields in
//! declaration order). Payloads Go builds from a `map[string]any` are emitted
//! sorted by key instead, so the ported code runs them through
//! [`go_map_order`].

use serde_json::{Map, Value};

/// Sorts every object key in `value`, recursively — Go's `map[string]any`
/// marshalling order.
pub fn go_map_order(value: Value) -> Value {
    match value {
        Value::Object(fields) => {
            let mut entries: Vec<(String, Value)> = fields.into_iter().collect();
            entries.sort_by(|left, right| left.0.cmp(&right.0));
            Value::Object(
                entries
                    .into_iter()
                    .map(|(key, nested)| (key, go_map_order(nested)))
                    .collect::<Map<String, Value>>(),
            )
        }
        Value::Array(items) => Value::Array(items.into_iter().map(go_map_order).collect()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::go_map_order;
    use serde_json::json;

    #[test]
    fn nested_objects_and_array_elements_are_sorted() {
        let sorted = go_map_order(json!({"b": [{"d": 1, "c": 2}], "a": {"z": 0, "y": 0}}));
        assert_eq!(
            serde_json::to_string(&sorted).expect("serialize"),
            r#"{"a":{"y":0,"z":0},"b":[{"c":2,"d":1}]}"#
        );
    }
}
