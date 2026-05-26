use crate::app_definitions::AppDefinition;
use anyhow::{anyhow, Result};
use serde::Serialize;
use std::process::Command;

#[derive(Debug, Clone, Serialize)]
pub struct RunningProcess {
    pub pid: u32,
    pub start_time: Option<String>,
    pub command: String,
}

pub fn detect_running(definition: &AppDefinition) -> Result<Vec<RunningProcess>> {
    if std::env::var_os("SWITCH_CLI_SKIP_PROCESS_PROBE").as_deref()
        == Some(std::ffi::OsStr::new("1"))
    {
        return Ok(Vec::new());
    }
    if let Ok(fixture) = std::env::var("SWITCH_CLI_PROCESS_PROBE_FIXTURE") {
        return Ok(parse_fixture(definition, &fixture));
    }
    if let Ok(error) = std::env::var("SWITCH_CLI_PROCESS_PROBE_ERROR_FIXTURE") {
        return Err(anyhow!("process probe fixture error: {error}"));
    }
    if definition.process_probe.names.is_empty() {
        return Ok(Vec::new());
    }
    let output = Command::new("ps")
        .args(["-axo", "pid=,lstart=,comm=,args="])
        .output()
        .map_err(|err| anyhow!("process probe failed to run ps: {err}"))?;
    if !output.status.success() {
        return Err(anyhow!("process probe ps exited with {}", output.status));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let current_pid = std::process::id();
    let mut matches = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let Some(pid_text) = parts.next() else {
            continue;
        };
        let Ok(pid) = pid_text.parse::<u32>() else {
            continue;
        };
        if pid == current_pid {
            continue;
        }
        let start_time = (0..5)
            .filter_map(|_| parts.next())
            .collect::<Vec<_>>()
            .join(" ");
        let comm = parts.next().unwrap_or_default();
        let args = parts.collect::<Vec<_>>().join(" ");
        if definition
            .process_probe
            .names
            .iter()
            .any(|name| process_name_matches(name, comm, &args))
        {
            matches.push(RunningProcess {
                pid,
                start_time: if start_time.is_empty() {
                    None
                } else {
                    Some(start_time)
                },
                command: if args.is_empty() {
                    comm.to_string()
                } else {
                    args
                },
            });
        }
    }
    Ok(matches)
}

fn parse_fixture(definition: &AppDefinition, fixture: &str) -> Vec<RunningProcess> {
    fixture
        .lines()
        .filter_map(|line| {
            let parts = line.split('\t').collect::<Vec<_>>();
            let (pid, start_time, command) = match parts.as_slice() {
                [pid, command] => (*pid, None, *command),
                [pid, start_time, command] => (*pid, Some((*start_time).to_string()), *command),
                _ => return None,
            };
            let pid = pid.parse::<u32>().ok()?;
            if definition
                .process_probe
                .names
                .iter()
                .any(|name| process_name_matches(name, command, command))
            {
                Some(RunningProcess {
                    pid,
                    start_time,
                    command: command.to_string(),
                })
            } else {
                None
            }
        })
        .collect()
}

fn process_name_matches(name: &str, comm: &str, args: &str) -> bool {
    let comm_base = comm.rsplit('/').next().unwrap_or(comm);
    if comm_base == name || comm == name {
        return true;
    }
    let first_arg = args.split_whitespace().next().unwrap_or_default();
    let first_base = first_arg.rsplit('/').next().unwrap_or(first_arg);
    first_base == name
}

pub fn format_app_running(app: &str, processes: &[RunningProcess]) -> String {
    let mut message = format!("AppRunning: {app} appears to be running");
    for process in processes {
        let start_time = process.start_time.as_deref().unwrap_or("unknown");
        message.push_str(&format!(
            "\n  pid={} start_time={} command={}",
            process.pid, start_time, process.command
        ));
    }
    message
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_basename() {
        assert!(process_name_matches("codex", "/opt/bin/codex", ""));
        assert!(process_name_matches(
            "codex",
            "zsh",
            "/usr/local/bin/codex run"
        ));
        assert!(!process_name_matches("codex", "codex-helper", ""));
    }
}
