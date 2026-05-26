use crate::app_definitions::validate_id;
use crate::paths::{ensure_dir_private, write_private, Paths};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ActiveState {
    pub schema_version: u32,
    #[serde(default)]
    pub active_profiles: BTreeMap<String, Option<ActiveProfile>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveProfile {
    pub id: String,
    #[serde(default)]
    pub resolved_targets: Vec<ResolvedTarget>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedTarget {
    pub target_id: String,
    pub resolved_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingSwitch {
    pub schema_version: u32,
    pub operation: String,
    pub operation_id: String,
    pub app: String,
    #[serde(default)]
    pub from_profile: Option<String>,
    #[serde(default)]
    pub to_profile: Option<String>,
    #[serde(default)]
    pub backup_id: Option<String>,
    #[serde(default)]
    pub restore_from_backup_id: Option<String>,
    #[serde(default)]
    pub targets: Vec<ResolvedTarget>,
    pub stage: String,
    #[serde(default)]
    pub expected: Value,
}

impl ActiveState {
    pub fn load(paths: &Paths) -> Result<Self> {
        let path = paths.active_path();
        if !path.exists() {
            return Ok(Self {
                schema_version: 1,
                active_profiles: BTreeMap::new(),
            });
        }
        let text = fs::read_to_string(&path).with_context(|| path.display().to_string())?;
        let state: Self = serde_json::from_str(&text)?;
        state.validate_loaded()?;
        Ok(state)
    }

    pub fn save(&self, paths: &Paths) -> Result<()> {
        let text = serde_json::to_vec_pretty(self)?;
        write_private(&paths.active_path(), &text)
    }

    fn validate_loaded(&self) -> Result<()> {
        if self.schema_version != 1 {
            return Err(anyhow!(
                "StateInvalid: unsupported active.json schema_version {}",
                self.schema_version
            ));
        }
        for (app, active) in &self.active_profiles {
            validate_id(app)
                .with_context(|| format!("StateInvalid: invalid active app id {app}"))?;
            if let Some(active) = active {
                validate_id(&active.id).with_context(|| {
                    format!("StateInvalid: invalid active profile id {}", active.id)
                })?;
            }
        }
        Ok(())
    }
}

pub fn append_history(paths: &Paths, record: &serde_json::Value) -> Result<()> {
    let path = paths.history_path();
    crate::paths::ensure_parent(&path)?;
    if let Some(operation_id) = record.get("operation_id").and_then(Value::as_str) {
        if history_contains_operation_id(&path, operation_id)? {
            return Ok(());
        }
    }
    let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
    writeln!(file, "{}", serde_json::to_string(record)?)?;
    crate::paths::set_mode(&path, 0o600)?;
    Ok(())
}

fn history_contains_operation_id(path: &PathBuf, operation_id: &str) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    let text = fs::read_to_string(path).with_context(|| path.display().to_string())?;
    for line in text.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if value.get("operation_id").and_then(Value::as_str) == Some(operation_id) {
            return Ok(true);
        }
    }
    Ok(false)
}

pub fn pending_dir(paths: &Paths) -> PathBuf {
    paths.state_dir().join("pending-switch")
}

pub fn pending_path(paths: &Paths, app: &str) -> PathBuf {
    pending_dir(paths).join(format!("{app}.json"))
}

pub fn load_pending(paths: &Paths, app: &str) -> Result<Option<PendingSwitch>> {
    validate_pending_app(app)?;
    let path = pending_path(paths, app);
    if !path.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(&path).with_context(|| path.display().to_string())?;
    let pending: PendingSwitch = serde_json::from_str(&text)?;
    pending.validate_loaded(app)?;
    Ok(Some(pending))
}

pub fn write_pending(paths: &Paths, pending: &PendingSwitch) -> Result<()> {
    validate_pending_app(&pending.app)?;
    ensure_dir_private(&pending_dir(paths))?;
    write_private(
        &pending_path(paths, &pending.app),
        &serde_json::to_vec_pretty(pending)?,
    )
}

pub fn remove_pending(paths: &Paths, app: &str) -> Result<()> {
    validate_pending_app(app)?;
    let path = pending_path(paths, app);
    if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

fn validate_pending_app(app: &str) -> Result<()> {
    validate_id(app).map_err(|err| anyhow!("invalid app id for pending state {app}: {err}"))
}

impl PendingSwitch {
    fn validate_loaded(&self, expected_app: &str) -> Result<()> {
        if self.schema_version != 1 {
            return Err(anyhow!(
                "StateInvalid: unsupported pending switch schema_version {}",
                self.schema_version
            ));
        }
        validate_id(&self.app)
            .with_context(|| format!("StateInvalid: invalid pending app id {}", self.app))?;
        if self.app != expected_app {
            return Err(anyhow!(
                "StateInvalid: pending switch app {} does not match file app {}",
                self.app,
                expected_app
            ));
        }
        validate_state_token("operation_id", &self.operation_id)?;
        if !matches!(
            self.stage.as_str(),
            "applying" | "verifying" | "bookkeeping"
        ) {
            return Err(anyhow!(
                "StateInvalid: invalid pending stage {}",
                self.stage
            ));
        }
        if let Some(from_profile) = &self.from_profile {
            validate_id(from_profile).with_context(|| {
                format!("StateInvalid: invalid pending from_profile id {from_profile}")
            })?;
        }
        if let Some(to_profile) = &self.to_profile {
            validate_id(to_profile).with_context(|| {
                format!("StateInvalid: invalid pending to_profile id {to_profile}")
            })?;
        }
        if let Some(backup_id) = &self.backup_id {
            validate_state_token("backup_id", backup_id)?;
        }
        if let Some(backup_id) = &self.restore_from_backup_id {
            validate_state_token("restore_from_backup_id", backup_id)?;
        }
        for target in &self.targets {
            validate_nonempty_state_string("target_id", &target.target_id)?;
            validate_nonempty_state_string("resolved_path", &target.resolved_path)?;
        }

        match self.operation.as_str() {
            "use" => {
                if self.to_profile.is_none() {
                    return Err(anyhow!("StateInvalid: pending use missing to_profile"));
                }
                if self.backup_id.is_none() {
                    return Err(anyhow!("StateInvalid: pending use missing backup_id"));
                }
                if self.restore_from_backup_id.is_some() {
                    return Err(anyhow!(
                        "StateInvalid: pending use must not include restore_from_backup_id"
                    ));
                }
            }
            "restore-target" => {
                if self.backup_id.is_none() {
                    return Err(anyhow!(
                        "StateInvalid: pending restore-target missing backup_id"
                    ));
                }
                if self.restore_from_backup_id.is_none() {
                    return Err(anyhow!(
                        "StateInvalid: pending restore-target missing restore_from_backup_id"
                    ));
                }
                if self.to_profile.is_some() || self.from_profile.is_some() {
                    return Err(anyhow!(
                        "StateInvalid: pending restore-target must not include profile ids"
                    ));
                }
            }
            other => return Err(anyhow!("StateInvalid: invalid pending operation {other}")),
        }
        Ok(())
    }
}

fn validate_state_token(name: &str, value: &str) -> Result<()> {
    validate_nonempty_state_string(name, value)?;
    if value.contains('/') || value.contains('\\') || value == "." || value == ".." {
        return Err(anyhow!("StateInvalid: invalid pending {name} {value}"));
    }
    if !value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '.' || ch == '_' || ch == '-')
    {
        return Err(anyhow!("StateInvalid: invalid pending {name} {value}"));
    }
    Ok(())
}

fn validate_nonempty_state_string(name: &str, value: &str) -> Result<()> {
    if value.is_empty() || value.len() > 4096 {
        return Err(anyhow!("StateInvalid: invalid pending {name}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn append_history_is_idempotent_by_operation_id() {
        let dir = tempdir().unwrap();
        let paths = Paths {
            home: dir.path().to_path_buf(),
            switch_home: dir.path().join(".any-switch"),
        };
        let record = serde_json::json!({
            "operation_id": "op-1",
            "operation": "use",
            "ok": true
        });
        append_history(&paths, &record).unwrap();
        append_history(&paths, &record).unwrap();
        let text = fs::read_to_string(paths.history_path()).unwrap();
        assert_eq!(text.lines().count(), 1);
    }

    #[test]
    fn active_state_rejects_invalid_ids() {
        let dir = tempdir().unwrap();
        let paths = Paths {
            home: dir.path().to_path_buf(),
            switch_home: dir.path().join(".any-switch"),
        };
        fs::create_dir_all(paths.state_dir()).unwrap();
        fs::write(
            paths.active_path(),
            r#"{"schema_version":1,"active_profiles":{"../escape":{"id":"profile"}}}"#,
        )
        .unwrap();

        let err = ActiveState::load(&paths).unwrap_err();
        assert!(err.to_string().contains("invalid active app id"));

        fs::write(
            paths.active_path(),
            r#"{"schema_version":1,"active_profiles":{"codex":{"id":"../escape"}}}"#,
        )
        .unwrap();

        let err = ActiveState::load(&paths).unwrap_err();
        assert!(err.to_string().contains("invalid active profile id"));
    }

    #[test]
    fn pending_state_rejects_invalid_or_mismatched_contents() {
        let dir = tempdir().unwrap();
        let paths = Paths {
            home: dir.path().to_path_buf(),
            switch_home: dir.path().join(".any-switch"),
        };
        fs::create_dir_all(pending_dir(&paths)).unwrap();
        fs::write(
            pending_path(&paths, "codex"),
            r#"{"schema_version":1,"operation":"use","operation_id":"op-1","app":"../escape","to_profile":"codex-safe","backup_id":"backup-1","targets":[],"stage":"applying"}"#,
        )
        .unwrap();

        let err = load_pending(&paths, "codex").unwrap_err();
        assert!(err.to_string().contains("invalid pending app id"));

        fs::write(
            pending_path(&paths, "codex"),
            r#"{"schema_version":1,"operation":"use","operation_id":"op-1","app":"other","to_profile":"codex-safe","backup_id":"backup-1","targets":[],"stage":"applying"}"#,
        )
        .unwrap();

        let err = load_pending(&paths, "codex").unwrap_err();
        assert!(err.to_string().contains("does not match file app"));

        fs::write(
            pending_path(&paths, "codex"),
            r#"{"schema_version":1,"operation":"use","operation_id":"op-1","app":"codex","to_profile":"../escape","backup_id":"backup-1","targets":[],"stage":"applying"}"#,
        )
        .unwrap();

        let err = load_pending(&paths, "codex").unwrap_err();
        assert!(err.to_string().contains("invalid pending to_profile id"));

        fs::write(
            pending_path(&paths, "codex"),
            r#"{"schema_version":1,"operation":"restore-target","operation_id":"op-1","app":"codex","backup_id":"rollback-1","restore_from_backup_id":"../backup","targets":[],"stage":"applying"}"#,
        )
        .unwrap();

        let err = load_pending(&paths, "codex").unwrap_err();
        assert!(err
            .to_string()
            .contains("invalid pending restore_from_backup_id"));
    }
}
