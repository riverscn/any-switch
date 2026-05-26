use anyhow::{anyhow, Context, Result};
use std::env;
#[cfg(unix)]
use std::ffi::CStr;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Paths {
    pub home: PathBuf,
    pub switch_home: PathBuf,
}

impl Paths {
    pub fn discover() -> Result<Self> {
        let home = current_os_home()?;
        let switch_home = match env::var_os("ANY_SWITCH_HOME") {
            Some(raw) => {
                let path = PathBuf::from(raw);
                if !path.is_absolute() {
                    return Err(anyhow!("ANY_SWITCH_HOME must be absolute"));
                }
                path
            }
            None => home.join(".any-switch"),
        };
        ensure_inside_home(&switch_home, &home).map_err(|err| anyhow!("ANY_SWITCH_HOME {err}"))?;
        ensure_existing_ancestor_inside_home(&switch_home, &home)
            .map_err(|err| anyhow!("ANY_SWITCH_HOME {err}"))?;
        Ok(Self { home, switch_home })
    }

    pub fn profiles_path(&self) -> PathBuf {
        self.switch_home.join("profiles.yaml")
    }

    pub fn state_dir(&self) -> PathBuf {
        self.switch_home.join("state")
    }

    pub fn active_path(&self) -> PathBuf {
        self.state_dir().join("active.json")
    }

    pub fn history_path(&self) -> PathBuf {
        self.state_dir().join("history.jsonl")
    }

    pub fn backups_dir(&self) -> PathBuf {
        self.switch_home.join("backups")
    }

    pub fn ensure_layout(&self) -> Result<()> {
        ensure_dir_private(&self.switch_home)?;
        for dir in [
            self.switch_home.join("apps.d"),
            self.switch_home.join("overrides.d"),
            self.switch_home.join("captures"),
            self.backups_dir(),
            self.state_dir(),
            self.switch_home.join("locks"),
        ] {
            ensure_dir_private(&dir)?;
        }
        Ok(())
    }

    pub fn expand_target_path(&self, template: &str) -> Result<PathBuf> {
        let mut text = template.to_string();
        text = expand_home_prefix(&text, &self.home);
        text = text.replace("${MACOS_USER}", &current_os_user());
        text = expand_defaulted_envs(text, &self.home)?;
        if text.contains("${") {
            return Err(anyhow!("unsupported path template expansion: {template}"));
        }
        let path = PathBuf::from(text);
        let absolute = if path.is_absolute() {
            path
        } else {
            return Err(anyhow!(
                "target path must be absolute after expansion: {template}"
            ));
        };
        ensure_inside_home(&absolute, &self.home)?;
        ensure_existing_ancestor_inside_home(&absolute, &self.home)?;
        self.ensure_outside_switch_home(&absolute)?;
        Ok(absolute)
    }

    pub fn ensure_outside_switch_home(&self, path: &Path) -> Result<()> {
        if is_inside(path, &self.switch_home) {
            return Err(anyhow!(
                "path must not be inside ANY_SWITCH_HOME: {}",
                path.display()
            ));
        }
        if self.switch_home.exists() {
            let real_switch_home = self
                .switch_home
                .canonicalize()
                .with_context(|| format!("canonicalize {}", self.switch_home.display()))?;
            let real_path = canonical_existing_ancestor(path)?;
            if is_inside(&real_path, &real_switch_home) {
                return Err(anyhow!(
                    "path resolves inside ANY_SWITCH_HOME: {} -> {}",
                    path.display(),
                    real_path.display()
                ));
            }
        }
        Ok(())
    }
}

fn expand_defaulted_envs(mut text: String, home: &Path) -> Result<String> {
    while let Some(start) = text.find("${") {
        let Some(relative_end) = text[start..].find('}') else {
            return Err(anyhow!("unsupported path template expansion: {text}"));
        };
        let end = start + relative_end;
        let expression = &text[start + 2..end];
        let Some((name, default)) = expression.split_once(":-") else {
            return Err(anyhow!(
                "unsupported path template expansion: ${{{expression}}}"
            ));
        };
        validate_env_name(name)?;
        let value = env::var(name).unwrap_or_else(|_| default.to_string());
        let value = expand_home_prefix(&value, home);
        let path = PathBuf::from(&value);
        if !path.is_absolute() {
            return Err(anyhow!("{name} must expand to an absolute path"));
        }
        ensure_inside_home(&path, home)?;
        ensure_existing_ancestor_inside_home(&path, home)?;
        text.replace_range(start..=end, &value);
    }
    Ok(text)
}

