use crate::paths::Paths;
use anyhow::{anyhow, Context, Result};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppDefinition {
    pub schema_version: u32,
    pub app: AppMeta,
    #[serde(default)]
    pub process_probe: ProcessProbe,
    #[serde(default)]
    pub doctor: DoctorDefinition,
    #[serde(default)]
    pub guards: Vec<GuardDefinition>,
    #[serde(default)]
    pub override_checks: Vec<OverrideCheckDefinition>,
    #[serde(default)]
    pub kinds: BTreeMap<String, KindDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppMeta {
    pub id: String,
    pub display_name: String,
    pub definition_version: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessProbe {
    #[serde(default)]
    pub names: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DoctorDefinition {
    #[serde(default)]
    pub json_fields: Vec<DoctorJsonField>,
    #[serde(default)]
    pub json_object_schemas: Vec<DoctorJsonObjectSchema>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DoctorJsonField {
    pub name: String,
    pub path: String,
    pub json_path: String,
    #[serde(default)]
    pub stale_after_days: Option<i64>,
    #[serde(default)]
    pub sensitive: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DoctorJsonObjectSchema {
    pub name: String,
    pub path: String,
    pub json_path: String,
    #[serde(default)]
    pub known_keys: Vec<String>,
    #[serde(default)]
    pub extra_keys_warning: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GuardDefinition {
    pub handler: String,
    pub path: String,
    #[serde(default)]
    pub toml_path: Option<String>,
    #[serde(default)]
    pub allowed_values: Vec<String>,
    #[serde(default)]
    pub missing_ok: bool,
    #[serde(default)]
    pub error_kind: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OverrideCheckDefinition {
    pub handler: String,
    #[serde(default)]
    pub applies_to_kinds: Vec<String>,
    #[serde(default)]
    pub env_names: Vec<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub json_path: Option<String>,
    #[serde(default)]
    pub keys: Vec<String>,
    #[serde(default)]
    pub reason_prefix: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub test_env: Option<String>,
    #[serde(default)]
    pub macos_dir: Option<String>,
    #[serde(default)]
    pub linux_dir: Option<String>,
    #[serde(default)]
    pub windows_dir: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KindDefinition {
    #[serde(default)]
    pub field_schema: BTreeMap<String, FieldSchema>,
    #[serde(default)]
    pub targets: Vec<TargetDefinition>,
    #[serde(default)]
    pub capture_sources: Vec<CaptureSourceDefinition>,
    #[serde(default)]
    pub cleanup_targets: Vec<CleanupTargetDefinition>,
    #[serde(default)]
    pub identity: Option<IdentityDefinition>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureSourceDefinition {
    pub handler: String,
    #[serde(default)]
    pub backend: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub service: Option<String>,
    #[serde(default)]
    pub account: Option<String>,
    pub stored_as: String,
    #[serde(default)]
    pub required: Option<bool>,
    #[serde(default)]
    pub platforms: Vec<String>,
    #[serde(default)]
    pub identity: Option<IdentityDefinition>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CleanupTargetDefinition {
    pub handler: String,
    pub path: String,
    pub json_path: String,
    #[serde(default)]
    pub keys: Vec<String>,
    #[serde(default)]
    pub requires_app_stopped: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldSchema {
    #[serde(rename = "type")]
    pub field_type: Option<String>,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub sensitive: Option<bool>,
    #[serde(default)]
    pub fields: BTreeMap<String, FieldSchema>,
    #[serde(default)]
    pub default: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdentityDefinition {
    pub handler: Option<String>,
    #[serde(default)]
    pub fields: BTreeMap<String, IdentityField>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdentityField {
    pub path: String,
    #[serde(default = "optional_verify")]
    pub verify: String,
}

fn optional_verify() -> String {
    "optional".to_string()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetDefinition {
    pub handler: String,
    pub path: String,
    #[serde(default)]
    pub json_path: Option<String>,
    #[serde(default)]
    pub managed_keys: Vec<String>,
    #[serde(default)]
    pub mapping: IndexMap<String, String>,
    #[serde(default)]
    pub template: Option<String>,
    #[serde(default)]
    pub toml_paths: Vec<String>,
    #[serde(default)]
    pub import_json_matches: IndexMap<String, serde_json::Value>,
    #[serde(default)]
    pub import_json_required_strings: Vec<String>,
    #[serde(default)]
    pub import_json_forbidden_strings: Vec<String>,
    #[serde(default)]
    pub import_json_fields: IndexMap<String, String>,
    #[serde(default)]
    pub import_unmatched_error: Option<String>,
    #[serde(default)]
    pub requires_app_stopped: bool,
}

#[derive(Debug, Clone)]
pub struct DefinitionRegistry {
    apps: BTreeMap<String, LoadedDefinition>,
}

#[derive(Debug, Clone)]
pub struct LoadedDefinition {
    pub source: DefinitionSource,
    pub definition: AppDefinition,
}

#[derive(Debug, Clone, Serialize)]
pub enum DefinitionSource {
    System,
    User,
    Override,
}

impl DefinitionRegistry {
    pub fn load(paths: &Paths) -> Result<Self> {
        let mut apps = BTreeMap::new();
        for asset in BUILTIN_DEFINITIONS {
            let definition = parse_definition(asset.text).with_context(|| {
                format!("failed to load built-in app definition {}", asset.name)
            })?;
            validate_definition(&definition)?;
            validate_definition_paths(&definition, paths)?;
            apps.insert(
                definition.app.id.clone(),
                LoadedDefinition {
                    source: DefinitionSource::System,
                    definition,
                },
            );
        }

        let user_dir = paths.switch_home.join("apps.d");
        if user_dir.exists() {
            for entry in fs::read_dir(&user_dir).with_context(|| user_dir.display().to_string())? {
                let path = entry?.path();
                if path.extension().and_then(|v| v.to_str()) != Some("yaml") {
                    continue;
                }
                let text = fs::read_to_string(&path)?;
                let definition = parse_definition(&text)
                    .with_context(|| format!("failed to load {}", path.display()))?;
                validate_definition(&definition)?;
                validate_definition_paths(&definition, paths)?;
                if apps.contains_key(&definition.app.id) {
                    return Err(anyhow!(
                        "DefinitionLoadFailed: user definition {} duplicates existing app {}",
                        path.display(),
                        definition.app.id
                    ));
                }
                apps.insert(
                    definition.app.id.clone(),
                    LoadedDefinition {
                        source: DefinitionSource::User,
                        definition,
                    },
                );
            }
        }

        let override_dir = paths.switch_home.join("overrides.d");
        if override_dir.exists() {
            for entry in
                fs::read_dir(&override_dir).with_context(|| override_dir.display().to_string())?
            {
                let path = entry?.path();
                if path.extension().and_then(|v| v.to_str()) != Some("yaml") {
                    continue;
                }
                let text = fs::read_to_string(&path)?;
                let override_definition = parse_definition(&text)
                    .with_context(|| format!("failed to load override {}", path.display()))?;
                validate_definition(&override_definition)?;
                let loaded = apps.get_mut(&override_definition.app.id).ok_or_else(|| {
                    anyhow!(
                        "DefinitionLoadFailed: override {} references unknown app {}",
                        path.display(),
                        override_definition.app.id
                    )
                })?;
                apply_override(&mut loaded.definition, &override_definition)?;
                validate_definition(&loaded.definition)?;
                validate_definition_paths(&loaded.definition, paths)?;
                loaded.source = DefinitionSource::Override;
            }
        }

        Ok(Self { apps })
    }

    pub fn get(&self, app: &str) -> Result<&LoadedDefinition> {
        self.apps
            .get(app)
            .ok_or_else(|| anyhow!("AppNotFound: {app}"))
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &LoadedDefinition)> {
        self.apps.iter()
    }
}

pub fn validate_definition_paths(definition: &AppDefinition, paths: &Paths) -> Result<()> {
    for guard in &definition.guards {
        paths.expand_target_path(&guard.path).with_context(|| {
            format!(
                "DefinitionLoadFailed: app {} guard {} target {} is not allowed",
                definition.app.id, guard.handler, guard.path
            )
        })?;
    }
    for check in &definition.override_checks {
        if let Some(path) = &check.path {
            paths.expand_target_path(path).with_context(|| {
                format!(
                    "DefinitionLoadFailed: app {} override check target {} is not allowed",
                    definition.app.id, path
                )
            })?;
        }
    }
    for field in &definition.doctor.json_fields {
        paths.expand_target_path(&field.path).with_context(|| {
            format!(
                "DefinitionLoadFailed: app {} doctor json field {} target {} is not allowed",
                definition.app.id, field.name, field.path
            )
        })?;
    }
    for schema in &definition.doctor.json_object_schemas {
        paths.expand_target_path(&schema.path).with_context(|| {
            format!(
                "DefinitionLoadFailed: app {} doctor json object schema {} target {} is not allowed",
                definition.app.id, schema.name, schema.path
            )
        })?;
    }
    for kind in definition.kinds.values() {
        for source in &kind.capture_sources {
            if let Some(path) = &source.path {
                paths.expand_target_path(path).with_context(|| {
                    format!(
                        "DefinitionLoadFailed: app {} capture source {} is not allowed",
                        definition.app.id, path
                    )
                })?;
            }
        }
        for cleanup in &kind.cleanup_targets {
            paths.expand_target_path(&cleanup.path).with_context(|| {
                format!(
                    "DefinitionLoadFailed: app {} cleanup target {} is not allowed",
                    definition.app.id, cleanup.path
                )
            })?;
        }
        for target in &kind.targets {
            paths.expand_target_path(&target.path).with_context(|| {
                format!(
                    "DefinitionLoadFailed: app {} target {} is not allowed",
                    definition.app.id, target.path
                )
            })?;
        }
    }
    Ok(())
}

fn apply_override(base: &mut AppDefinition, override_definition: &AppDefinition) -> Result<()> {
    if base.app.id != override_definition.app.id {
        return Err(anyhow!("DefinitionLoadFailed: override app id mismatch"));
    }
    for name in &override_definition.process_probe.names {
        if !base.process_probe.names.contains(name) {
            base.process_probe.names.push(name.clone());
        }
    }
    for (kind_name, override_kind) in &override_definition.kinds {
        let base_kind = base.kinds.get_mut(kind_name).ok_or_else(|| {
            anyhow!(
                "DefinitionLoadFailed: override references unknown kind {}",
                kind_name
            )
        })?;
        if !override_kind.targets.is_empty() {
            return Err(anyhow!(
                "DefinitionLoadFailed: overrides may not replace targets or handlers"
            ));
        }
        if !override_kind.capture_sources.is_empty() || !override_kind.cleanup_targets.is_empty() {
            return Err(anyhow!(
                "DefinitionLoadFailed: overrides may not replace capture sources or cleanup targets"
            ));
        }
        merge_field_schema(&mut base_kind.field_schema, &override_kind.field_schema)?;
        if override_kind.identity.is_some() {
            return Err(anyhow!(
                "DefinitionLoadFailed: overrides may not replace oauth identity extractors"
            ));
        }
    }
    Ok(())
}

fn merge_field_schema(
    base: &mut BTreeMap<String, FieldSchema>,
    override_schema: &BTreeMap<String, FieldSchema>,
) -> Result<()> {
    for (name, override_field) in override_schema {
        let base_field = base.get_mut(name).ok_or_else(|| {
            anyhow!(
                "DefinitionLoadFailed: override references unknown field {}",
                name
            )
        })?;
        if override_field.default.is_some() {
            base_field.default = override_field.default.clone();
        }
        if override_field.sensitive.is_some() {
            base_field.sensitive = override_field.sensitive;
        }
        if !override_field.fields.is_empty() {
            merge_field_schema(&mut base_field.fields, &override_field.fields)?;
        }
    }
    Ok(())
}

pub fn parse_definition(text: &str) -> Result<AppDefinition> {
    let definition: AppDefinition = serde_yaml::from_str(text)?;
    Ok(definition)
}

pub fn system_definition(app: &str) -> Result<Option<AppDefinition>> {
    for asset in BUILTIN_DEFINITIONS {
        let definition = parse_definition(asset.text)
            .with_context(|| format!("failed to load built-in app definition {}", asset.name))?;
        if definition.app.id == app {
            return Ok(Some(definition));
        }
    }
    Ok(None)
}

pub fn validate_definition(definition: &AppDefinition) -> Result<()> {
    if definition.schema_version != 1 || definition.app.definition_version != 1 {
        return Err(anyhow!(
            "DefinitionLoadFailed: only schema/definition version 1 is supported"
        ));
    }
    validate_id(&definition.app.id)?;
    validate_doctor_definition(&definition.doctor)?;
    validate_guards(&definition.guards)?;
    validate_override_checks(&definition.override_checks)?;
    let known_handlers = [
        "json_env_merge",
        "json_subtree",
        "file_capture",
        "toml_managed_paths",
        "json_paths",
        "jwt_payload_json_paths",
        "process_name",
    ];
    for (kind, kind_def) in &definition.kinds {
        if !matches!(
            kind.as_str(),
            "env_injection" | "file_template" | "oauth_capture" | "opaque_capture"
        ) {
            return Err(anyhow!("KindNotSupported: {kind}"));
        }
        if kind == "opaque_capture" {
            return Err(anyhow!(
                "KindNotSupported: opaque_capture is reserved but not implemented"
            ));
        }
        validate_field_schema(&kind_def.field_schema)?;
        if kind == "oauth_capture" {
            let identity = kind_def.identity.as_ref().ok_or_else(|| {
                anyhow!("DefinitionLoadFailed: oauth_capture requires identity handler")
            })?;
            let handler = identity.handler.as_deref().ok_or_else(|| {
                anyhow!("DefinitionLoadFailed: oauth_capture requires identity handler")
            })?;
            if !known_handlers.contains(&handler) {
                return Err(anyhow!("UnknownHandler: {handler}"));
            }
            validate_identity_definition(identity)?;
            let required_count = identity
                .fields
                .values()
                .filter(|field| field.verify == "required")
                .count();
            if required_count == 0 {
                return Err(anyhow!(
                    "DefinitionLoadFailed: oauth_capture requires at least one required identity field"
                ));
            }
            if definition.process_probe.names.is_empty() {
                return Err(anyhow!(
                    "DefinitionLoadFailed: oauth_capture requires process_probe.names"
                ));
            }
        }
        for target in &kind_def.targets {
            if !known_handlers.contains(&target.handler.as_str()) {
                return Err(anyhow!("UnknownHandler: {}", target.handler));
            }
            validate_target_definition(kind, target)?;
        }
        for source in &kind_def.capture_sources {
            validate_capture_source_definition(source)?;
        }
        for cleanup in &kind_def.cleanup_targets {
            validate_cleanup_target_definition(cleanup)?;
        }
    }
    Ok(())
}

fn validate_override_checks(checks: &[OverrideCheckDefinition]) -> Result<()> {
    for check in checks {
        match check.handler.as_str() {
            "process_env_non_empty" => {
                if check.env_names.is_empty() {
                    return Err(anyhow!(
                        "DefinitionLoadFailed: process_env_non_empty override check requires env_names"
                    ));
                }
            }
            "json_object_keys_non_empty" => {
                if check.path.is_none() || check.json_path.is_none() || check.keys.is_empty() {
                    return Err(anyhow!(
                        "DefinitionLoadFailed: json_object_keys_non_empty override check requires path, json_path, and keys"
                    ));
                }
                validate_simple_json_path(check.json_path.as_deref().unwrap())?;
            }
            "json_string_non_empty" => {
                if check.path.is_none() || check.json_path.is_none() || check.reason.is_none() {
                    return Err(anyhow!(
                        "DefinitionLoadFailed: json_string_non_empty override check requires path, json_path, and reason"
                    ));
                }
                validate_simple_json_path(check.json_path.as_deref().unwrap())?;
            }
            "managed_json_object_keys_non_empty" => {
                if check.json_path.is_none() || check.keys.is_empty() {
                    return Err(anyhow!(
                        "DefinitionLoadFailed: managed_json_object_keys_non_empty override check requires json_path and keys"
                    ));
                }
                validate_simple_json_path(check.json_path.as_deref().unwrap())?;
            }
            "managed_json_path_present" => {
                if check.json_path.is_none() || check.reason.is_none() {
                    return Err(anyhow!(
                        "DefinitionLoadFailed: managed_json_path_present override check requires json_path and reason"
                    ));
                }
                validate_simple_json_path(check.json_path.as_deref().unwrap())?;
            }
            other => return Err(anyhow!("UnknownHandler: {other}")),
        }
    }
    Ok(())
}

fn validate_capture_source_definition(source: &CaptureSourceDefinition) -> Result<()> {
    match source.handler.as_str() {
        "file_capture" => {
            if source.path.is_none() {
                return Err(anyhow!(
                    "DefinitionLoadFailed: file_capture source requires path"
                ));
            }
        }
        "secret_entry" => {
            if source.service.is_none() || source.account.is_none() {
                return Err(anyhow!(
                    "DefinitionLoadFailed: secret_entry source requires service and account"
                ));
            }
        }
        other => return Err(anyhow!("UnknownHandler: {other}")),
    }
    validate_stored_as(&source.stored_as)?;
    for platform in &source.platforms {
        if !matches!(platform.as_str(), "macos" | "linux" | "windows") {
            return Err(anyhow!(
                "DefinitionLoadFailed: capture source {} has unsupported platform {}",
                source.stored_as,
                platform
            ));
        }
    }
    if let Some(identity) = &source.identity {
        validate_identity_definition(identity)?;
    }
    Ok(())
}

fn validate_cleanup_target_definition(cleanup: &CleanupTargetDefinition) -> Result<()> {
    match cleanup.handler.as_str() {
        "json_remove_keys" => {
            validate_simple_json_path(&cleanup.json_path)?;
            if cleanup.keys.is_empty() {
                return Err(anyhow!(
                    "DefinitionLoadFailed: json_remove_keys cleanup target requires keys"
                ));
            }
        }
        other => return Err(anyhow!("UnknownHandler: {other}")),
    }
    Ok(())
}

fn validate_stored_as(stored_as: &str) -> Result<()> {
    if stored_as.is_empty()
        || stored_as == "manifest.json"
        || stored_as.contains('/')
        || stored_as.contains('\\')
        || stored_as.split('/').any(|part| part == "." || part == "..")
    {
        return Err(anyhow!(
            "DefinitionLoadFailed: invalid stored_as {stored_as}"
        ));
    }
    Ok(())
}

fn validate_guards(guards: &[GuardDefinition]) -> Result<()> {
    for guard in guards {
        match guard.handler.as_str() {
            "toml_string_allowlist" => {
                let toml_path = guard.toml_path.as_deref().ok_or_else(|| {
                    anyhow!("DefinitionLoadFailed: toml_string_allowlist guard requires toml_path")
                })?;
                validate_toml_path(toml_path)?;
                if guard.allowed_values.is_empty() {
                    return Err(anyhow!(
                        "DefinitionLoadFailed: toml_string_allowlist guard requires allowed_values"
                    ));
                }
            }
            other => return Err(anyhow!("UnknownHandler: {other}")),
        }
    }
    Ok(())
}

fn validate_doctor_definition(doctor: &DoctorDefinition) -> Result<()> {
    for field in &doctor.json_fields {
        validate_diagnostic_name(&field.name)?;
        validate_simple_json_path(&field.json_path)?;
        if field.stale_after_days.is_some_and(|days| days <= 0) {
            return Err(anyhow!(
                "DefinitionLoadFailed: doctor json field {} stale_after_days must be positive",
                field.name
            ));
        }
    }
    for schema in &doctor.json_object_schemas {
        validate_diagnostic_name(&schema.name)?;
        validate_simple_json_path(&schema.json_path)?;
        if schema.known_keys.is_empty() {
            return Err(anyhow!(
                "DefinitionLoadFailed: doctor json object schema {} requires known_keys",
                schema.name
            ));
        }
        for key in &schema.known_keys {
            if key.is_empty() {
                return Err(anyhow!(
                    "DefinitionLoadFailed: doctor json object schema {} contains an empty known key",
                    schema.name
                ));
            }
        }
    }
    Ok(())
}

fn validate_diagnostic_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 64
        || !name
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' || ch == '-')
    {
        return Err(anyhow!(
            "DefinitionLoadFailed: invalid doctor field name {name}"
        ));
    }
    Ok(())
}

fn validate_target_definition(kind: &str, target: &TargetDefinition) -> Result<()> {
    for path in target.import_json_matches.keys() {
        validate_simple_json_path(path)?;
    }
    for path in &target.import_json_required_strings {
        validate_simple_json_path(path)?;
    }
    for path in &target.import_json_forbidden_strings {
        validate_simple_json_path(path)?;
    }
    for path in target.import_json_fields.values() {
        validate_simple_json_path(path)?;
    }
    match target.handler.as_str() {
        "json_env_merge" => {
            let json_path = target.json_path.as_deref().ok_or_else(|| {
                anyhow!("DefinitionLoadFailed: json_env_merge target requires json_path")
            })?;
            validate_simple_json_path(json_path)?;
            if target.managed_keys.is_empty() {
                return Err(anyhow!(
                    "DefinitionLoadFailed: json_env_merge target requires managed_keys"
                ));
            }
        }
        "json_subtree" => {
            let json_path = target.json_path.as_deref().ok_or_else(|| {
                anyhow!("DefinitionLoadFailed: json_subtree target requires json_path")
            })?;
            validate_simple_json_path(json_path)?;
        }
        "toml_managed_paths" => {
            if target.toml_paths.is_empty() {
                return Err(anyhow!(
                    "DefinitionLoadFailed: toml_managed_paths target requires toml_paths"
                ));
            }
        }
        _ => {
            if let Some(path) = &target.json_path {
                validate_simple_json_path(path)?;
            }
        }
    }
    if kind == "oauth_capture" {
        if !matches!(
            target.handler.as_str(),
            "file_capture" | "json_subtree" | "toml_managed_paths"
        ) {
            return Err(anyhow!(
                "DefinitionLoadFailed: oauth_capture target handler {} is not supported",
                target.handler
            ));
        }
        if !target.requires_app_stopped {
            return Err(anyhow!(
                "DefinitionLoadFailed: oauth_capture targets must require app stopped"
            ));
        }
    }
    Ok(())
}

fn validate_toml_path(path: &str) -> Result<()> {
    if path.is_empty()
        || path.split('.').any(|segment| {
            segment.is_empty()
                || !segment
                    .chars()
                    .all(|ch| ch == '_' || ch == '-' || ch.is_ascii_alphanumeric())
        })
    {
        return Err(anyhow!(
            "DefinitionLoadFailed: unsupported toml_path {path}"
        ));
    }
    Ok(())
}

fn validate_identity_definition(identity: &IdentityDefinition) -> Result<()> {
    for (name, field) in &identity.fields {
        if !matches!(field.verify.as_str(), "required" | "optional") {
            return Err(anyhow!(
                "DefinitionLoadFailed: identity field {name} has unsupported verify value {}",
                field.verify
            ));
        }
        validate_simple_json_path(&field.path)?;
    }
    Ok(())
}

fn validate_field_schema(schema: &BTreeMap<String, FieldSchema>) -> Result<()> {
    for field in schema.values() {
        if let Some(field_type) = field.field_type.as_deref() {
            if !matches!(field_type, "string" | "object") {
                return Err(anyhow!(
                    "DefinitionLoadFailed: unsupported field type {field_type}"
                ));
            }
        }
        validate_field_schema(&field.fields)?;
    }
    Ok(())
}

pub fn validate_id(id: &str) -> Result<()> {
    let mut chars = id.chars();
    let Some(first) = chars.next() else {
        return Err(anyhow!("invalid id: empty"));
    };
    if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        return Err(anyhow!("invalid id: {id}"));
    }
    if id.len() > 64
        || !id
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
    {
        return Err(anyhow!("invalid id: {id}"));
    }
    Ok(())
}

pub fn validate_simple_json_path(path: &str) -> Result<()> {
    if path == "$" {
        return Ok(());
    }
    if !path.starts_with("$.") {
        return Err(anyhow!(
            "DefinitionLoadFailed: unsupported json_path {path}"
        ));
    }
    for segment in path.trim_start_matches("$.").split('.') {
        if segment.is_empty()
            || !segment
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
        {
            return Err(anyhow!(
                "DefinitionLoadFailed: unsupported json_path {path}"
            ));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct BuiltinDefinitionAsset {
    name: &'static str,
    text: &'static str,
}

const BUILTIN_DEFINITIONS: &[BuiltinDefinitionAsset] =
    include!(concat!(env!("OUT_DIR"), "/builtin_definitions.rs"));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_validate() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths {
            home: dir.path().to_path_buf(),
            switch_home: dir.path().join(".switch-cli"),
        };
        let registry = DefinitionRegistry::load(&paths).unwrap();
        assert_eq!(registry.iter().count(), BUILTIN_DEFINITIONS.len());
        for asset in BUILTIN_DEFINITIONS {
            let definition = parse_definition(asset.text).unwrap();
            assert!(registry.get(&definition.app.id).is_ok());
        }
    }

    #[test]
    fn system_definition_returns_raw_builtin() {
        let first = parse_definition(BUILTIN_DEFINITIONS[0].text).unwrap();
        let definition = system_definition(&first.app.id).unwrap().unwrap();
        assert_eq!(definition.app.id, first.app.id);
        assert!(system_definition("missing").unwrap().is_none());
    }

    #[test]
    fn rejects_wildcard_json_path() {
        assert!(validate_simple_json_path("$.a.*").is_err());
    }

    #[test]
    fn rejects_unknown_field_type() {
        let yaml = r#"
schema_version: 1
app:
  id: custom
  display_name: Custom
  definition_version: 1
kinds:
  env_injection:
    field_schema:
      enabled:
        type: boolean
"#;
        let definition = parse_definition(yaml).unwrap();
        let err = validate_definition(&definition).unwrap_err();
        assert!(err.to_string().contains("unsupported field type boolean"));
    }

    #[test]
    fn rejects_reserved_opaque_capture_until_implemented() {
        let yaml = r#"
schema_version: 1
app:
  id: custom
  display_name: Custom
  definition_version: 1
kinds:
  opaque_capture: {}
"#;
        let definition = parse_definition(yaml).unwrap();
        let err = validate_definition(&definition).unwrap_err();
        assert!(err
            .to_string()
            .contains("opaque_capture is reserved but not implemented"));
    }

    #[test]
    fn rejects_login_or_reauth_definition_fields() {
        let yaml = r#"
schema_version: 1
app:
  id: custom
  display_name: Custom
  definition_version: 1
login:
  command: custom login
kinds:
  env_injection:
    field_schema:
      token:
        type: string
"#;
        let err = parse_definition(yaml).unwrap_err();
        assert!(err.to_string().contains("unknown field `login`"));
    }

    #[test]
    fn rejects_executable_target_command_field() {
        let yaml = r#"
schema_version: 1
app:
  id: custom
  display_name: Custom
  definition_version: 1
kinds:
  file_template:
    targets:
      - handler: file_capture
        path: ~/.custom/auth.json
        command: custom login
"#;
        let err = parse_definition(yaml).unwrap_err();
        assert!(err.to_string().contains("unknown field `command`"));
    }

    #[test]
    fn validates_doctor_json_fields() {
        let yaml = r#"
schema_version: 1
app:
  id: custom
  display_name: Custom
  definition_version: 1
doctor:
  json_fields:
    - name: last_refresh
      path: ~/.custom/auth.json
      json_path: $.last_refresh
      stale_after_days: 90
kinds: {}
"#;
        let definition = parse_definition(yaml).unwrap();
        validate_definition(&definition).unwrap();
    }

    #[test]
    fn rejects_invalid_doctor_json_fields() {
        let yaml = r#"
schema_version: 1
app:
  id: custom
  display_name: Custom
  definition_version: 1
doctor:
  json_fields:
    - name: LastRefresh
      path: ~/.custom/auth.json
      json_path: $.last_refresh
kinds: {}
"#;
        let definition = parse_definition(yaml).unwrap();
        let err = validate_definition(&definition).unwrap_err();
        assert!(err
            .to_string()
            .contains("invalid doctor field name LastRefresh"));
    }

    #[test]
    fn rejects_unknown_identity_handler() {
        let yaml = r#"
schema_version: 1
app:
  id: custom
  display_name: Custom
  definition_version: 1
process_probe:
  names: [custom]
kinds:
  oauth_capture:
    identity:
      handler: shell_script
      fields:
        account_id:
          path: $.account_id
          verify: required
"#;
        let definition = parse_definition(yaml).unwrap();
        let err = validate_definition(&definition).unwrap_err();
        assert!(err.to_string().contains("UnknownHandler: shell_script"));
    }

    #[test]
    fn rejects_oauth_target_without_app_stopped_requirement() {
        let yaml = r#"
schema_version: 1
app:
  id: custom
  display_name: Custom
  definition_version: 1
process_probe:
  names: [custom]
kinds:
  oauth_capture:
    identity:
      handler: json_paths
      fields:
        account_id:
          path: $.account_id
          verify: required
    targets:
      - handler: file_capture
        path: ~/.custom/auth.json
"#;
        let definition = parse_definition(yaml).unwrap();
        let err = validate_definition(&definition).unwrap_err();
        assert!(err
            .to_string()
            .contains("oauth_capture targets must require app stopped"));
    }

    #[test]
    fn rejects_oauth_definition_without_process_probe() {
        let yaml = r#"
schema_version: 1
app:
  id: custom
  display_name: Custom
  definition_version: 1
kinds:
  oauth_capture:
    identity:
      handler: json_paths
      fields:
        account_id:
          path: $.account_id
          verify: required
    targets:
      - handler: file_capture
        path: ~/.custom/auth.json
        requires_app_stopped: true
"#;
        let definition = parse_definition(yaml).unwrap();
        let err = validate_definition(&definition).unwrap_err();
        assert!(err
            .to_string()
            .contains("oauth_capture requires process_probe.names"));
    }

    #[test]
    fn rejects_json_target_without_json_path() {
        let yaml = r#"
schema_version: 1
app:
  id: custom
  display_name: Custom
  definition_version: 1
kinds:
  env_injection:
    field_schema:
      token:
        type: string
        required: true
    targets:
      - handler: json_env_merge
        path: ~/.custom/settings.json
        managed_keys: [TOKEN]
"#;
        let definition = parse_definition(yaml).unwrap();
        let err = validate_definition(&definition).unwrap_err();
        assert!(err
            .to_string()
            .contains("json_env_merge target requires json_path"));
    }

    #[test]
    fn rejects_toml_target_without_managed_paths() {
        let yaml = r#"
schema_version: 1
app:
  id: custom
  display_name: Custom
  definition_version: 1
kinds:
  file_template:
    field_schema:
      model:
        type: string
    targets:
      - handler: toml_managed_paths
        path: ~/.custom/config.toml
"#;
        let definition = parse_definition(yaml).unwrap();
        let err = validate_definition(&definition).unwrap_err();
        assert!(err
            .to_string()
            .contains("toml_managed_paths target requires toml_paths"));
    }

    #[test]
    fn rejects_invalid_identity_verify_value() {
        let yaml = r#"
schema_version: 1
app:
  id: custom
  display_name: Custom
  definition_version: 1
process_probe:
  names: [custom]
kinds:
  oauth_capture:
    identity:
      handler: json_paths
      fields:
        account_id:
          path: $.account_id
          verify: blocking
    targets:
      - handler: file_capture
        path: ~/.custom/auth.json
        requires_app_stopped: true
"#;
        let definition = parse_definition(yaml).unwrap();
        let err = validate_definition(&definition).unwrap_err();
        assert!(err
            .to_string()
            .contains("unsupported verify value blocking"));
    }

    #[test]
    fn rejects_invalid_identity_json_path() {
        let yaml = r#"
schema_version: 1
app:
  id: custom
  display_name: Custom
  definition_version: 1
process_probe:
  names: [custom]
kinds:
  oauth_capture:
    identity:
      handler: json_paths
      fields:
        account_id:
          path: $.accounts.*
          verify: required
    targets:
      - handler: file_capture
        path: ~/.custom/auth.json
        requires_app_stopped: true
"#;
        let definition = parse_definition(yaml).unwrap();
        let err = validate_definition(&definition).unwrap_err();
        assert!(err
            .to_string()
            .contains("unsupported json_path $.accounts.*"));
    }
}
