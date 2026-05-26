use crate::app_definitions::TargetDefinition;
use crate::paths::{ensure_parent, write_private_following_symlink};
use crate::profiles::Profile;
use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use serde_json::ser::{PrettyFormatter, Serializer};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use toml_edit::{value, DocumentMut, Item, Table};

pub fn render_template(input: &str, profile: &Profile) -> Option<String> {
    let trimmed = input.trim();
    if !trimmed.starts_with("{{") || !trimmed.ends_with("}}") {
        return Some(input.to_string());
    }
    let expr = trimmed
        .trim_start_matches("{{")
        .trim_end_matches("}}")
        .trim();
    let path = expr.strip_prefix("fields.")?;
    lookup_field(profile, path).and_then(value_to_string)
}

pub fn render_text_template(input: &str, profile: &Profile) -> Result<String> {
    let mut output = String::new();
    let mut rest = input;
    while let Some(start) = rest.find("{{") {
        let (before, after_start) = rest.split_at(start);
        output.push_str(before);
        let Some(end) = after_start.find("}}") else {
            return Err(anyhow!("TemplateInvalid: missing closing braces"));
        };
        let expr = after_start[2..end].trim();
        let Some(path) = expr.strip_prefix("fields.") else {
            return Err(anyhow!("TemplateInvalid: unsupported expression {expr}"));
        };
        let value = lookup_field(profile, path)
            .and_then(value_to_string)
            .ok_or_else(|| anyhow!("FieldMissing: {path}"))?;
        output.push_str(&value);
        rest = &after_start[end + 2..];
    }
    output.push_str(rest);
    Ok(output)
}

fn lookup_field<'a>(profile: &'a Profile, path: &str) -> Option<&'a Value> {
    let mut parts = path.split('.');
    let first = parts.next()?;
    let mut current = profile.fields.get(first)?;
    for part in parts {
        current = current.as_object()?.get(part)?;
    }
    Some(current)
}

fn value_to_string(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(value) if value.is_empty() => None,
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => Some(value.to_string()),
    }
}

pub fn apply_json_env_merge(
    path: &Path,
    target: &TargetDefinition,
    profile: &Profile,
) -> Result<()> {
    let mut root = read_json_object(path)?;
    let env = ensure_object_path(&mut root, target.json_path.as_deref().unwrap_or("$.env"))?;
    for key in &target.managed_keys {
        env.remove(key);
    }
    for (key, template) in &target.mapping {
        if let Some(value) = render_template(template, profile) {
            env.insert(key.clone(), Value::String(value));
        }
    }
    write_json(path, &root)
}

pub fn render_json_env(path: &Path, target: &TargetDefinition, profile: &Profile) -> Result<Value> {
    let mut root = read_json_object(path)?;
    let env = ensure_object_path(&mut root, target.json_path.as_deref().unwrap_or("$.env"))?;
    for key in &target.managed_keys {
        env.remove(key);
    }
    for (key, template) in &target.mapping {
        if let Some(value) = render_template(template, profile) {
            env.insert(key.clone(), Value::String(value));
        }
    }
    Ok(root)
}

pub fn render_file_capture(target: &TargetDefinition, profile: &Profile) -> Result<Vec<u8>> {
    if let Some(template) = &target.template {
        return Ok(render_text_template(template, profile)?.into_bytes());
    }
    Err(anyhow!(
        "DefinitionLoadFailed: file_capture target requires template for static rendering"
    ))
}

pub fn apply_file_capture(path: &Path, target: &TargetDefinition, profile: &Profile) -> Result<()> {
    write_private_following_symlink(path, &render_file_capture(target, profile)?)
}

pub fn apply_toml_managed_paths(
    path: &Path,
    target: &TargetDefinition,
    profile: &Profile,
) -> Result<()> {
    let mut doc = read_toml(path)?;
    merge_toml_managed_paths(&mut doc, target, profile)?;
    write_private_following_symlink(path, doc.to_string().as_bytes())
}

