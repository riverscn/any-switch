use crate::app_definitions::{validate_id, AppDefinition, FieldSchema};
use crate::paths::{write_private, Paths};
use crate::redaction::is_sensitive_field;
use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileStore {
    pub schema_version: u32,
    #[serde(default)]
    pub preferences: Preferences,
    #[serde(default)]
    pub profiles: Vec<Profile>,
}

impl Default for ProfileStore {
    fn default() -> Self {
        Self {
            schema_version: 1,
            preferences: Preferences::default(),
            profiles: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preferences {
    #[serde(default = "default_keep_backups")]
    pub keep_backups: usize,
    #[serde(default = "default_confirm")]
    pub confirm_before_switch: bool,
    #[serde(default = "default_stale_days")]
    pub oauth_stale_warn_days: u64,
    #[serde(default)]
    pub default_app: Option<String>,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            keep_backups: default_keep_backups(),
            confirm_before_switch: default_confirm(),
            oauth_stale_warn_days: default_stale_days(),
            default_app: None,
        }
    }
}

fn default_keep_backups() -> usize {
    20
}
fn default_confirm() -> bool {
    true
}
fn default_stale_days() -> u64 {
    30
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub app: String,
    pub kind: String,
    pub schema_version: u32,
    pub name: String,
    #[serde(default)]
    pub notes: String,
    pub created_at: String,
    #[serde(default)]
    pub fields: IndexMap<String, Value>,
    #[serde(default)]
    pub identity: IndexMap<String, Value>,
    #[serde(default)]
    pub capture: Option<Value>,
    #[serde(default)]
    pub extensions: Value,
}

impl ProfileStore {
    pub fn load(paths: &Paths) -> Result<Self> {
        let path = paths.profiles_path();
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = fs::read_to_string(&path).with_context(|| path.display().to_string())?;
        let store: Self = serde_yaml::from_str(&text)?;
        store.validate_loaded()?;
        Ok(store)
    }

    pub fn save(&self, paths: &Paths) -> Result<()> {
        self.ensure_writable_schema()?;
        let text = serde_yaml::to_string(self)?;
        write_private(&paths.profiles_path(), text.as_bytes())
    }

    pub fn ensure_writable_schema(&self) -> Result<()> {
        if self.schema_version > 1 {
            return Err(anyhow!(
                "SchemaTooNew: profiles.yaml schema_version {} is newer than this CLI; read-only commands are allowed but writes require upgrading any-switch",
                self.schema_version
            ));
        }
        if let Some(profile) = self
            .profiles
            .iter()
            .find(|profile| profile.schema_version > 1)
        {
            return Err(anyhow!(
                "SchemaTooNew: profile {} schema_version {} is newer than this CLI; read-only commands are allowed but writes require upgrading any-switch",
                profile.id,
                profile.schema_version
            ));
        }
        Ok(())
    }

    fn validate_loaded(&self) -> Result<()> {
        let mut ids = std::collections::BTreeSet::new();
        for profile in &self.profiles {
            validate_id(&profile.id)
                .with_context(|| format!("ProfileInvalid: invalid profile id {}", profile.id))?;
            validate_id(&profile.app)
                .with_context(|| format!("ProfileInvalid: invalid app id {}", profile.app))?;
            if !ids.insert(profile.id.as_str()) {
                return Err(anyhow!(
                    "ProfileInvalid: duplicate profile id {}",
                    profile.id
                ));
            }
        }
        if let Some(default_app) = &self.preferences.default_app {
            validate_id(default_app)
                .with_context(|| format!("ProfileInvalid: invalid default_app {default_app}"))?;
        }
        Ok(())
    }

    pub fn find(&self, id: &str) -> Option<&Profile> {
        self.profiles.iter().find(|profile| profile.id == id)
    }

