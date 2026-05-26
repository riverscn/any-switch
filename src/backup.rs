use crate::app_definitions::validate_id;
use crate::handlers;
use crate::keychain;
use crate::paths::{
    ensure_existing_ancestor_inside_home, ensure_inside_home, set_mode, write_private,
    write_private_following_symlink, Paths,
};
use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::cmp::Reverse;
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupManifest {
    pub schema_version: u32,
    pub operation_id: String,
    pub app: String,
    pub created_at: String,
    pub targets: Vec<BackupTarget>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupTarget {
    pub target_id: String,
    #[serde(rename = "type")]
    pub target_type: String,
    pub requires_app_stopped: bool,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub resolved_path: String,
    #[serde(default)]
    pub json_path: Option<String>,
    #[serde(default)]
    pub toml_paths: Vec<String>,
    #[serde(default)]
    pub backend: Option<String>,
    #[serde(default)]
    pub service: Option<String>,
    #[serde(default)]
    pub account: Option<String>,
    #[serde(default)]
    pub resolved_account: Option<String>,
    pub stored_as: String,
    pub sha256: String,
}

#[derive(Debug, Clone)]
pub enum BackupInput {
    File {
        path: PathBuf,
        requires_app_stopped: bool,
    },
    JsonSubtree {
        path: PathBuf,
        json_path: String,
        requires_app_stopped: bool,
    },
    TomlManagedPaths {
        path: PathBuf,
        toml_paths: Vec<String>,
        requires_app_stopped: bool,
    },
    SecretEntry {
        backend: String,
        service: String,
        account: String,
        resolved_account: String,
        requires_app_stopped: bool,
    },
}

impl BackupInput {
    pub fn file(path: PathBuf, requires_app_stopped: bool) -> Self {
        Self::File {
            path,
            requires_app_stopped,
        }
    }

    pub fn target_id(&self) -> String {
        match self {
            Self::File { path, .. } => format!("file:{}", path.display()),
            Self::JsonSubtree {
                path, json_path, ..
            } => format!("json:{}#{json_path}", path.display()),
            Self::TomlManagedPaths {
                path, toml_paths, ..
            } => {
                format!("toml:{}#{}", path.display(), toml_paths.join(","))
            }
            Self::SecretEntry {
                backend,
                service,
                resolved_account,
                ..
            } => format!("keychain:{backend}:{service}:{resolved_account}"),
        }
    }

    pub fn resolved_path(&self) -> Option<&Path> {
        match self {
            Self::File { path, .. }
            | Self::JsonSubtree { path, .. }
            | Self::TomlManagedPaths { path, .. } => Some(path.as_path()),
            Self::SecretEntry { .. } => None,
        }
    }

    pub fn requires_app_stopped(&self) -> bool {
        match self {
            Self::File {
                requires_app_stopped,
                ..
            }
            | Self::JsonSubtree {
                requires_app_stopped,
                ..
            }
            | Self::TomlManagedPaths {
                requires_app_stopped,
                ..
            }
            | Self::SecretEntry {
                requires_app_stopped,
                ..
            } => *requires_app_stopped,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupUsage {
    pub app: String,
    pub backup_count: usize,
    pub logical_bytes: u64,
    pub inode_bytes: u64,
}

pub fn create_backup(paths: &Paths, app: &str, target_paths: &[PathBuf]) -> Result<String> {
    let inputs = target_paths
        .iter()
        .cloned()
        .map(|path| BackupInput::file(path, false))
        .collect::<Vec<_>>();
    create_backup_with_inputs(paths, app, &inputs)
}

pub fn create_backup_with_inputs(
    paths: &Paths,
    app: &str,
    inputs: &[BackupInput],
) -> Result<String> {
    let operation_id = uuid::Uuid::now_v7().to_string();
    let backup_id = format!("{}-{operation_id}", Utc::now().format("%Y%m%dT%H%M%S%.3fZ"));
    let backup_dir = paths.backups_dir().join(app).join(&backup_id);
    fs::create_dir_all(&backup_dir)?;
    set_mode(&backup_dir, 0o700)?;

    let mut targets = Vec::new();
    for (idx, input) in inputs.iter().enumerate() {
        let stored_as = format!("target-{idx}.bak");
        let stored_path = backup_dir.join(&stored_as);
        let bytes = read_backup_input(input)?;
        let sha256 = sha256_hex(&bytes);
        if !try_hardlink_existing_backup(paths, app, &sha256, &stored_path)? {
            write_private(&stored_path, &bytes)?;
        }
        targets.push(input.to_manifest_target(stored_as, sha256));
    }

    let manifest = BackupManifest {
        schema_version: 1,
        operation_id,
        app: app.to_string(),
        created_at: Utc::now().to_rfc3339(),
        targets,
    };
    write_private(
        &backup_dir.join("manifest.json"),
        &serde_json::to_vec_pretty(&manifest)?,
    )?;
    Ok(backup_id)
}

fn try_hardlink_existing_backup(
    paths: &Paths,
    app: &str,
    sha256: &str,
    destination: &Path,
) -> Result<bool> {
    let Some(candidate) = find_backup_blob_by_sha(paths, app, sha256)? else {
        return Ok(false);
    };
    if !is_safe_hardlink_candidate(&candidate)? {
        return Ok(false);
    }
    match fs::hard_link(&candidate, destination) {
        Ok(()) => Ok(true),
        Err(_) => Ok(false),
    }
}

fn find_backup_blob_by_sha(paths: &Paths, app: &str, sha256: &str) -> Result<Option<PathBuf>> {
    let app_dir = paths.backups_dir().join(app);
    if !app_dir.exists() {
        return Ok(None);
    }
    for entry in fs::read_dir(app_dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let manifest_path = entry.path().join("manifest.json");
        if !manifest_path.exists() {
            continue;
        }
        let text = fs::read_to_string(&manifest_path)?;
        let manifest: BackupManifest = serde_json::from_str(&text)?;
        for target in manifest.targets {
            if target.sha256 == sha256 {
                let candidate = entry.path().join(target.stored_as);
                if candidate.exists() {
                    return Ok(Some(candidate));
                }
            }
        }
    }
    Ok(None)
}

#[cfg(unix)]
fn is_safe_hardlink_candidate(path: &Path) -> Result<bool> {
    use std::os::unix::fs::MetadataExt;
    let metadata = fs::metadata(path)?;
    Ok(metadata.is_file()
        && metadata.mode() & 0o777 == 0o600
        && metadata.uid() == unsafe { libc::getuid() })
}

#[cfg(not(unix))]
fn is_safe_hardlink_candidate(path: &Path) -> Result<bool> {
    Ok(fs::metadata(path)?.is_file())
}

pub fn restore_backup(paths: &Paths, app: &str, backup_id: &str) -> Result<()> {
    let backup_dir = paths.backups_dir().join(app).join(backup_id);
    let manifest = load_validated_manifest(paths, app, backup_id)?;
    for target in manifest.targets {
        let blob = backup_dir.join(&target.stored_as);
        let bytes = fs::read(&blob)?;
        let actual = sha256_hex(&bytes);
        if actual != target.sha256 {
            return Err(anyhow!(
                "BackupInvalid: {} hash mismatch for {}",
                blob.display(),
                target.stored_as
            ));
        }
        let path = PathBuf::from(&target.resolved_path);
        restore_target(&target, &bytes, &path)?;
    }
    Ok(())
}

pub fn live_matches_backup(paths: &Paths, app: &str, backup_id: &str) -> Result<bool> {
    let manifest = load_validated_manifest(paths, app, backup_id)?;
    let inputs = inputs_from_manifest(&manifest)?;
    for (target, input) in manifest.targets.iter().zip(inputs.iter()) {
        let Ok(bytes) = read_backup_input(input) else {
            return Ok(false);
        };
        if sha256_hex(&bytes) != target.sha256 {
            return Ok(false);
        }
    }
    Ok(true)
}

fn read_backup_input(input: &BackupInput) -> Result<Vec<u8>> {
    match input {
        BackupInput::File { path, .. } => {
            if path.exists() {
                fs::read(path).with_context(|| path.display().to_string())
            } else {
                Ok(Vec::new())
            }
        }
        BackupInput::JsonSubtree {
            path, json_path, ..
        } => {
            if !path.exists() {
                return Ok(serde_json::to_vec_pretty(&serde_json::Value::Null)?);
            }
            let value =
                handlers::read_json_path(path, json_path)?.unwrap_or(serde_json::Value::Null);
            Ok(serde_json::to_vec_pretty(&value)?)
        }
        BackupInput::TomlManagedPaths {
            path, toml_paths, ..
        } => {
            if path.exists() {
                Ok(handlers::capture_toml_fragment(path, toml_paths)?.into_bytes())
            } else {
                Ok(Vec::new())
            }
        }
        BackupInput::SecretEntry {
            service,
            resolved_account,
            ..
        } => keychain::read_generic_password(service, resolved_account),
    }
}

impl BackupInput {
    fn to_manifest_target(&self, stored_as: String, sha256: String) -> BackupTarget {
        match self {
            BackupInput::File {
                path,
                requires_app_stopped,
            } => BackupTarget {
                target_id: self.target_id(),
                target_type: "file".to_string(),
                requires_app_stopped: *requires_app_stopped,
                path: path.display().to_string(),
                resolved_path: path.display().to_string(),
                json_path: None,
                toml_paths: Vec::new(),
                backend: None,
                service: None,
                account: None,
                resolved_account: None,
                stored_as,
                sha256,
            },
            BackupInput::JsonSubtree {
                path,
                json_path,
                requires_app_stopped,
            } => BackupTarget {
                target_id: self.target_id(),
                target_type: "json_subtree".to_string(),
                requires_app_stopped: *requires_app_stopped,
                path: path.display().to_string(),
                resolved_path: path.display().to_string(),
                json_path: Some(json_path.clone()),
                toml_paths: Vec::new(),
                backend: None,
                service: None,
                account: None,
                resolved_account: None,
                stored_as,
                sha256,
            },
            BackupInput::TomlManagedPaths {
                path,
                toml_paths,
                requires_app_stopped,
            } => BackupTarget {
                target_id: self.target_id(),
                target_type: "toml_managed_paths".to_string(),
                requires_app_stopped: *requires_app_stopped,
                path: path.display().to_string(),
                resolved_path: path.display().to_string(),
                json_path: None,
                toml_paths: toml_paths.clone(),
                backend: None,
                service: None,
                account: None,
                resolved_account: None,
                stored_as,
                sha256,
            },
            BackupInput::SecretEntry {
                backend,
                service,
                account,
                resolved_account,
                requires_app_stopped,
            } => BackupTarget {
                target_id: self.target_id(),
                target_type: "secret_entry".to_string(),
                requires_app_stopped: *requires_app_stopped,
                path: String::new(),
                resolved_path: String::new(),
                json_path: None,
                toml_paths: Vec::new(),
                backend: Some(backend.clone()),
                service: Some(service.clone()),
                account: Some(account.clone()),
                resolved_account: Some(resolved_account.clone()),
                stored_as,
                sha256,
            },
        }
    }
}

fn restore_target(target: &BackupTarget, bytes: &[u8], path: &Path) -> Result<()> {
    match target.target_type.as_str() {
        "file" => write_private_following_symlink(path, bytes),
        "json_subtree" => {
            let json_path = target
                .json_path
                .as_deref()
                .ok_or_else(|| anyhow!("BackupInvalid: json_subtree missing json_path"))?;
            let value: serde_json::Value = serde_json::from_slice(bytes)?;
            handlers::write_json_path(path, json_path, value)
        }
        "toml_managed_paths" => {
            let text = std::str::from_utf8(bytes)?;
            handlers::merge_toml_fragment(path, text, &target.toml_paths)
        }
        "secret_entry" => {
            let service = target
                .service
                .as_deref()
                .ok_or_else(|| anyhow!("BackupInvalid: secret_entry missing service"))?;
            let account = target
                .resolved_account
                .as_deref()
                .ok_or_else(|| anyhow!("BackupInvalid: secret_entry missing resolved_account"))?;
            keychain::write_generic_password(service, account, bytes)
        }
        other => Err(anyhow!("BackupInvalid: unsupported target type {other}")),
    }
}

pub fn load_validated_manifest(
    paths: &Paths,
    app: &str,
    backup_id: &str,
) -> Result<BackupManifest> {
    validate_backup_lookup(app, backup_id)?;
    let backup_dir = paths.backups_dir().join(app).join(backup_id);
    let manifest_path = backup_dir.join("manifest.json");
    let text =
        fs::read_to_string(&manifest_path).with_context(|| manifest_path.display().to_string())?;
    let mut manifest: BackupManifest = serde_json::from_str(&text)
        .with_context(|| format!("BackupInvalid: parse {}", manifest_path.display()))?;
    normalize_legacy_manifest(&mut manifest);
    if manifest.schema_version != 1 {
        return Err(anyhow!(
            "BackupInvalid: unsupported schema_version {}",
            manifest.schema_version
        ));
    }
    if manifest.app != app {
        return Err(anyhow!(
            "BackupInvalid: manifest app {} does not match {app}",
            manifest.app
        ));
    }
    for target in &manifest.targets {
        validate_stored_as(&target.stored_as)?;
        validate_target_spec(paths, target)?;
        let blob = backup_dir.join(&target.stored_as);
        if !blob.exists() {
            return Err(anyhow!("BackupInvalid: missing {}", blob.display()));
        }
        let bytes = fs::read(&blob)?;
        let actual = sha256_hex(&bytes);
        if actual != target.sha256 {
            return Err(anyhow!(
                "BackupInvalid: {} hash mismatch for {}",
                blob.display(),
                target.stored_as
            ));
        }
    }
    Ok(manifest)
}

fn validate_backup_lookup(app: &str, backup_id: &str) -> Result<()> {
    validate_id(app).map_err(|err| anyhow!("BackupInvalid: invalid app id {app}: {err}"))?;
    if backup_id.is_empty()
        || backup_id.len() > 128
        || backup_id.contains('/')
        || backup_id.contains('\\')
        || backup_id == "."
        || backup_id == ".."
        || !backup_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '.' || ch == '_' || ch == '-')
    {
        return Err(anyhow!("BackupInvalid: invalid backup_id {backup_id}"));
    }
    Ok(())
}

fn normalize_legacy_manifest(manifest: &mut BackupManifest) {
    let _ = manifest;
}

fn validate_target_spec(paths: &Paths, target: &BackupTarget) -> Result<()> {
    match target.target_type.as_str() {
        "file" | "json_subtree" | "toml_managed_paths" => {
            let resolved_path = PathBuf::from(&target.resolved_path);
            ensure_inside_home(&resolved_path, &paths.home)
                .map_err(|err| anyhow!("BackupInvalid: {err}"))?;
            ensure_existing_ancestor_inside_home(&resolved_path, &paths.home)
                .map_err(|err| anyhow!("BackupInvalid: {err}"))?;
            paths
                .ensure_outside_switch_home(&resolved_path)
                .map_err(|err| anyhow!("BackupInvalid: {err}"))?;
            match target.target_type.as_str() {
                "json_subtree" if target.json_path.is_none() => {
                    return Err(anyhow!("BackupInvalid: json_subtree missing json_path"));
                }
                "toml_managed_paths" if target.toml_paths.is_empty() => {
                    return Err(anyhow!(
                        "BackupInvalid: toml_managed_paths missing toml_paths"
                    ));
                }
                _ => {}
            }
        }
        "secret_entry" => {
            let backend = target
                .backend
                .as_deref()
                .ok_or_else(|| anyhow!("BackupInvalid: secret_entry missing backend"))?;
            if backend != "macos_keychain" {
                return Err(anyhow!(
                    "BackupInvalid: unsupported secret backend {backend}"
                ));
            }
            if target.service.as_deref().unwrap_or_default().is_empty() {
                return Err(anyhow!("BackupInvalid: secret_entry missing service"));
            }
            if target
                .resolved_account
                .as_deref()
                .unwrap_or_default()
                .is_empty()
            {
                return Err(anyhow!(
                    "BackupInvalid: secret_entry missing resolved_account"
                ));
            }
        }
        other => {
            return Err(anyhow!("BackupInvalid: unsupported target type {other}"));
        }
    }
    Ok(())
}

pub fn inputs_from_manifest(manifest: &BackupManifest) -> Result<Vec<BackupInput>> {
    manifest
        .targets
        .iter()
        .map(|target| match target.target_type.as_str() {
            "file" => Ok(BackupInput::File {
                path: PathBuf::from(&target.resolved_path),
                requires_app_stopped: target.requires_app_stopped,
            }),
            "json_subtree" => Ok(BackupInput::JsonSubtree {
                path: PathBuf::from(&target.resolved_path),
                json_path: target
                    .json_path
                    .clone()
                    .ok_or_else(|| anyhow!("BackupInvalid: json_subtree missing json_path"))?,
                requires_app_stopped: target.requires_app_stopped,
            }),
            "toml_managed_paths" => Ok(BackupInput::TomlManagedPaths {
                path: PathBuf::from(&target.resolved_path),
                toml_paths: target.toml_paths.clone(),
                requires_app_stopped: target.requires_app_stopped,
            }),
            "secret_entry" => Ok(BackupInput::SecretEntry {
                backend: target
                    .backend
                    .clone()
                    .ok_or_else(|| anyhow!("BackupInvalid: secret_entry missing backend"))?,
                service: target
                    .service
                    .clone()
                    .ok_or_else(|| anyhow!("BackupInvalid: secret_entry missing service"))?,
                account: target.account.clone().unwrap_or_default(),
                resolved_account: target.resolved_account.clone().ok_or_else(|| {
                    anyhow!("BackupInvalid: secret_entry missing resolved_account")
                })?,
                requires_app_stopped: target.requires_app_stopped,
            }),
            other => Err(anyhow!("BackupInvalid: unsupported target type {other}")),
        })
        .collect()
}

fn validate_stored_as(stored_as: &str) -> Result<()> {
    if stored_as.is_empty()
        || stored_as == "manifest.json"
        || stored_as.starts_with('/')
        || stored_as
            .split('/')
            .any(|part| part == "." || part == ".." || part.is_empty())
    {
        return Err(anyhow!("BackupInvalid: invalid stored_as {stored_as}"));
    }
    Ok(())
}

pub fn list_backups(paths: &Paths, app: Option<&str>) -> Result<Vec<(String, String)>> {
    if let Some(app) = app {
        validate_id(app).map_err(|err| anyhow!("BackupInvalid: invalid app id {app}: {err}"))?;
    }
    let mut rows = Vec::new();
    let root = paths.backups_dir();
    if !root.exists() {
        return Ok(rows);
    }
    for app_entry in fs::read_dir(root)? {
        let app_entry = app_entry?;
        if !app_entry.file_type()?.is_dir() {
            continue;
        }
        let app_id = app_entry.file_name().to_string_lossy().to_string();
        if app.is_some_and(|wanted| wanted != app_id) {
            continue;
        }
        for backup_entry in fs::read_dir(app_entry.path())? {
            let backup_entry = backup_entry?;
            let backup_id = backup_entry.file_name().to_string_lossy().to_string();
            if backup_entry.file_type()?.is_dir()
                && validate_backup_lookup(&app_id, &backup_id).is_ok()
            {
                rows.push((app_id.clone(), backup_id));
            }
        }
    }
    rows.sort();
    Ok(rows)
}

pub fn prune_backups(paths: &Paths, app: &str, keep: usize) -> Result<()> {
    let dir = paths.backups_dir().join(app);
    if !dir.exists() {
        return Ok(());
    }
    let mut entries = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            let modified = entry.metadata()?.modified()?;
            entries.push((modified, entry.path()));
        }
    }
    entries.sort_by_key(|entry| Reverse(entry.0));
    for (_, path) in entries.into_iter().skip(keep) {
        fs::remove_dir_all(path)?;
    }
    Ok(())
}

pub fn backup_usage(paths: &Paths, app: Option<&str>) -> Result<Vec<BackupUsage>> {
    let root = paths.backups_dir();
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut by_app = BTreeMap::new();
    for (app_id, backup_id) in list_backups(paths, app)? {
        let backup_dir = root.join(&app_id).join(&backup_id);
        let manifest_path = backup_dir.join("manifest.json");
        if !manifest_path.exists() {
            continue;
        }
        let text = fs::read_to_string(manifest_path)?;
        let manifest: BackupManifest = serde_json::from_str(&text)?;
        let usage = by_app.entry(app_id.clone()).or_insert(BackupUsage {
            app: app_id,
            backup_count: 0,
            logical_bytes: 0,
            inode_bytes: 0,
        });
        usage.backup_count += 1;
        for target in manifest.targets {
            if validate_stored_as(&target.stored_as).is_err() {
                continue;
            }
            let file = backup_dir.join(target.stored_as);
            if let Ok(metadata) = fs::metadata(&file) {
                usage.logical_bytes += metadata.len();
            }
        }
    }

    for usage in by_app.values_mut() {
        usage.inode_bytes = unique_backup_inode_bytes(paths, &usage.app)?;
    }
    Ok(by_app.into_values().collect())
}

#[cfg(unix)]
fn unique_backup_inode_bytes(paths: &Paths, app: &str) -> Result<u64> {
    use std::os::unix::fs::MetadataExt;
    let app_dir = paths.backups_dir().join(app);
    let mut seen = HashSet::new();
    let mut bytes = 0;
    if !app_dir.exists() {
        return Ok(0);
    }
    for backup_entry in fs::read_dir(app_dir)? {
        let backup_entry = backup_entry?;
        if !backup_entry.file_type()?.is_dir() {
            continue;
        }
        for file_entry in fs::read_dir(backup_entry.path())? {
            let file_entry = file_entry?;
            if !file_entry.file_type()?.is_file() {
                continue;
            }
            if file_entry.file_name() == "manifest.json" {
                continue;
            }
            let metadata = file_entry.metadata()?;
            if seen.insert((metadata.dev(), metadata.ino())) {
                bytes += metadata.len();
            }
        }
    }
    Ok(bytes)
}

#[cfg(not(unix))]
fn unique_backup_inode_bytes(paths: &Paths, app: &str) -> Result<u64> {
    let app_dir = paths.backups_dir().join(app);
    let mut bytes = 0;
    if !app_dir.exists() {
        return Ok(0);
    }
    for backup_entry in fs::read_dir(app_dir)? {
        let backup_entry = backup_entry?;
        if !backup_entry.file_type()?.is_dir() {
            continue;
        }
        for file_entry in fs::read_dir(backup_entry.path())? {
            let file_entry = file_entry?;
            if file_entry.file_type()?.is_file() && file_entry.file_name() != "manifest.json" {
                bytes += file_entry.metadata()?.len();
            }
        }
    }
    Ok(bytes)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn file_sha256(path: &Path) -> Result<String> {
    Ok(sha256_hex(&fs::read(path)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[cfg(unix)]
    #[test]
    fn identical_backup_blobs_are_hardlinked_and_counted_once() {
        use std::os::unix::fs::MetadataExt;

        let home = tempdir().unwrap();
        let switch_home = tempdir().unwrap();
        let paths = Paths {
            home: home.path().to_path_buf(),
            switch_home: switch_home.path().to_path_buf(),
        };
        let target = home.path().join("target.json");
        fs::write(&target, b"same").unwrap();

        let first = create_backup(&paths, "codex", std::slice::from_ref(&target)).unwrap();
        let second = create_backup(&paths, "codex", std::slice::from_ref(&target)).unwrap();

        let first_blob = paths
            .backups_dir()
            .join("codex")
            .join(first)
            .join("target-0.bak");
        let second_blob = paths
            .backups_dir()
            .join("codex")
            .join(second)
            .join("target-0.bak");
        let first_metadata = fs::metadata(first_blob).unwrap();
        let second_metadata = fs::metadata(second_blob).unwrap();
        assert_eq!(first_metadata.ino(), second_metadata.ino());

        let usage = backup_usage(&paths, Some("codex")).unwrap();
        assert_eq!(usage.len(), 1);
        assert_eq!(usage[0].backup_count, 2);
        assert_eq!(usage[0].logical_bytes, 8);
        assert_eq!(usage[0].inode_bytes, 4);
    }
}