fn validate_env_name(name: &str) -> Result<()> {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return Err(anyhow!("empty environment variable name"));
    };
    if !(first == '_' || first.is_ascii_uppercase()) {
        return Err(anyhow!("unsupported environment variable name: {name}"));
    }
    if !chars.all(|ch| ch == '_' || ch.is_ascii_uppercase() || ch.is_ascii_digit()) {
        return Err(anyhow!("unsupported environment variable name: {name}"));
    }
    Ok(())
}

fn expand_home_prefix(text: &str, home: &Path) -> String {
    if text == "~" {
        home.display().to_string()
    } else if let Some(rest) = text.strip_prefix("~/") {
        home.join(rest).display().to_string()
    } else {
        text.to_string()
    }
}

pub fn ensure_inside_home(path: &Path, home: &Path) -> Result<()> {
    if !is_inside(path, home) {
        return Err(anyhow!("path is outside home: {}", path.display()));
    }
    Ok(())
}

pub fn ensure_existing_ancestor_inside_home(path: &Path, home: &Path) -> Result<()> {
    let real = canonical_existing_ancestor(path)?;
    let real_home = home
        .canonicalize()
        .with_context(|| format!("canonicalize home {}", home.display()))?;
    if !is_inside(&real, &real_home) {
        return Err(anyhow!(
            "path resolves outside home: {} -> {}",
            path.display(),
            real.display()
        ));
    }
    Ok(())
}

fn canonical_existing_ancestor(path: &Path) -> Result<PathBuf> {
    let mut cursor = path;
    while !cursor.exists() {
        let Some(parent) = cursor.parent() else {
            return Ok(path.to_path_buf());
        };
        cursor = parent;
    }
    cursor
        .canonicalize()
        .with_context(|| format!("canonicalize {}", cursor.display()))
}

pub fn is_inside(path: &Path, parent: &Path) -> bool {
    path == parent || path.starts_with(parent)
}

pub fn ensure_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        ensure_dir_private(parent)?;
    }
    Ok(())
}

pub fn ensure_dir_private(path: &Path) -> Result<()> {
    fs::create_dir_all(path).with_context(|| path.display().to_string())?;
    set_mode(path, 0o700)?;
    Ok(())
}

pub fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    ensure_parent(path)?;
    let mode = private_file_mode_for_write(path)?;
    let tmp = path.with_extension(format!(
        "{}.tmp",
        path.extension().and_then(|v| v.to_str()).unwrap_or("file")
    ));
    {
        let mut file = fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&tmp)
            .with_context(|| tmp.display().to_string())?;
        file.write_all(bytes)
            .with_context(|| tmp.display().to_string())?;
        set_mode(&tmp, mode)?;
        file.sync_all()
            .with_context(|| format!("fsync {}", tmp.display()))?;
    }
    fs::rename(&tmp, path).with_context(|| format!("rename {}", path.display()))?;
    set_mode(path, mode)?;
    fsync_parent(path)?;
    Ok(())
}

pub fn write_private_following_symlink(path: &Path, bytes: &[u8]) -> Result<()> {
    let target = final_write_path(path)?;
    write_private(&target, bytes)
}

fn final_write_path(path: &Path) -> Result<PathBuf> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => path
            .canonicalize()
            .with_context(|| format!("canonicalize symlink target {}", path.display())),
        Ok(_) => Ok(path.to_path_buf()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(path.to_path_buf()),
        Err(err) => Err(err).with_context(|| path.display().to_string()),
    }
}

#[cfg(unix)]
fn private_file_mode_for_write(path: &Path) -> Result<u32> {
    use std::os::unix::fs::PermissionsExt;
    if !path.exists() {
        return Ok(0o600);
    }
    let mode = fs::metadata(path)?.permissions().mode() & 0o777;
    if mode & 0o077 == 0 && mode & !0o600 == 0 {
        Ok(mode)
    } else {
        Ok(0o600)
    }
}

#[cfg(not(unix))]
fn private_file_mode_for_write(_path: &Path) -> Result<u32> {
    Ok(0o600)
}

#[cfg(unix)]
fn fsync_parent(path: &Path) -> Result<()> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    let dir = fs::File::open(parent).with_context(|| format!("open dir {}", parent.display()))?;
    dir.sync_all()
        .with_context(|| format!("fsync dir {}", parent.display()))
}