fn merge_toml_managed_paths(
    doc: &mut DocumentMut,
    target: &TargetDefinition,
    profile: &Profile,
) -> Result<()> {
    for toml_path in &target.toml_paths {
        remove_toml_path(doc, toml_path);
        if let Some(value) = lookup_field(profile, toml_path) {
            set_toml_item(doc, toml_path, value_to_toml_item(value));
        }
    }

    if target
        .toml_paths
        .iter()
        .any(|path| path == "model_providers")
    {
        let provider = lookup_field(profile, "model_provider")
            .and_then(Value::as_str)
            .unwrap_or("openai");
        let provider_id = lookup_field(profile, "provider_id")
            .and_then(Value::as_str)
            .unwrap_or(provider);
        let providers = doc
            .entry("model_providers")
            .or_insert(Item::Table(Table::new()))
            .as_table_mut()
            .ok_or_else(|| anyhow!("config.toml model_providers is not a table"))?;
        let table = providers
            .entry(provider_id)
            .or_insert(Item::Table(Table::new()))
            .as_table_mut()
            .ok_or_else(|| anyhow!("provider entry is not a table"))?;
        table["name"] = value(provider);
        if let Some(base_url) = lookup_field(profile, "base_url").and_then(Value::as_str) {
            table["base_url"] = value(base_url);
        }
    }
    Ok(())
}

pub fn read_json_path(path: &Path, json_path: &str) -> Result<Option<Value>> {
    let root = read_json_object(path)?;
    if json_path == "$" {
        return Ok(Some(root));
    }
    let mut current = &root;
    for segment in json_path_segments(json_path)? {
        let Some(next) = current.as_object().and_then(|object| object.get(&segment)) else {
            return Ok(None);
        };
        current = next;
    }
    Ok(Some(current.clone()))
}

pub fn write_json_path(path: &Path, json_path: &str, value: Value) -> Result<()> {
    let mut root = read_json_object(path)?;
    if json_path == "$" {
        write_json(path, &value)?;
        return Ok(());
    }
    let segments = json_path_segments(json_path)?;
    let (last, parents) = segments
        .split_last()
        .ok_or_else(|| anyhow!("unsupported json path: {json_path}"))?;
    let mut current = &mut root;
    for segment in parents {
        if !current.is_object() {
            *current = Value::Object(Default::default());
        }
        current = current
            .as_object_mut()
            .unwrap()
            .entry(segment.clone())
            .or_insert_with(|| Value::Object(Default::default()));
    }
    if !current.is_object() {
        *current = Value::Object(Default::default());
    }
    current.as_object_mut().unwrap().insert(last.clone(), value);
    write_json(path, &root)
}

pub fn remove_json_object_keys(path: &Path, json_path: &str, keys: &[&str]) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let mut root = read_json_object(path)?;
    let Some(env) = get_object_path_mut(&mut root, json_path)? else {
        return Ok(());
    };
    let mut changed = false;
    for key in keys {
        changed |= env.remove(*key).is_some();
    }
    if changed {
        write_json(path, &root)?;
    }
    Ok(())
}

pub fn json_object_has_any_key(path: &Path, json_path: &str, keys: &[&str]) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    let root = read_json_object(path)?;
    let Some(env) = get_object_path(&root, json_path) else {
        return Ok(false);
    };
    Ok(keys.iter().any(|key| env.contains_key(*key)))
}

pub fn capture_toml_fragment(path: &Path, toml_paths: &[String]) -> Result<String> {
    let doc = read_toml(path)?;
    let mut fragment = DocumentMut::new();
    for toml_path in toml_paths {
        if let Some(item) = get_toml_item(&doc, toml_path) {
            set_toml_item(&mut fragment, toml_path, item.clone());
        }
    }
    Ok(fragment.to_string())
}

pub fn merge_toml_fragment(path: &Path, fragment: &str, toml_paths: &[String]) -> Result<()> {
    let mut doc = read_toml(path)?;
    let fragment = fragment.parse::<DocumentMut>()?;
    for toml_path in toml_paths {
        remove_toml_path(&mut doc, toml_path);
        if let Some(item) = get_toml_item(&fragment, toml_path) {
            set_toml_item(&mut doc, toml_path, item.clone());
        }
    }
    write_private_following_symlink(path, doc.to_string().as_bytes())
}

pub fn rendered_targets(
    target_paths: &[(TargetDefinition, PathBuf)],
    profile: &Profile,
) -> Result<Vec<(PathBuf, Vec<u8>)>> {
    let mut rendered = Vec::new();
    for (target, path) in target_paths {
        match target.handler.as_str() {
            "json_env_merge" => rendered.push((
                path.clone(),
                serde_json::to_vec_pretty(&render_json_env(path, target, profile)?)?,
            )),
            "file_capture" => rendered.push((path.clone(), render_file_capture(target, profile)?)),
            "toml_managed_paths" => {
                let mut doc = read_toml(path)?;
                merge_toml_managed_paths(&mut doc, target, profile)?;
                rendered.push((path.clone(), doc.to_string().into_bytes()));
            }
            other => return Err(anyhow!("handler {other} is not implemented for rendering")),
        }
    }
    Ok(rendered)
}

