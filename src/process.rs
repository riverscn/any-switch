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
    if std::env::var_os("ANY_SWITCH_SKIP_PROCESS_PROBE").as_deref()
        == Some(std::ffi::OsStr::new("1"))
    {
        return Ok(Vec::new());
    }
    if let Ok(fixture) = std::env::var("ANY_SWITCH_PROCESS_PROBE_FIXTURE") {
        return Ok(parse_fixture(definition, &fixture));
    }
    if let Ok(error) = std::env::var("ANY_SWITCH_PROCESS_PROBE_ERROR_FIXTURE") {
        return Err(anyhow!("process probe fixture error: {error}"));
    }
    if definition.process_probe.names.is_empty() {
        return Ok(Vec::new());
    }
    detect_platform_running(definition)
}

#[cfg(unix)]
fn detect_platform_running(definition: &AppDefinition) -> Result<Vec<RunningProcess>> {
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

#[cfg(windows)]
fn detect_platform_running(definition: &AppDefinition) -> Result<Vec<RunningProcess>> {
    match detect_windows_powershell(definition) {
        Ok(processes) => Ok(processes),
        Err(powershell_err) => detect_windows_tasklist(definition).map_err(|tasklist_err| {
            anyhow!("process probe failed: powershell: {powershell_err}; tasklist: {tasklist_err}")
        }),
    }
}

#[cfg(windows)]
fn detect_windows_powershell(definition: &AppDefinition) -> Result<Vec<RunningProcess>> {
    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "Get-CimInstance Win32_Process | Select-Object ProcessId,CreationDate,Name,CommandLine | ConvertTo-Csv -NoTypeInformation",
        ])
        .output()
        .map_err(|err| anyhow!("process probe failed to run powershell: {err}"))?;
    if !output.status.success() {
        return Err(anyhow!("powershell exited with {}", output.status));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let current_pid = std::process::id();
    let mut matches = Vec::new();
    for (index, line) in stdout.lines().enumerate() {
        if index == 0 || line.trim().is_empty() {
            continue;
        }
        let fields = parse_csv_line(line);
        if fields.len() < 4 {
            continue;
        }
        let Ok(pid) = fields[0].parse::<u32>() else {
            continue;
        };
        if pid == current_pid {
            continue;
        }
        let name = fields[2].as_str();
        let command = if fields[3].trim().is_empty() {
            name.to_string()
        } else {
            fields[3].clone()
        };
        if definition
            .process_probe
            .names
            .iter()
            .any(|probe_name| process_name_matches(probe_name, name, &command))
        {
            matches.push(RunningProcess {
                pid,
                start_time: (!fields[1].is_empty()).then(|| fields[1].clone()),
                command,
            });
        }
    }
    Ok(matches)
}

#[cfg(windows)]
fn detect_windows_tasklist(definition: &AppDefinition) -> Result<Vec<RunningProcess>> {
    let output = Command::new("tasklist")
        .args(["/FO", "CSV", "/NH"])
        .output()
        .map_err(|err| anyhow!("process probe failed to run tasklist: {err}"))?;
    if !output.status.success() {
        return Err(anyhow!("tasklist exited with {}", output.status));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let current_pid = std::process::id();
    let mut matches = Vec::new();
    for line in stdout.lines() {
        let fields = parse_csv_line(line);
        if fields.len() < 2 {
            continue;
        }
        let Ok(pid) = fields[1].parse::<u32>() else {
            continue;
        };
        if pid == current_pid {
            continue;
        }
        let name = fields[0].as_str();
        if definition
            .process_probe
            .names
            .iter()
            .any(|probe_name| process_name_matches(probe_name, name, name))
        {
            matches.push(RunningProcess {
                pid,
                start_time: None,
                command: name.to_string(),
            });
        }
    }
    Ok(matches)
}

#[cfg(not(any(unix, windows)))]
fn detect_platform_running(_definition: &AppDefinition) -> Result<Vec<RunningProcess>> {
    Err(anyhow!("process probe is not implemented on this platform"))
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
    let comm_base = path_basename(comm);
    if process_name_equals(comm_base, name) || process_name_equals(comm, name) {
        return true;
    }
    let first_arg = args.split_whitespace().next().unwrap_or_default();
    let first_base = path_basename(first_arg.trim_matches('"'));
    process_name_equals(first_base, name)
}

fn path_basename(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

#[cfg(windows)]
fn process_name_equals(actual: &str, expected: &str) -> bool {
    actual
        .strip_suffix(".exe")
        .unwrap_or(actual)
        .eq_ignore_ascii_case(expected.strip_suffix(".exe").unwrap_or(expected))
}

#[cfg(not(windows))]
fn process_name_equals(actual: &str, expected: &str) -> bool {
    actual == expected
}

#[cfg(any(test, windows))]
fn parse_csv_line(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut chars = line.chars().peekable();
    let mut quoted = false;
    while let Some(ch) = chars.next() {
        match ch {
            '"' if quoted && chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => {
                fields.push(std::mem::take(&mut field));
            }
            _ => field.push(ch),
        }
    }
    fields.push(field);
    fields
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

    #[cfg(windows)]
    #[test]
    fn matches_windows_exe_names_case_insensitively() {
        assert!(process_name_matches("codex", r"C:\Tools\Codex.exe", ""));
        assert!(process_name_matches(
            "codex",
            "powershell.exe",
            r#""C:\Tools\Codex.exe" run"#
        ));
        assert!(!process_name_matches("codex", "codex-helper.exe", ""));
    }

    #[test]
    fn parses_quoted_csv_fields() {
        assert_eq!(
            parse_csv_line(
                r#""123","20260526010203.000000+480","codex.exe","C:\Tools\codex.exe run, now""#
            ),
            vec![
                "123",
                "20260526010203.000000+480",
                "codex.exe",
                r"C:\Tools\codex.exe run, now"
            ]
        );
    }
}