#[cfg(not(unix))]
fn fsync_parent(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
pub fn set_mode(path: &Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(mode);
    fs::set_permissions(path, perms)?;
    Ok(())
}

#[cfg(not(unix))]
pub fn set_mode(_path: &Path, _mode: u32) -> Result<()> {
    Ok(())
}

pub(crate) fn current_os_home() -> Result<PathBuf> {
    #[cfg(unix)]
    {
        #[cfg(debug_assertions)]
        if let Some(raw) = env::var_os("ANY_SWITCH_TEST_HOME") {
            let path = PathBuf::from(raw);
            if path.is_absolute() {
                return Ok(path);
            }
            return Err(anyhow!("ANY_SWITCH_TEST_HOME must be absolute"));
        }

        let uid = unsafe { libc::getuid() };
        let passwd = unsafe { libc::getpwuid(uid) };
        if passwd.is_null() {
            return Err(anyhow!("cannot determine home directory from getpwuid"));
        }
        let dir = unsafe { CStr::from_ptr((*passwd).pw_dir) };
        let text = dir
            .to_str()
            .map_err(|_| anyhow!("home directory from getpwuid is not UTF-8"))?;
        if text.is_empty() {
            return Err(anyhow!("home directory from getpwuid is empty"));
        }
        Ok(PathBuf::from(text))
    }
    #[cfg(not(unix))]
    {
        home::home_dir().ok_or_else(|| anyhow!("cannot determine home directory"))
    }
}

pub(crate) fn current_os_user() -> String {
    #[cfg(unix)]
    {
        let uid = unsafe { libc::getuid() };
        let passwd = unsafe { libc::getpwuid(uid) };
        if !passwd.is_null() {
            let name = unsafe { CStr::from_ptr((*passwd).pw_name) };
            if let Ok(text) = name.to_str() {
                if !text.is_empty() {
                    return text.to_string();
                }
            }
        }
        "unknown".to_string()
    }
    #[cfg(not(unix))]
    env::var("USER").unwrap_or_else(|_| "unknown".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    #[cfg(unix)]
    use std::sync::{Mutex, OnceLock};

    #[cfg(unix)]
    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    #[test]
    fn expands_home_prefix() {
        assert_eq!(
            expand_home_prefix("~/x", Path::new("/home/me")),
            "/home/me/x"
        );
    }

    #[cfg(unix)]
    #[test]
    fn current_os_user_ignores_user_env() {
        let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let previous = env::var_os("USER");
        env::set_var("USER", "spoofed-any-switch-user");

        let user = current_os_user();

        if let Some(previous) = previous {
            env::set_var("USER", previous);
        } else {
            env::remove_var("USER");
        }
        assert_ne!(user, "spoofed-any-switch-user");
    }

    #[cfg(unix)]
    #[test]
    fn current_os_home_ignores_home_env() {
        let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let previous_home = env::var_os("HOME");
        let previous_test_home = env::var_os("ANY_SWITCH_TEST_HOME");
        env::set_var("HOME", "/tmp/spoofed-any-switch-home");
        env::remove_var("ANY_SWITCH_TEST_HOME");

        let home = current_os_home().unwrap();

        if let Some(previous_home) = previous_home {
            env::set_var("HOME", previous_home);
        } else {
            env::remove_var("HOME");
        }
        if let Some(previous_test_home) = previous_test_home {
            env::set_var("ANY_SWITCH_TEST_HOME", previous_test_home);
        } else {
            env::remove_var("ANY_SWITCH_TEST_HOME");
        }
        assert_ne!(home, PathBuf::from("/tmp/spoofed-any-switch-home"));
    }

    #[cfg(unix)]
    #[test]
    fn switch_home_must_be_inside_current_home() {
        let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let home = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let previous_home = env::var_os("ANY_SWITCH_TEST_HOME");
        let previous_switch_home = env::var_os("ANY_SWITCH_HOME");
        env::set_var("ANY_SWITCH_TEST_HOME", home.path());
        env::set_var("ANY_SWITCH_HOME", outside.path());

        let err = Paths::discover().unwrap_err().to_string();

        if let Some(previous_home) = previous_home {
            env::set_var("ANY_SWITCH_TEST_HOME", previous_home);
        } else {
            env::remove_var("ANY_SWITCH_TEST_HOME");
        }
        if let Some(previous_switch_home) = previous_switch_home {
            env::set_var("ANY_SWITCH_HOME", previous_switch_home);
        } else {
            env::remove_var("ANY_SWITCH_HOME");
        }
        assert!(err.contains("ANY_SWITCH_HOME path is outside home"));
    }

    #[cfg(unix)]
    #[test]
    fn default_switch_home_symlink_must_stay_inside_home() {
        let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let home = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let previous_home = env::var_os("ANY_SWITCH_TEST_HOME");
        let previous_switch_home = env::var_os("ANY_SWITCH_HOME");
        env::set_var("ANY_SWITCH_TEST_HOME", home.path());
        env::remove_var("ANY_SWITCH_HOME");
        std::os::unix::fs::symlink(outside.path(), home.path().join(".any-switch")).unwrap();

        let err = Paths::discover().unwrap_err().to_string();

        if let Some(previous_home) = previous_home {
            env::set_var("ANY_SWITCH_TEST_HOME", previous_home);
        } else {
            env::remove_var("ANY_SWITCH_TEST_HOME");
        }
        if let Some(previous_switch_home) = previous_switch_home {
            env::set_var("ANY_SWITCH_HOME", previous_switch_home);
        } else {
            env::remove_var("ANY_SWITCH_HOME");
        }
        assert!(err.contains("ANY_SWITCH_HOME path resolves outside home"));
    }

    #[cfg(unix)]
    #[test]
    fn target_must_not_resolve_inside_switch_home() {
        let home = tempfile::tempdir().unwrap();
        let real_switch_home = home.path().join("real-switch");
        let switch_home_link = home.path().join(".any-switch");
        fs::create_dir_all(&real_switch_home).unwrap();
        std::os::unix::fs::symlink(&real_switch_home, &switch_home_link).unwrap();
        let paths = Paths {
            home: home.path().to_path_buf(),
            switch_home: switch_home_link,
        };

        let err = paths
            .expand_target_path(&real_switch_home.join("profiles.yaml").display().to_string())
            .unwrap_err()
            .to_string();

        assert!(err.contains("resolves inside ANY_SWITCH_HOME"));
    }

    #[test]
    fn expands_defaulted_env_template_for_any_app_definition() {
        let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let home = tempfile::tempdir().unwrap();
        let previous = env::var_os("TOOLBOX_CONFIG_DIR");
        env::remove_var("TOOLBOX_CONFIG_DIR");
        let paths = Paths {
            home: home.path().to_path_buf(),
            switch_home: home.path().join(".any-switch"),
        };

        let expanded = paths
            .expand_target_path("${TOOLBOX_CONFIG_DIR:-~/.toolbox}/credentials.json")
            .unwrap();

        assert_eq!(
            expanded,
            home.path().join(".toolbox").join("credentials.json")
        );
        if let Some(previous) = previous {
            env::set_var("TOOLBOX_CONFIG_DIR", previous);
        }
    }

    #[cfg(unix)]
    #[test]
    fn write_private_tightens_wide_file_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secret.json");
        fs::write(&path, b"old").unwrap();
        set_mode(&path, 0o644).unwrap();
        write_private(&path, b"new").unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        assert_eq!(fs::read(&path).unwrap(), b"new");
    }

    #[cfg(unix)]
    #[test]
    fn write_private_preserves_stricter_owner_only_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secret.json");
        fs::write(&path, b"old").unwrap();
        set_mode(&path, 0o400).unwrap();
        write_private(&path, b"new").unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o400);
        assert_eq!(fs::read(&path).unwrap(), b"new");
    }

    #[cfg(unix)]
    #[test]
    fn write_private_following_symlink_preserves_final_link() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("real.json");
        let link = dir.path().join("link.json");
        fs::write(&target, b"old").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        write_private_following_symlink(&link, b"new").unwrap();

        assert!(fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(fs::read(&target).unwrap(), b"new");
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_parent_outside_home() {
        let home = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let link = home.path().join("tool-link");
        std::os::unix::fs::symlink(outside.path(), &link).unwrap();
        let paths = Paths {
            home: home.path().to_path_buf(),
            switch_home: home.path().join(".any-switch"),
        };
        let err = paths
            .expand_target_path(&link.join("auth.json").display().to_string())
            .unwrap_err()
            .to_string();
        assert!(err.contains("resolves outside home"));
    }
}