pub fn apply_target(target: &TargetDefinition, path: &Path, profile: &Profile) -> Result<()> {
    match target.handler.as_str() {
        "json_env_merge" => apply_json_env_merge(path, target, profile),
        "file_capture" => apply_file_capture(path, target, profile),
        "toml_managed_paths" => apply_toml_managed_paths(path, target, profile),
        other => Err(anyhow!("handler {other} is not implemented for use")),
    }
}

pub fn static_status(target: &TargetDefinition, path: &Path, profile: &Profile) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    match target.handler.as_str() {
        "json_env_merge" => {
            let root = read_json_object(path)?;
            let actual = get_object_path(&root, target.json_path.as_deref().unwrap_or("$.env"));
            let Some(actual) = actual else {
                return Ok(false);
            };
            for key in &target.managed_keys {
                let expected = target
                    .mapping
                    .get(key)
                    .and_then(|v| render_template(v, profile));
                let actual_value = actual.get(key).and_then(Value::as_str).map(str::to_string);
                if expected != actual_value {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        "file_capture" => {
            if target.template.is_some() {
                return Ok(fs::read(path)? == render_file_capture(target, profile)?);
            }
            Ok(false)
        }
        "toml_managed_paths" => {
            let doc = read_toml(path)?;
            let mut expected_doc = doc.clone();
            merge_toml_managed_paths(&mut expected_doc, target, profile)?;
            for toml_path in &target.toml_paths {
                let actual = meaningful_toml_item(get_toml_item(&doc, toml_path));
                let expected = meaningful_toml_item(get_toml_item(&expected_doc, toml_path));
                match (expected, actual) {
                    (None, None) => {}
                    (Some(_), None)
                        if default_model_providers_absence_is_equivalent(
                            target, profile, toml_path,
                        ) => {}
                    (Some(expected), Some(actual))
                        if toml_items_semantically_equal(expected, actual) => {}
                    _ => return Ok(false),
                }
            }
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn read_json_object(path: &Path) -> Result<Value> {
    if !path.exists() {
        return Ok(Value::Object(Default::default()));
    }
    let bytes = fs::read(path).with_context(|| path.display().to_string())?;
    if bytes.is_empty() {
        return Ok(Value::Object(Default::default()));
    }
    let value: Value = serde_json::from_slice(&bytes)?;
    if value.is_object() {
        Ok(value)
    } else {
        Err(anyhow!("{} must contain a JSON object", path.display()))
    }
}

fn write_json(path: &Path, value: &Value) -> Result<()> {
    ensure_parent(path)?;
    let style = JsonStyle::sample(path)?;
    let bytes = style.serialize(value)?;
    write_private_following_symlink(path, &bytes)
}

struct JsonStyle {
    compact: bool,
    indent: Vec<u8>,
    trailing_newline: bool,
}

impl JsonStyle {
    fn sample(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let bytes = fs::read(path).with_context(|| path.display().to_string())?;
        if bytes.is_empty() {
            return Ok(Self::default());
        }
        let trailing_newline = bytes.ends_with(b"\n");
        let compact = !bytes.contains(&b'\n');
        let indent = sample_indent(&bytes).unwrap_or_else(|| b"  ".to_vec());
        Ok(Self {
            compact,
            indent,
            trailing_newline,
        })
    }

    fn serialize(&self, value: &Value) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        if self.compact {
            serde_json::to_writer(&mut bytes, value)?;
        } else {
            let formatter = PrettyFormatter::with_indent(&self.indent);
            let mut serializer = Serializer::with_formatter(&mut bytes, formatter);
            value.serialize(&mut serializer)?;
        }
        if self.trailing_newline && !bytes.ends_with(b"\n") {
            bytes.push(b'\n');
        }
        Ok(bytes)
    }
}

impl Default for JsonStyle {
    fn default() -> Self {
        Self {
            compact: false,
            indent: b"  ".to_vec(),
            trailing_newline: true,
        }
    }
}

fn sample_indent(bytes: &[u8]) -> Option<Vec<u8>> {
    for line in bytes.split(|byte| *byte == b'\n').skip(1) {
        let indent = line
            .iter()
            .take_while(|byte| **byte == b' ' || **byte == b'\t')
            .copied()
            .collect::<Vec<_>>();
        if !indent.is_empty() {
            return Some(indent);
        }
    }
    None
}

fn ensure_object_path<'a>(
    root: &'a mut Value,
    path: &str,
) -> Result<&'a mut serde_json::Map<String, Value>> {
    let mut current = root;
    let segments = json_path_segments(path)?;
    for segment in segments {
        if !current.is_object() {
            *current = Value::Object(Default::default());
        }
        current = current
            .as_object_mut()
            .unwrap()
            .entry(segment)
            .or_insert_with(|| Value::Object(Default::default()));
    }
    current
        .as_object_mut()
        .ok_or_else(|| anyhow!("{path} is not a JSON object"))
}

fn get_object_path<'a>(root: &'a Value, path: &str) -> Option<&'a serde_json::Map<String, Value>> {
    let mut current = root;
    for segment in json_path_segments(path).ok()? {
        current = current.as_object()?.get(&segment)?;
    }
    current.as_object()
}

