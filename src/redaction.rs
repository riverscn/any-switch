use crate::app_definitions::FieldSchema;
use indexmap::IndexMap;
use serde_json::Value;
use std::collections::BTreeMap;

pub fn is_sensitive_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    ["token", "key", "secret", "password"]
        .iter()
        .any(|needle| lower.contains(needle))
}

pub fn is_sensitive_field(name: &str, schema: Option<&FieldSchema>) -> bool {
    match schema.and_then(|schema| schema.sensitive) {
        Some(value) => value,
        None => is_sensitive_name(name),
    }
}

pub fn redact_fields(
    fields: &IndexMap<String, Value>,
    schema: &BTreeMap<String, FieldSchema>,
) -> Value {
    let mut out = serde_json::Map::new();
    for (key, value) in fields {
        if is_sensitive_field(key, schema.get(key)) {
            out.insert(key.clone(), Value::String("***".to_string()));
        } else if let Value::Object(map) = value {
            let nested_schema = schema.get(key).map(|s| &s.fields);
            let mut nested = serde_json::Map::new();
            for (nested_key, nested_value) in map {
                let sensitive = nested_schema
                    .and_then(|s| s.get(nested_key))
                    .map(|s| is_sensitive_field(nested_key, Some(s)))
                    .unwrap_or_else(|| is_sensitive_name(nested_key));
                nested.insert(
                    nested_key.clone(),
                    if sensitive {
                        Value::String("***".to_string())
                    } else {
                        nested_value.clone()
                    },
                );
            }
            out.insert(key.clone(), Value::Object(nested));
        } else {
            out.insert(key.clone(), value.clone());
        }
    }
    Value::Object(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_patterns_are_sensitive() {
        assert!(is_sensitive_name("auth_token"));
        assert!(is_sensitive_name("OPENAI_API_KEY"));
        assert!(!is_sensitive_name("model_provider"));
    }
}
