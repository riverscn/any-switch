use crate::app_definitions::validate_id;
use crate::backup::sha256_hex;
use crate::paths::{ensure_dir_private, set_mode, Paths};
use anyhow::{anyhow, Context, Result};
use std::env;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug)]
pub struct FileLock {
    path: PathBuf,
    file: File,
}

impl FileLock {
    pub fn acquire(path: PathBuf) -> Result<Self> {
        if let Some(parent) = path.parent() {
            ensure_dir_private(parent)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .with_context(|| path.display().to_string())?;
        set_mode(&path, 0o600)?;
        lock_file(&file, &path)?;
        Ok(Self { path, file })
    }

    pub fn acquire_wait(path: PathBuf, timeout: Duration) -> Result<Self> {
        let deadline = Instant::now() + timeout;
        loop {
            match Self::acquire(path.clone()) {
                Ok(lock) => return Ok(lock),
                Err(err) if err.to_string().starts_with("LockBusy:") => {
                    if Instant::now() >= deadline {
                        return Err(err);
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                Err(err) => return Err(err),
            }
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = unlock_file(&self.file);
    }
}

pub fn profiles_lock(paths: &Paths) -> PathBuf {
    paths.switch_home.join("locks").join("profiles.lock")
}

pub fn state_lock(paths: &Paths) -> PathBuf {
    paths.switch_home.join("locks").join("state.lock")
}

pub fn acquire_state_lock(paths: &Paths) -> Result<FileLock> {
    FileLock::acquire_wait(state_lock(paths), lock_wait_timeout())
}

pub fn app_lock(paths: &Paths, app: &str) -> Result<PathBuf> {
    validate_id(app).map_err(|err| anyhow!("invalid app id for lock {app}: {err}"))?;
    Ok(paths.switch_home.join("locks").join(format!("{app}.lock")))
}

pub fn target_lock(paths: &Paths, target_id: &str) -> PathBuf {
    let digest = sha256_hex(target_id.as_bytes());
    paths
        .switch_home
        .join("locks")
        .join(format!("target-{digest}.lock"))
}

pub fn acquire_target_locks(paths: &Paths, mut target_ids: Vec<String>) -> Result<Vec<FileLock>> {
    target_ids.sort();
    target_ids.dedup();
    let mut locks = Vec::new();
    let timeout = lock_wait_timeout();
    for target_id in target_ids {
        locks.push(FileLock::acquire_wait(
            target_lock(paths, &target_id),
            timeout,
        )?);
    }
    Ok(locks)
}

fn lock_wait_timeout() -> Duration {
    env::var("SWITCH_CLI_LOCK_WAIT_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or_else(|| Duration::from_secs(5))
}

#[cfg(unix)]
fn lock_file(file: &File, path: &Path) -> Result<()> {
    use std::os::fd::AsRawFd;
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if rc == 0 {
        return Ok(());
    }
    let err = std::io::Error::last_os_error();
    if err.kind() == std::io::ErrorKind::WouldBlock {
        Err(anyhow!("LockBusy: {}", path.display()))
    } else {
        Err(anyhow!("failed to lock {}: {err}", path.display()))
    }
}

#[cfg(unix)]
fn unlock_file(file: &File) -> Result<()> {
    use std::os::fd::AsRawFd;
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) };
    if rc == 0 {
        Ok(())
    } else {
        Err(anyhow!(
            "failed to unlock file: {}",
            std::io::Error::last_os_error()
        ))
    }
}

#[cfg(not(unix))]
fn lock_file(_file: &File, _path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(not(unix))]
fn unlock_file(_file: &File) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn second_lock_is_busy() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.lock");
        let first = FileLock::acquire(path.clone()).unwrap();
        let err = FileLock::acquire(path).unwrap_err().to_string();
        drop(first);
        assert!(err.contains("LockBusy"));
    }
}
