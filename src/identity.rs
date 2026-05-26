use crate::app_definitions::IdentityDefinition;
use anyhow::{anyhow, Result};
use indexmap::IndexMap;
use serde_json::Value;

pub fn extract_identity_from_definition(
    root: &Value,
    definition: &IdentityDefinition,
) -> Result<IndexMap<String, Value>> {
    match definition.handler.as_deref() {
        Some("json_paths" | "jwt_payload_json_paths") => {
            let mut identity = IndexMap::new();
            for (name, field) in &definition.fields {
                if let Some(value) = value_at_simple_json_path(root, &field.path)? {
                    identity.insert(name.clone(), value);
                }
            }
            Ok(identity)
        }
        Some(handler) => Err(anyhow!("UnknownHandler: {handler}")),
        None => Err(anyhow!("DefinitionLoadFailed: identity handler missing")),
    }
}

fn value_at_simple_json_path(root: &Value, path: &str) -> Result<Option<Value>> {
    if path == "$" {
        return Ok(Some(root.clone()));
    }
    let Some(rest) = path.strip_prefix("$.") else {
        return Err(anyhow!("unsupported json_path {path}"));
    };
    value_at_segments(root, &rest.split('.').collect::<Vec<_>>())
}

fn value_at_segments(root: &Value, segments: &[&str]) -> Result<Option<Value>> {
    let mut current = root;
    for (index, segment) in segments.iter().enumerate() {
        if let Some(object) = current.as_object() {
            let Some(next) = object.get(*segment) else {
                return Ok(None);
            };
            current = next;
        } else if let Some(token) = current.as_str() {
            let payload = decode_jwt_payload(token)?;
            return value_at_segments(&payload, &segments[index..]);
        } else {
            return Ok(None);
        }
    }
    Ok(Some(current.clone()))
}

pub fn decode_jwt_payload(token: &str) -> Result<Value> {
    let payload = token
        .split('.')
        .nth(1)
        .ok_or_else(|| anyhow!("JWT payload missing"))?;
    let bytes = decode_base64url(payload)?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn decode_base64url(input: &str) -> Result<Vec<u8>> {
    let mut bits: u32 = 0;
    let mut bit_count = 0;
    let mut out = Vec::new();
    for byte in input.bytes() {
        if byte == b'=' {
            break;
        }
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            _ => return Err(anyhow!("invalid base64url byte")),
        } as u32;
        bits = (bits << 6) | value;
        bit_count += 6;
        while bit_count >= 8 {
            bit_count -= 8;
            out.push(((bits >> bit_count) & 0xff) as u8);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_jwt_payload_without_signature_validation() {
        let payload = decode_jwt_payload("x.eyJzdWIiOiIxMjMiLCJlbWFpbCI6ImFAYi5jIn0.y").unwrap();
        assert_eq!(payload["sub"], "123");
        assert_eq!(payload["email"], "a@b.c");
    }

    #[test]
    fn extracts_identity_from_definition_json_paths_with_jwt_payload() {
        let definition = IdentityDefinition {
            handler: Some("json_paths".to_string()),
            fields: [
                (
                    "account_id".to_string(),
                    crate::app_definitions::IdentityField {
                        path: "$.tokens.account_id".to_string(),
                        verify: "required".to_string(),
                    },
                ),
                (
                    "email".to_string(),
                    crate::app_definitions::IdentityField {
                        path: "$.tokens.id_token.email".to_string(),
                        verify: "optional".to_string(),
                    },
                ),
            ]
            .into_iter()
            .collect(),
        };
        let root = serde_json::json!({
            "tokens": {
                "account_id": "acct-a",
                "id_token": "x.eyJzdWIiOiIxMjMiLCJlbWFpbCI6ImFAYi5jIn0.y"
            }
        });

        let identity = extract_identity_from_definition(&root, &definition).unwrap();

        assert_eq!(identity["account_id"], "acct-a");
        assert_eq!(identity["email"], "a@b.c");
    }
}
