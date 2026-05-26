use crate::backup::sha256_hex;
use crate::handlers;
use crate::keychain;
use crate::paths::{ensure_dir_private, write_private, write_private_following_symlink, Paths};
use crate::profiles::Profile;
use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureSpec {
    pub sources: Vec<CaptureSourceSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureSourceSpec {
    #[serde(rename = "type")]
    pub source_type: String,
    #[serde(default)]
    pub backend: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub service: Option<String>,
    #[serde(default)]
    pub account: Option<String>,
    #[serde(default)]
    pub json_path: Option<String>,
    #[serde(default)]
    pub toml_paths: Vec<String>,
    pub stored_as: String,
    #[serde(default)]
    pub required: Option<bool>,
    #[serde(default)]
    pub platforms: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureManifest {
    pub schema_version: u32,
    pub profile_id: String,
    pub captured_at: String,
    #[serde(default)]
    pub last_writeback_at: Option<String>,
    pub sources: Vec<CaptureManifestSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureManifestSource {
    pub stored_as: String,
    pub sha256: String,
}

impl CaptureSpec {
    pub fn from_profile(profile: &Profile) -> Result<Self> {
        let value = profile
            .capture
            .clone()
            .ok_or_else(|| anyhow!("CaptureMissing: profile {} has no capture", profile.id))?;
        let spec: Self = serde_json::from_value(value)?;
        validate_capture_spec(&spec)?;
        Ok(spec)
    }

    pub fn to_value(&self) -> Result<Value> {
        Ok(serde_json::to_value(self)?)
    }
}

fn validate_capture_spec(spec: &CaptureSpec) -> Result<()> {
    if spec.sources.is_empty() {
        return Err(anyhow!("CaptureMissing: capture.sources is empty"));
    }
    for source in &spec.sources {
        validate_capture_source(source)?;
    }
    Ok(())
}

fn validate_capture_source(source: &CaptureSourceSpec) -> Result<()> {
    validate_stored_as(&source.stored_as)?;
    for platform in &source.platforms {
        if !matches!(platform.as_str(), "macos" | "linux" | "windows") {
            return Err(anyhow!(
                "CaptureInvalid: source {} has unsupported platform {}",
                source.stored_as,
                platform
            ));
        }
    }
    match source.source_type.as_str() {
        "file" => {
            if source.path.is_none() {
                return Err(anyhow!(
                    "CaptureInvalid: source {} missing path",
                    source.stored_as
                ));
            }
        }
        "toml_managed_paths" => {
            if source.path.is_none() {
                return Err(anyhow!(
                    "CaptureInvalid: source {} missing path",
                    source.stored_as
                ));
            }
            if source.toml_paths.is_empty() {
                return Err(anyhow!(
                    "CaptureInvalid: source {} missing toml_paths",
                    source.stored_as
                ));
            }
        }
        "json_subtree" => {
            if source.path.is_none() {
                return Err(anyhow!(
                    "CaptureInvalid: source {} missing path",
                    source.stored_as
                ));
            }
            let Some(json_path) = source.json_path.as_deref() else {
                return Err(anyhow!(
                    "CaptureInvalid: source {} missing json_path",
                    source.stored_as
                ));
            };
            crate::app_definitions::validate_simple_json_path(json_path)?;
        }
        "secret_entry" => {
            if source.backend.as_deref().unwrap_or("macos_keychain") != "macos_keychain" {
                return Err(anyhow!(
                    "CaptureInvalid: source {} has unsupported secret backend",
                    source.stored_as
                ));
            }
            if source.service.is_none() || source.account.is_none() {
                return Err(anyhow!(
                    "CaptureInvalid: source {} missing service/account",
                    source.stored_as
                ));
            }
        }
        other => {
            return Err(anyhow!(
                "UnknownHandler: unsupported capture source {other}"
            ))
        }
    }
    Ok(())
}

impl CaptureSourceSpec {
    pub fn required(&self) -> bool {
        self.required.unwrap_or(true)
    }

    pub fn applies_to_current_platform(&self) -> bool {
        if self.platforms.is_empty() {
            return true;
        }
        let current = if cfg!(target_os = "macos") {
            "macos"
        } else if cfg!(target_os = "linux") {
            "linux"
        } else if cfg!(target_os = "windows") {
            "windows"
        } else {
            "unknown"
        };
        self.platforms.iter().any(|platform| platform == current)
    }
}

pub fn capture_dir(paths: &Paths, profile_id: &str) -> PathBuf {
    paths.switch_home.join("captures").join(profile_id)
}

pub fn manifest_path(paths: &Paths, profile_id: &str) -> PathBuf {
    capture_dir(paths, profile_id).join("manifest.json")
}

pub fn write_capture_files(
    paths: &Paths,
    profile_id: &str,
    files: Vec<(String, Vec<u8>)>,
    writeback: bool,
) -> Result<()> {
    let dir = capture_dir(paths, profile_id);
    ensure_dir_private(&dir)?;
    let mut sources = Vec::new();
    for (stored_as, bytes) in files {
        let path = capture_blob_path(paths, profile_id, &stored_as)?;
        write_private(&path, &bytes)?;
        sources.push(CaptureManifestSource {
            stored_as,
            sha256: sha256_hex(&bytes),
        });
    }
    let previous = load_manifest(paths, profile_id).ok();
    let now = Utc::now().to_rfc3339();
    let manifest = CaptureManifest {
        schema_version: 1,
        profile_id: profile_id.to_string(),
        captured_at: if writeback {
            previous
                .as_ref()
                .map(|manifest| manifest.captured_at.clone())
                .unwrap_or_else(|| now.clone())
        } else {
            now.clone()
        },
        last_writeback_at: if writeback { Some(now) } else { None },
        sources,
    };
    write_private(
        &manifest_path(paths, profile_id),
        &serde_json::to_vec_pretty(&manifest)?,
    )?;
    Ok(())
}

pub fn load_manifest(paths: &Paths, profile_id: &str) -> Result<CaptureManifest> {
    let path = manifest_path(paths, profile_id);
    let text = fs::read_to_string(&path).with_context(|| path.display().to_string())?;
    Ok(serde_json::from_str(&text)?)
}

pub fn ensure_capture_complete(paths: &Paths, profile: &Profile) -> Result<()> {
    let spec = CaptureSpec::from_profile(profile)?;
    let manifest = load_manifest(paths, &profile.id)?;
    for source in spec
        .sources
        .iter()
        .filter(|source| source.applies_to_current_platform())
    {
        let expected = manifest
            .sources
            .iter()
            .find(|entry| entry.stored_as == source.stored_as)
            .map(|entry| entry.sha256.as_str());
        if !source.required() && expected.is_none() {
            continue;
        }
        let path = capture_blob_path(paths, &profile.id, &source.stored_as)?;
        if !path.exists() {
            return Err(anyhow!(
                "CaptureMissing: profile {} is missing {}; run `any-switch import-current {} <name>` on this machine to re-capture credentials for the current platform",
                profile.id,
                source.stored_as,
                profile.app
            ));
        }
        let bytes = fs::read(&path)?;
        let actual = sha256_hex(&bytes);
        if expected != Some(actual.as_str()) {
            return Err(anyhow!(
                "CaptureMissing: profile {} capture {} hash mismatch",
                profile.id,
                source.stored_as
            ));
        }
    }
    Ok(())
}

pub fn read_live_source(paths: &Paths, source: &CaptureSourceSpec) -> Result<Option<Vec<u8>>> {
    if !source.applies_to_current_platform() {
        return Ok(None);
    }
    match source.source_type.as_str() {
        "file" => {
            let path = source_path(paths, source)?;
            if !path.exists() {
                if source.required() {
                    return Err(anyhow!("TargetMissing: {}", path.display()));
                }
                return Ok(None);
            }
            Ok(Some(fs::read(path)?))
        }
        "json_subtree" => {
            let path = source_path(paths, source)?;
            if !path.exists() {
                if source.required() {
                    return Err(anyhow!("TargetMissing: {}", path.display()));
                }
                return Ok(None);
            }
            let json_path = source
                .json_path
                .as_deref()
                .ok_or_else(|| anyhow!("json_subtree source missing json_path"))?;
            let Some(value) = handlers::read_json_path(&path, json_path)? else {
                if source.required() {
                    return Err(anyhow!("TargetMissing: {} {}", path.display(), json_path));
                }
                return Ok(None);
            };
            Ok(Some(serde_json::to_vec_pretty(&value)?))
        }
        "toml_managed_paths" => {
            let path = source_path(paths, source)?;
            if !path.exists() {
                if source.required() {
                    return Err(anyhow!("TargetMissing: {}", path.display()));
                }
                return Ok(None);
            }
            Ok(Some(
                handlers::capture_toml_fragment(&path, &source.toml_paths)?.into_bytes(),
            ))
        }
        "secret_entry" => {
            let (service, account) = source_secret_entry(source)?;
            Ok(Some(keychain::read_generic_password(&service, &account)?))
        }
        other => Err(anyhow!(
            "UnknownHandler: unsupported capture source {other}"
        )),
    }
}

pub fn write_live_source(
    paths: &Paths,
    source: &CaptureSourceSpec,
    bytes: &[u8],
) -> Result<Option<PathBuf>> {
    if !source.applies_to_current_platform() {
        return Ok(None);
    }
    match source.source_type.as_str() {
        "file" => {
            let path = source_path(paths, source)?;
            write_private_following_symlink(&path, bytes)?;
            Ok(Some(path))
        }
        "json_subtree" => {
            let path = source_path(paths, source)?;
            let json_path = source
                .json_path
                .as_deref()
                .ok_or_else(|| anyhow!("json_subtree source missing json_path"))?;
            let value: Value = serde_json::from_slice(bytes)?;
            handlers::write_json_path(&path, json_path, value)?;
            Ok(Some(path))
        }
        "toml_managed_paths" => {
            let path = source_path(paths, source)?;
            let text = std::str::from_utf8(bytes)?;
            handlers::merge_toml_fragment(&path, text, &source.toml_paths)?;
            Ok(Some(path))
        }
        "secret_entry" => {
            let (service, account) = source_secret_entry(source)?;
            keychain::write_generic_password(&service, &account, bytes)?;
            Ok(None)
        }
        other => Err(anyhow!(
            "UnknownHandler: unsupported capture source {other}"
        )),
    }
}

pub fn source_path(paths: &Paths, source: &CaptureSourceSpec) -> Result<PathBuf> {
    let template = source
        .path
        .as_deref()
        .ok_or_else(|| anyhow!("capture source {} missing path", source.stored_as))?;
    paths.expand_target_path(template)
}

fn source_secret_entry(source: &CaptureSourceSpec) -> Result<(String, String)> {
    let service = source
        .service
        .clone()
        .ok_or_else(|| anyhow!("secret_entry source {} missing service", source.stored_as))?;
    let account = source
        .account
        .clone()
        .ok_or_else(|| anyhow!("secret_entry source {} missing account", source.stored_as))?;
    Ok((
        service,
        account.replace("${MACOS_USER}", &crate::paths::current_os_user()),
    ))
}

pub fn capture_files_from_live(
    paths: &Paths,
    spec: &CaptureSpec,
) -> Result<Vec<(String, Vec<u8>)>> {
    let mut files = Vec::new();
    for source in &spec.sources {
        if let Some(bytes) = read_live_source(paths, source)? {
            files.push((source.stored_as.clone(), bytes));
        }
    }
    Ok(files)
}

pub fn apply_capture_to_live(paths: &Paths, profile: &Profile) -> Result<Vec<PathBuf>> {
    ensure_capture_complete(paths, profile)?;
    let spec = CaptureSpec::from_profile(profile)?;
    let mut targets = Vec::new();
    for source in &spec.sources {
        if !source.applies_to_current_platform() {
            continue;
        }
        let file = capture_blob_path(paths, &profile.id, &source.stored_as)?;
        if !file.exists() {
            if source.required() {
                return Err(anyhow!("CaptureMissing: {}", file.display()));
            }
            continue;
        }
        let bytes = fs::read(file)?;
        if let Some(path) = write_live_source(paths, source, &bytes)? {
            targets.push(path);
        }
    }
    Ok(targets)
}

pub fn live_matches_capture(paths: &Paths, profile: &Profile) -> Result<bool> {
    ensure_capture_complete(paths, profile)?;
    let spec = CaptureSpec::from_profile(profile)?;
    for source in spec
        .sources
        .iter()
        .filter(|source| source.applies_to_current_platform())
    {
        let file = capture_blob_path(paths, &profile.id, &source.stored_as)?;
        if !file.exists() {
            if source.required() {
                return Ok(false);
            }
            continue;
        }
        let expected = fs::read(file)?;
        let Some(actual) = read_live_source(paths, source)? else {
            return Ok(false);
        };
        if sha256_hex(&actual) != sha256_hex(&expected) {
            return Ok(false);
        }
    }
    Ok(true)
}

fn validate_stored_as(stored_as: &str) -> Result<()> {
    if stored_as.is_empty()
        || stored_as == "manifest.json"
        || stored_as.starts_with("manifest.json/")
        || stored_as.starts_with('/')
        || stored_as
            .split('/')
            .any(|part| part == "." || part == ".." || part.is_empty())
    {
        return Err(anyhow!("invalid capture stored_as: {stored_as}"));
    }
    Ok(())
}

fn capture_blob_path(paths: &Paths, profile_id: &str, stored_as: &str) -> Result<PathBuf> {
    validate_stored_as(stored_as)?;
    let dir = capture_dir(paths, profile_id);
    let path = dir.join(stored_as);
    ensure_capture_path_inside_dir(&dir, &path)?;
    Ok(path)
}

fn ensure_capture_path_inside_dir(dir: &Path, path: &Path) -> Result<()> {
    let real_dir = dir
        .canonicalize()
        .with_context(|| format!("canonicalize capture dir {}", dir.display()))?;
    let mut cursor = path;
    while !cursor.exists() {
        let Some(parent) = cursor.parent() else {
            return Ok(());
        };
        cursor = parent;
    }
    let real = cursor
        .canonicalize()
        .with_context(|| format!("canonicalize capture path {}", cursor.display()))?;
    if !real.starts_with(&real_dir) {
        return Err(anyhow!(
            "CaptureInvalid: capture stored_as resolves outside capture dir: {}",
            path.display()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::Paths;
    use crate::profiles::Profile;
    use indexmap::IndexMap;
    use serde_json::json;
    #[cfg(unix)]
    use std::os::unix::fs::symlink;

    fn profile_with_capture(capture: Value) -> Profile {
        Profile {
            id: "codex-test".to_string(),
            app: "codex".to_string(),
            kind: "oauth_capture".to_string(),
            schema_version: 1,
            name: "test".to_string(),
            notes: String::new(),
            created_at: "2026-05-25T00:00:00Z".to_string(),
            fields: IndexMap::new(),
            identity: IndexMap::new(),
            capture: Some(capture),
            extensions: Value::Object(Default::default()),
        }
    }

    #[test]
    fn rejects_capture_stored_as_escape() {
        let profile = profile_with_capture(json!({
            "sources": [{
                "type": "file",
                "path": "~/.codex/auth.json",
                "stored_as": "../auth.json"
            }]
        }));

        let err = CaptureSpec::from_profile(&profile).unwrap_err();
        assert!(err
            .to_string()
            .contains("invalid capture stored_as: ../auth.json"));
    }

    #[test]
    fn rejects_unknown_capture_platform() {
        let profile = profile_with_capture(json!({
            "sources": [{
                "type": "file",
                "path": "~/.codex/auth.json",
                "stored_as": "auth.json",
                "platforms": ["plan9"]
            }]
        }));

        let err = CaptureSpec::from_profile(&profile).unwrap_err();
        assert!(err.to_string().contains("unsupported platform plan9"));
    }

    #[test]
    fn rejects_json_subtree_without_json_path() {
        let profile = profile_with_capture(json!({
            "sources": [{
                "type": "json_subtree",
                "path": "~/.claude.json",
                "stored_as": "oauthAccount.json"
            }]
        }));

        let err = CaptureSpec::from_profile(&profile).unwrap_err();
        assert!(err.to_string().contains("missing json_path"));
    }

    #[test]
    fn toml_capture_without_paths_is_rejected() {
        let profile = profile_with_capture(json!({
            "sources": [{
                "type": "toml_managed_paths",
                "path": "~/.custom/config.toml",
                "stored_as": "config.toml",
                "required": false
            }]
        }));

        let err = CaptureSpec::from_profile(&profile).unwrap_err();

        assert!(err.to_string().contains("missing toml_paths"));
    }

    #[test]
    fn rejects_unknown_capture_source_type() {
        let profile = profile_with_capture(json!({
            "sources": [{
                "type": "shell_script",
                "stored_as": "blob"
            }]
        }));

        let err = CaptureSpec::from_profile(&profile).unwrap_err();
        assert!(err
            .to_string()
            .contains("UnknownHandler: unsupported capture source shell_script"));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_capture_blob_symlink_escape() {
        let home = tempfile::tempdir().unwrap();
        let switch_home = home.path().join(".any-switch");
        let paths = Paths {
            home: home.path().to_path_buf(),
            switch_home,
        };
        let capture_dir = capture_dir(&paths, "codex-test");
        fs::create_dir_all(&capture_dir).unwrap();
        let outside = home.path().join("outside-auth.json");
        fs::write(&outside, "{}").unwrap();
        symlink(&outside, capture_dir.join("auth.json")).unwrap();
        fs::write(
            capture_dir.join("manifest.json"),
            serde_json::to_vec_pretty(&CaptureManifest {
                schema_version: 1,
                profile_id: "codex-test".to_string(),
                captured_at: "2026-05-25T00:00:00Z".to_string(),
                last_writeback_at: None,
                sources: vec![CaptureManifestSource {
                    stored_as: "auth.json".to_string(),
                    sha256: sha256_hex(b"{}"),
                }],
            })
            .unwrap(),
        )
        .unwrap();
        let profile = profile_with_capture(json!({
            "sources": [{
                "type": "file",
                "path": "~/.codex/auth.json",
                "stored_as": "auth.json"
            }]
        }));

        let err = ensure_capture_complete(&paths, &profile).unwrap_err();
        assert!(err.to_string().contains("resolves outside capture dir"));
    }
}