    pub fn remove(&mut self, id: &str) -> Option<Profile> {
        let pos = self.profiles.iter().position(|profile| profile.id == id)?;
        Some(self.profiles.remove(pos))
    }
}

pub fn slug(input: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in input.chars().flat_map(|ch| ch.to_lowercase()) {
        if ch.is_ascii_lowercase() || ch.is_ascii_digit() {
            out.push(ch);
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

pub fn build_profile_id(app: &str, name: &str, explicit: Option<&str>) -> Result<String> {
    let id = if let Some(id) = explicit {
        id.to_string()
    } else {
        let slug = slug(name);
        if slug.is_empty() {
            return Err(anyhow!("profile name cannot produce a slug; pass --id"));
        }
        let prefix = format!("{app}-");
        let max_slug_len = 64usize.saturating_sub(prefix.len());
        format!("{}{}", prefix, &slug[..slug.len().min(max_slug_len)])
    };
    validate_id(&id)?;
    Ok(id)
}

pub fn new_profile(
    app: &str,
    kind: &str,
    name: &str,
    id: String,
    fields: IndexMap<String, Value>,
) -> Profile {
    Profile {
        id,
        app: app.to_string(),
        kind: kind.to_string(),
        schema_version: 1,
        name: name.to_string(),
        notes: String::new(),
        created_at: Utc::now().to_rfc3339(),
        fields,
        identity: IndexMap::new(),
        capture: None,
        extensions: Value::Object(Default::default()),
    }
}

pub fn new_oauth_profile(
    app: &str,
    name: &str,
    id: String,
    identity: IndexMap<String, Value>,
    capture: Value,
) -> Profile {
    Profile {
        id,
        app: app.to_string(),
        kind: "oauth_capture".to_string(),
        schema_version: 1,
        name: name.to_string(),
        notes: String::new(),
        created_at: Utc::now().to_rfc3339(),
        fields: IndexMap::new(),
        identity,
        capture: Some(capture),
        extensions: Value::Object(Default::default()),
    }
}

pub fn validate_static_profile(definition: &AppDefinition, profile: &Profile) -> Result<()> {
    let kind = definition
        .kinds
        .get(&profile.kind)
        .ok_or_else(|| anyhow!("KindNotSupported: {}", profile.kind))?;
    if profile.kind == "oauth_capture" {
        return Err(anyhow!(
            "oauth_capture profiles must be created by import-current"
        ));
    }
    validate_fields(&profile.fields, &kind.field_schema, "")?;
    Ok(())
}

pub fn validate_fields(
    fields: &IndexMap<String, Value>,
    schema: &BTreeMap<String, FieldSchema>,
    prefix: &str,
) -> Result<()> {
    for (key, field_schema) in schema {
        let full_key = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        if field_schema.required && !fields.contains_key(key) {
            return Err(anyhow!("FieldMissing: {full_key}"));
        }
        if let Some(value) = fields.get(key) {
            validate_field_type(value, field_schema, &full_key)?;
            if !field_schema.fields.is_empty() {
                let object = value
                    .as_object()
                    .ok_or_else(|| anyhow!("FieldMissing: {full_key} must be an object"))?;
                let nested: IndexMap<String, Value> =
                    object.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                validate_fields(&nested, &field_schema.fields, &full_key)?;
            }
        }
    }
    Ok(())
}

fn validate_field_type(value: &Value, schema: &FieldSchema, key: &str) -> Result<()> {
    match schema.field_type.as_deref() {
        Some("string") if !value.is_string() => {
            Err(anyhow!("FieldMissing: {key} must be a string"))
        }
        Some("object") if !value.is_object() => {
            Err(anyhow!("FieldMissing: {key} must be an object"))
        }
        _ => Ok(()),
    }
}

pub fn reject_unsafe_field_arg(key: &str, schema: &BTreeMap<String, FieldSchema>) -> Result<()> {
    let root = key.split('.').next().unwrap_or(key);
    if is_sensitive_field(root, schema.get(root)) {
        return Err(anyhow!(
            "UnsafeSecretArgument: sensitive field {key} must use --secret-field"
        ));
    }
    Ok(())
}

pub fn set_dotted_field(fields: &mut IndexMap<String, Value>, key: &str, value: Value) {
    let parts: Vec<_> = key.split('.').collect();
    if parts.len() == 1 {
        fields.insert(key.to_string(), value);
        return;
    }
    let root = parts[0].to_string();
    let entry = fields
        .entry(root)
        .or_insert_with(|| Value::Object(Default::default()));
    let mut current = entry;
    for part in &parts[1..parts.len() - 1] {
        if !current.is_object() {
            *current = Value::Object(Default::default());
        }
        current = current
            .as_object_mut()
            .unwrap()
            .entry((*part).to_string())
            .or_insert_with(|| Value::Object(Default::default()));
    }
    if !current.is_object() {
        *current = Value::Object(Default::default());
    }
    current
        .as_object_mut()
        .unwrap()
        .insert(parts.last().unwrap().to_string(), value);
}

pub fn apply_defaults(
    fields: &mut IndexMap<String, Value>,
    schema: &BTreeMap<String, FieldSchema>,
) {
    for (key, field_schema) in schema {
        if let Some(value) = fields.get_mut(key) {
            apply_nested_defaults(value, &field_schema.fields);
        } else if let Some(value) = default_value(field_schema) {
            fields.insert(key.clone(), value);
        }
    }
}

fn default_value(schema: &FieldSchema) -> Option<Value> {
    let mut value = schema.default.clone().or_else(|| {
        let mut object = serde_json::Map::new();
        apply_defaults_to_object(&mut object, &schema.fields);
        if object.is_empty() {
            None
        } else {
            Some(Value::Object(object))
        }
    })?;
    apply_nested_defaults(&mut value, &schema.fields);
    Some(value)
}

fn apply_nested_defaults(value: &mut Value, schema: &BTreeMap<String, FieldSchema>) {
    if schema.is_empty() {
        return;
    }
    if let Some(object) = value.as_object_mut() {
        apply_defaults_to_object(object, schema);
    }
}

fn apply_defaults_to_object(
    fields: &mut serde_json::Map<String, Value>,
    schema: &BTreeMap<String, FieldSchema>,
) {
    for (key, field_schema) in schema {
        if let Some(value) = fields.get_mut(key) {
            apply_nested_defaults(value, &field_schema.fields);
        } else if let Some(value) = default_value(field_schema) {
            fields.insert(key.clone(), value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_definitions::FieldSchema;

    #[test]
    fn slug_drops_non_ascii() {
        assert_eq!(slug("GLM 智谱"), "glm");
    }

    #[test]
    fn dotted_field_builds_object() {
        let mut fields = IndexMap::new();
        set_dotted_field(&mut fields, "models.default", Value::String("x".into()));
        assert_eq!(fields["models"]["default"], "x");
    }

    #[test]
    fn defaults_apply_recursively_without_overwriting_user_values() {
        let leaf = FieldSchema {
            default: Some(Value::String("default-model".into())),
            ..FieldSchema::default()
        };
        let other = FieldSchema {
            default: Some(Value::String("other-model".into())),
            ..FieldSchema::default()
        };
        let mut nested_schema = BTreeMap::new();
        nested_schema.insert("default".to_string(), leaf);
        nested_schema.insert("other".to_string(), other);
        let object = FieldSchema {
            fields: nested_schema,
            ..FieldSchema::default()
        };
        let mut schema = BTreeMap::new();
        schema.insert("models".to_string(), object);
        let mut fields = IndexMap::new();
        set_dotted_field(
            &mut fields,
            "models.default",
            Value::String("custom-model".into()),
        );

        apply_defaults(&mut fields, &schema);

        assert_eq!(fields["models"]["default"], "custom-model");
        assert_eq!(fields["models"]["other"], "other-model");
    }

    #[test]
    fn validate_fields_rejects_wrong_scalar_type() {
        let mut schema = BTreeMap::new();
        schema.insert(
            "api_key".to_string(),
            FieldSchema {
                field_type: Some("string".to_string()),
                required: true,
                ..FieldSchema::default()
            },
        );
        let mut fields = IndexMap::new();
        fields.insert("api_key".to_string(), Value::Bool(true));

        let err = validate_fields(&fields, &schema, "").unwrap_err();
        assert!(err.to_string().contains("api_key must be a string"));
    }
}
