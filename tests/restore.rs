use assert_cmd::Command;
use predicates::str::contains;
use std::fs;
mod support;
use support::*;

#[test]
fn restore_oauth_backup_ignores_allow_running_and_requires_assume_app_stopped() {
    let cwd = std::env::current_dir().unwrap();
    let switch_home = tempfile::Builder::new()
        .prefix(".test-switch-")
        .tempdir_in(&cwd)
        .unwrap();
    let codex_home = tempfile::Builder::new()
        .prefix(".test-codex-")
        .tempdir_in(&cwd)
        .unwrap();
    write_codex_oauth(codex_home.path(), "acct-a", "refresh-a");

    Command::cargo_bin("switch-cli")
        .unwrap()
        .env("SWITCH_CLI_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("SWITCH_CLI_SKIP_PROCESS_PROBE", "1")
        .args(["import-current", "codex", "a", "--kind", "oauth_capture"])
        .assert()
        .success();
    Command::cargo_bin("switch-cli")
        .unwrap()
        .env("SWITCH_CLI_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("SWITCH_CLI_SKIP_PROCESS_PROBE", "1")
        .args(["detach", "codex"])
        .assert()
        .success();
    Command::cargo_bin("switch-cli")
        .unwrap()
        .env("SWITCH_CLI_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("SWITCH_CLI_SKIP_PROCESS_PROBE", "1")
        .args(["use", "codex-a", "--yes"])
        .assert()
        .success();
    let backup_id = list_backup_ids(switch_home.path(), "codex")
        .pop()
        .expect("backup id");

    Command::cargo_bin("switch-cli")
        .unwrap()
        .env("SWITCH_CLI_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env(
            "SWITCH_CLI_PROCESS_PROBE_FIXTURE",
            "4242\tSun May 24 20:00:00 2026\t/usr/local/bin/codex",
        )
        .args([
            "restore-target",
            "codex",
            &backup_id,
            "--yes",
            "--allow-running",
        ])
        .assert()
        .failure()
        .stderr(contains("AppRunning"));

    Command::cargo_bin("switch-cli")
        .unwrap()
        .env("SWITCH_CLI_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env(
            "SWITCH_CLI_PROCESS_PROBE_FIXTURE",
            "4242\tSun May 24 20:00:00 2026\t/usr/local/bin/codex",
        )
        .args([
            "restore-target",
            "codex",
            &backup_id,
            "--yes",
            "--assume-app-stopped",
        ])
        .assert()
        .success();

    let history =
        fs::read_to_string(switch_home.path().join("state").join("history.jsonl")).unwrap();
    assert!(history.contains("\"operation\":\"restore-target\""));
    assert!(history.contains("\"type\":\"assume_app_stopped\""));
    assert!(history.contains("\"pid\":4242"));
    assert!(history.contains("\"start_time\":\"Sun May 24 20:00:00 2026\""));
    assert!(history.contains("/usr/local/bin/codex"));
}

#[test]
fn restore_target_prunes_backups_after_success() {
    let cwd = std::env::current_dir().unwrap();
    let switch_home = tempfile::Builder::new()
        .prefix(".test-switch-")
        .tempdir_in(&cwd)
        .unwrap();
    let codex_home = tempfile::Builder::new()
        .prefix(".test-codex-")
        .tempdir_in(&cwd)
        .unwrap();

    fs::create_dir_all(switch_home.path()).unwrap();
    fs::write(
        switch_home.path().join("profiles.yaml"),
        r#"
schema_version: 1
preferences:
  keep_backups: 2
profiles: []
"#,
    )
    .unwrap();

    Command::cargo_bin("switch-cli")
        .unwrap()
        .env("SWITCH_CLI_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("FIRST_API_KEY", "sk-first")
        .args([
            "add",
            "codex",
            "first",
            "--kind",
            "file_template",
            "--field",
            "model=gpt-5-codex",
            "--secret-field",
            "api_key=@env:FIRST_API_KEY",
        ])
        .assert()
        .success();

    Command::cargo_bin("switch-cli")
        .unwrap()
        .env("SWITCH_CLI_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("SECOND_API_KEY", "sk-second")
        .args([
            "add",
            "codex",
            "second",
            "--kind",
            "file_template",
            "--field",
            "model=gpt-5-codex",
            "--secret-field",
            "api_key=@env:SECOND_API_KEY",
        ])
        .assert()
        .success();

    Command::cargo_bin("switch-cli")
        .unwrap()
        .env("SWITCH_CLI_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("SWITCH_CLI_SKIP_PROCESS_PROBE", "1")
        .args(["use", "codex-first", "--yes"])
        .assert()
        .success();
    Command::cargo_bin("switch-cli")
        .unwrap()
        .env("SWITCH_CLI_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("SWITCH_CLI_SKIP_PROCESS_PROBE", "1")
        .args(["use", "codex-second", "--yes"])
        .assert()
        .success();

    let restore_from = list_backup_ids(switch_home.path(), "codex")
        .first()
        .cloned()
        .expect("backup id");
    let profiles_path = switch_home.path().join("profiles.yaml");
    let profiles_before_restore = fs::read_to_string(&profiles_path).unwrap();
    let profiles_mtime_before_restore = fs::metadata(&profiles_path).unwrap().modified().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));

    Command::cargo_bin("switch-cli")
        .unwrap()
        .env("SWITCH_CLI_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("SWITCH_CLI_SKIP_PROCESS_PROBE", "1")
        .args(["restore-target", "codex", &restore_from, "--yes"])
        .assert()
        .success();

    assert_eq!(
        fs::read_to_string(&profiles_path).unwrap(),
        profiles_before_restore
    );
    assert_eq!(
        fs::metadata(&profiles_path).unwrap().modified().unwrap(),
        profiles_mtime_before_restore
    );
    assert_eq!(list_backup_ids(switch_home.path(), "codex").len(), 2);
}

#[test]
fn restore_target_rejects_unsafe_backup_id_before_reading_manifest() {
    let cwd = std::env::current_dir().unwrap();
    let switch_home = tempfile::Builder::new()
        .prefix(".test-switch-")
        .tempdir_in(&cwd)
        .unwrap();
    let codex_home = tempfile::Builder::new()
        .prefix(".test-codex-")
        .tempdir_in(&cwd)
        .unwrap();
    fs::create_dir_all(codex_home.path()).unwrap();
    let auth_path = codex_home.path().join("auth.json");
    fs::write(
        &auth_path,
        serde_json::to_vec_pretty(&serde_json::json!({
            "auth_mode": "apikey",
            "OPENAI_API_KEY": "sk-live"
        }))
        .unwrap(),
    )
    .unwrap();

    let escape_dir = switch_home.path().join("backups").join("escape");
    fs::create_dir_all(&escape_dir).unwrap();
    let blob = serde_json::to_vec_pretty(&serde_json::json!({
        "auth_mode": "apikey",
        "OPENAI_API_KEY": "sk-escaped"
    }))
    .unwrap();
    fs::write(escape_dir.join("target-0.bak"), &blob).unwrap();
    fs::write(
        escape_dir.join("manifest.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": 1,
            "operation_id": "escape-restore",
            "app": "codex",
            "created_at": "2026-05-25T00:00:00Z",
            "targets": [{
                "target_id": format!("file:{}", auth_path.display()),
                "type": "file",
                "requires_app_stopped": false,
                "path": auth_path.display().to_string(),
                "resolved_path": auth_path.display().to_string(),
                "stored_as": "target-0.bak",
                "sha256": switch_cli::backup::sha256_hex(&blob)
            }]
        }))
        .unwrap(),
    )
    .unwrap();

    Command::cargo_bin("switch-cli")
        .unwrap()
        .env("SWITCH_CLI_HOME", switch_home.path())
        .env("SWITCH_CLI_TEST_HOME", &cwd)
        .env("CODEX_HOME", codex_home.path())
        .env("SWITCH_CLI_SKIP_PROCESS_PROBE", "1")
        .args(["restore-target", "codex", "../escape", "--yes"])
        .assert()
        .failure()
        .stderr(contains("BackupInvalid: invalid backup_id"));

    let live = fs::read_to_string(&auth_path).unwrap();
    assert!(live.contains("sk-live"));
    assert!(!live.contains("sk-escaped"));
}