fn get_object_path_mut<'a>(
    root: &'a mut Value,
    path: &str,
) -> Result<Option<&'a mut serde_json::Map<String, Value>>> {
    let mut current = root;
    for segment in json_path_segments(path)? {
        let Some(next) = current
            .as_object_mut()
            .and_then(|object| object.get_mut(&segment))
        else {
            return Ok(None);
        };
        current = next;
    }
    Ok(current.as_object_mut())
}

fn json_path_segments(path: &str) -> Result<Vec<String>> {
    if path == "$" {
        return Ok(Vec::new());
    }
    let rest = path
        .strip_prefix("$.")
        .ok_or_else(|| anyhow!("unsupported json path: {path}"))?;
    Ok(rest.split('.').map(ToOwned::to_owned).collect())
}

fn read_toml(path: &Path) -> Result<DocumentMut> {
    if !path.exists() {
        return Ok(DocumentMut::new());
    }
    fs::read_to_string(path)
        .with_context(|| path.display().to_string())?
        .parse::<DocumentMut>()
        .with_context(|| format!("parse TOML {}", path.display()))
}

fn toml_path_segments(path: &str) -> Vec<&str> {
    path.split('.')
        .filter(|segment| !segment.is_empty())
        .collect()
}

fn get_toml_item<'a>(doc: &'a DocumentMut, path: &str) -> Option<&'a Item> {
    let mut item = doc.as_item();
    for segment in toml_path_segments(path) {
        item = item.get(segment)?;
    }
    Some(item)
}

fn meaningful_toml_item(item: Option<&Item>) -> Option<&Item> {
    item.filter(|item| !item.is_none())
}

fn toml_items_semantically_equal(left: &Item, right: &Item) -> bool {
    if let (Some(left), Some(right)) = (left.as_str(), right.as_str()) {
        return left == right;
    }
    left.to_string().trim() == right.to_string().trim()
}

fn default_model_providers_absence_is_equivalent(
    target: &TargetDefinition,
    profile: &Profile,
    toml_path: &str,
) -> bool {
    toml_path == "model_providers"
        && target
            .toml_paths
            .iter()
            .any(|path| path == "model_providers")
        && lookup_field(profile, "model_provider")
            .and_then(Value::as_str)
            .is_none_or(|provider| provider == "openai")
        && lookup_field(profile, "provider_id").is_none()
        && lookup_field(profile, "base_url").is_none()
}

fn set_toml_item(doc: &mut DocumentMut, path: &str, item: Item) {
    let segments = toml_path_segments(path);
    let Some((last, parents)) = segments.split_last() else {
        return;
    };
    let mut table = doc.as_table_mut();
    for segment in parents {
        let entry = table
            .entry(segment)
            .or_insert_with(|| Item::Table(Table::new()));
        if !entry.is_table() {
            *entry = Item::Table(Table::new());
        }
        table = entry.as_table_mut().expect("entry was just made a table");
    }
    table[*last] = item;
}

fn remove_toml_path(doc: &mut DocumentMut, path: &str) {
    let segments = toml_path_segments(path);
    let Some((last, parents)) = segments.split_last() else {
        return;
    };
    let mut table = doc.as_table_mut();
    for segment in parents {
        let Some(item) = table.get_mut(segment) else {
            return;
        };
        let Some(next) = item.as_table_mut() else {
            return;
        };
        table = next;
    }
    table.remove(last);
}

fn value_to_toml_item(value: &Value) -> Item {
    match value {
        Value::Bool(value) => toml_edit::value(*value),
        Value::Number(value) => {
            if let Some(value) = value.as_i64() {
                toml_edit::value(value)
            } else if let Some(value) = value.as_f64() {
                toml_edit::value(value)
            } else {
                toml_edit::value(value.to_string())
            }
        }
        Value::String(value) => toml_edit::value(value.clone()),
        _ => toml_edit::value(value.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_definitions::system_definition;
    use indexmap::IndexMap;
    use tempfile::tempdir;

    #[test]
    fn json_env_merge_keeps_unmanaged_keys() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(
            &path,
            r#"{"env":{"KEEP":"1","ANTHROPIC_AUTH_TOKEN":"old"}}"#,
        )
        .unwrap();
        let definition = system_definition("claude").unwrap().unwrap();
        let target = &definition.kinds["env_injection"].targets[0];
        let mut fields = IndexMap::new();
        fields.insert(
            "base_url".to_string(),
            Value::String("https://example.test".to_string()),
        );
        fields.insert(
            "auth_token".to_string(),
            Value::String("secret".to_string()),
        );
        let profile = Profile {
            id: "claude-test".into(),
            app: "claude".into(),
            kind: "env_injection".into(),
            schema_version: 1,
            name: "test".into(),
            notes: String::new(),
            created_at: "now".into(),
            fields,
            identity: IndexMap::new(),
            capture: None,
            extensions: Value::Null,
        };
        apply_json_env_merge(&path, target, &profile).unwrap();
        let bytes = fs::read(path).unwrap();
        assert!(!bytes.contains(&b'\n'));
        let json: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["env"]["KEEP"], "1");
        assert_eq!(json["env"]["ANTHROPIC_AUTH_TOKEN"], "secret");
    }

    #[test]
    fn json_env_merge_preserves_pretty_indent_and_trailing_newline() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(
            &path,
            "{\n    \"env\": {\n        \"KEEP\": \"1\"\n    }\n}\n",
        )
        .unwrap();
        let definition = system_definition("claude").unwrap().unwrap();
        let target = &definition.kinds["env_injection"].targets[0];
        let mut fields = IndexMap::new();
        fields.insert(
            "base_url".to_string(),
            Value::String("https://example.test".to_string()),
        );
        fields.insert(
            "auth_token".to_string(),
            Value::String("secret".to_string()),
        );
        let profile = Profile {
            id: "claude-test".into(),
            app: "claude".into(),
            kind: "env_injection".into(),
            schema_version: 1,
            name: "test".into(),
            notes: String::new(),
            created_at: "now".into(),
            fields,
            identity: IndexMap::new(),
            capture: None,
            extensions: Value::Null,
        };
        apply_json_env_merge(&path, target, &profile).unwrap();
        let text = fs::read_to_string(path).unwrap();
        assert!(text.ends_with('\n'));
        assert!(text.contains("\n    \"env\""));
        assert!(text.contains("\n        \"KEEP\""));
    }

    #[test]
    fn toml_managed_paths_keep_unknown_table() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "[mcp_servers.foo]\ncommand = \"bar\"\n").unwrap();
        let mut fields = IndexMap::new();
        fields.insert(
            "model".to_string(),
            Value::String("gpt-5-codex".to_string()),
        );
        fields.insert(
            "model_provider".to_string(),
            Value::String("openai".to_string()),
        );
        let profile = Profile {
            id: "codex-test".into(),
            app: "codex".into(),
            kind: "file_template".into(),
            schema_version: 1,
            name: "test".into(),
            notes: String::new(),
            created_at: "now".into(),
            fields,
            identity: IndexMap::new(),
            capture: None,
            extensions: Value::Null,
        };
        let target = TargetDefinition {
            handler: "toml_managed_paths".to_string(),
            path: path.display().to_string(),
            json_path: None,
            managed_keys: Vec::new(),
            mapping: Default::default(),
            template: None,
            toml_paths: vec!["model".to_string(), "model_provider".to_string()],
            import_json_matches: Default::default(),
            import_json_required_strings: Vec::new(),
            import_json_forbidden_strings: Vec::new(),
            import_json_fields: Default::default(),
            import_unmatched_error: None,
            requires_app_stopped: false,
        };
        apply_toml_managed_paths(&path, &target, &profile).unwrap();
        let doc = fs::read_to_string(path)
            .unwrap()
            .parse::<DocumentMut>()
            .unwrap();
        assert_eq!(doc["mcp_servers"]["foo"]["command"].as_str(), Some("bar"));
        assert_eq!(doc["model"].as_str(), Some("gpt-5-codex"));
    }
}
