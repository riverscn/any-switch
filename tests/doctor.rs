use assert_cmd::Command;
use predicates::prelude::PredicateBooleanExt;
use predicates::str::{contains, is_match};
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
mod support;
use support::*;
use tempfile::tempdir;

fn add_claude_oauth_account_extra_key(home: &std::path::Path) {
    let path = home.join(".claude.json");
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["oauthAccount"]["subscriptionRegion"] = serde_json::json!("us");
    fs::write(path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
}

#[cfg(unix)]
#[test]
fn doctor_reports_permission_warnings() {
    let switch_home = tempdir().unwrap();
    let mut root_permissions = fs::metadata(switch_home.path()).unwrap().permissions();
    root_permissions.set_mode(0o755);
    fs::set_permissions(switch_home.path(), root_permissions).unwrap();
    fs::write(
        switch_home.path().join("profiles.yaml"),
        "schema_version: 1\nprofiles: []\n",
    )
    .unwrap();
    let mut permissions = fs::metadata(switch_home.path().join("profiles.yaml"))
        .unwrap()
        .permissions();
    permissions.set_mode(0o644);
    fs::set_permissions(switch_home.path().join("profiles.yaml"), permissions).unwrap();
    for dir in ["apps.d", "overrides.d", "captures", "backups"] {
        let path = switch_home.path().join(dir);
        fs::create_dir_all(&path).unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).unwrap();
    }
    for file in [
        switch_home.path().join("apps.d").join("bad.txt"),
        switch_home.path().join("overrides.d").join("bad.txt"),
        switch_home.path().join("captures").join("bad.json"),
        switch_home.path().join("backups").join("bad.json"),
    ] {
        fs::write(&file, "x").unwrap();
        let mut permissions = fs::metadata(&file).unwrap().permissions();
        permissions.set_mode(0o644);
        fs::set_permissions(&file, permissions).unwrap();
    }

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("ANY_SWITCH_TEST_HOME", switch_home.path().parent().unwrap())
        .args(["doctor"])
        .assert()
        .success()
        .stdout(contains("profiles.yaml secret-leak surface"))
        .stdout(contains("permissions\twarning"))
        .stdout(contains("expected 0700"))
        .stdout(contains("expected 0600"))
        .stdout(contains("apps.d"))
        .stdout(contains("overrides.d"))
        .stdout(contains("captures"))
        .stdout(contains("backups"));
}

#[test]
fn doctor_warns_when_switch_home_is_under_cloud_sync_root() {
    let home = tempdir().unwrap();
    let switch_home = home.path().join("Dropbox").join(".any-switch");

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_TEST_HOME", home.path())
        .env("ANY_SWITCH_HOME", &switch_home)
        .args(["doctor"])
        .assert()
        .success()
        .stdout(contains(
            "profiles.yaml secret-leak surface\twarning: ANY_SWITCH_HOME appears to be under a cloud sync directory",
        ));
}

#[test]
fn doctor_reports_backup_usage() {
    let cwd = std::env::current_dir().unwrap();
    let switch_home = tempfile::Builder::new()
        .prefix(".test-switch-")
        .tempdir_in(&cwd)
        .unwrap();
    let codex_home = tempfile::Builder::new()
        .prefix(".test-codex-")
        .tempdir_in(&cwd)
        .unwrap();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("TEST_API_KEY", "sk-test")
        .args([
            "add",
            "codex",
            "backup-report",
            "--kind",
            "file_template",
            "--field",
            "model=gpt-5-codex",
            "--secret-field",
            "api_key=@env:TEST_API_KEY",
        ])
        .assert()
        .success();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args(["use", "codex-backup-report", "--yes"])
        .assert()
        .success();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .args(["doctor", "codex"])
        .assert()
        .success()
        .stdout(contains("backups\tcodex\tcount=1"))
        .stdout(contains("inode_bytes="))
        .stdout(contains("logical_bytes="));

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .args(["backup", "list", "codex", "--json"])
        .assert()
        .success()
        .stdout(contains("\"app\": \"codex\""))
        .stdout(contains("\"backup_id\""));
}

#[test]
fn doctor_warns_when_backup_usage_exceeds_soft_limit() {
    let switch_home = tempdir().unwrap();
    let backup_dir = switch_home
        .path()
        .join("backups")
        .join("codex")
        .join("20260525T000000.000Z-test");
    fs::create_dir_all(&backup_dir).unwrap();
    let blob = backup_dir.join("target-0.bak");
    fs::File::create(&blob)
        .unwrap()
        .set_len(100 * 1024 * 1024 + 1)
        .unwrap();
    fs::write(
        backup_dir.join("manifest.json"),
        r#"{
  "schema_version": 1,
  "operation_id": "test",
  "app": "codex",
  "created_at": "2026-05-25T00:00:00Z",
  "targets": [
    {
      "target_id": "file:/tmp/auth.json",
      "type": "file",
      "requires_app_stopped": false,
      "path": "/tmp/auth.json",
      "resolved_path": "/tmp/auth.json",
      "stored_as": "target-0.bak",
      "sha256": "unused"
    }
  ]
}
"#,
    )
    .unwrap();
    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("ANY_SWITCH_TEST_HOME", switch_home.path().parent().unwrap())
        .args(["doctor"])
        .assert()
        .success()
        .stdout(contains("backups\tcodex\tcount=1"))
        .stdout(contains("warning: exceeds 100 MB soft limit"));
}

#[test]
fn doctor_backup_usage_ignores_unsafe_manifest_stored_as_paths() {
    let switch_home = tempdir().unwrap();
    fs::write(switch_home.path().join("sentinel"), "outside-backup").unwrap();
    let backup_dir = switch_home
        .path()
        .join("backups")
        .join("codex")
        .join("20260525T000000.000Z-test");
    fs::create_dir_all(&backup_dir).unwrap();
    fs::write(backup_dir.join("target-0.bak"), "inside").unwrap();
    fs::write(
        backup_dir.join("manifest.json"),
        r#"{
  "schema_version": 1,
  "operation_id": "test",
  "app": "codex",
  "created_at": "2026-05-25T00:00:00Z",
  "targets": [
    {
      "target_id": "file:/tmp/auth.json",
      "type": "file",
      "requires_app_stopped": false,
      "path": "/tmp/auth.json",
      "resolved_path": "/tmp/auth.json",
      "stored_as": "../sentinel",
      "sha256": "unused"
    }
  ]
}
"#,
    )
    .unwrap();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("ANY_SWITCH_TEST_HOME", switch_home.path().parent().unwrap())
        .args(["doctor"])
        .assert()
        .success()
        .stdout(contains("backups\tcodex\tcount=1"))
        .stdout(contains("logical_bytes=0"));
}

#[test]
fn doctor_app_reports_process_and_active_target() {
    let cwd = std::env::current_dir().unwrap();
    let switch_home = tempfile::Builder::new()
        .prefix(".test-switch-")
        .tempdir_in(&cwd)
        .unwrap();
    let codex_home = tempfile::Builder::new()
        .prefix(".test-codex-")
        .tempdir_in(&cwd)
        .unwrap();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("TEST_API_KEY", "sk-test")
        .args([
            "add",
            "codex",
            "doctor",
            "--kind",
            "file_template",
            "--field",
            "model=gpt-5-codex",
            "--secret-field",
            "api_key=@env:TEST_API_KEY",
        ])
        .assert()
        .success();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args(["use", "codex-doctor", "--yes"])
        .assert()
        .success();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env(
            "ANY_SWITCH_PROCESS_PROBE_FIXTURE",
            "5150\tSun May 24 21:00:00 2026\t/usr/local/bin/codex",
        )
        .args(["doctor", "codex"])
        .assert()
        .success()
        .stdout(contains("process\tpid=5150"))
        .stdout(contains("start_time=Sun May 24 21:00:00 2026"))
        .stdout(contains("active_profile\tcodex-doctor"))
        .stdout(contains(
            codex_home.path().join("auth.json").display().to_string(),
        ));
}

#[test]
fn doctor_warns_instead_of_failing_when_process_probe_errors() {
    let switch_home = tempdir().unwrap();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("ANY_SWITCH_TEST_HOME", switch_home.path().parent().unwrap())
        .env("ANY_SWITCH_PROCESS_PROBE_ERROR_FIXTURE", "ps denied")
        .args(["doctor", "codex"])
        .assert()
        .success()
        .stdout(contains(
            "processes\twarning: process probe fixture error: ps denied",
        ));

    let output = Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("ANY_SWITCH_TEST_HOME", switch_home.path().parent().unwrap())
        .env("ANY_SWITCH_PROCESS_PROBE_ERROR_FIXTURE", "ps denied")
        .args(["doctor", "codex", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["app"]["process_status"], "warning");
    assert_eq!(
        value["app"]["process_warning"],
        "process probe fixture error: ps denied"
    );
}

#[test]
fn doctor_reports_definition_driven_json_fields_and_stale_warnings() {
    let cwd = std::env::current_dir().unwrap();
    let switch_home = tempfile::Builder::new()
        .prefix(".test-switch-")
        .tempdir_in(&cwd)
        .unwrap();
    let codex_home = tempfile::Builder::new()
        .prefix(".test-codex-")
        .tempdir_in(&cwd)
        .unwrap();
    fs::write(
        codex_home.path().join("auth.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "auth_mode": "chatgpt",
            "last_refresh": "2025-01-01T00:00:00Z",
            "tokens": {
                "account_id": "acct-a",
                "refresh_token": "refresh-a",
                "id_token": "x.eyJzdWIiOiIxMjMiLCJlbWFpbCI6ImFAYi5jIn0.y"
            }
        }))
        .unwrap(),
    )
    .unwrap();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args(["doctor", "codex"])
        .assert()
        .success()
        .stdout(contains(
            "definition_target\toauth_capture\tfile_capture\texists",
        ))
        .stdout(contains("definition_json_field\tauth_mode\tchatgpt"))
        .stdout(contains(
            "definition_json_field\tlast_refresh\t2025-01-01T00:00:00Z",
        ))
        .stdout(contains(
            "definition_json_field\tlast_refresh\twarning: older than 90 days",
        ))
        .stdout(contains("definition_identity\toauth_capture"))
        .stdout(contains("acct-a"))
        .stdout(contains("a@b.c"))
        .stdout(is_match("refresh-a").unwrap().not());
}

#[test]
fn doctor_uses_user_definition_without_builtin_app_assumptions() {
    let cwd = std::env::current_dir().unwrap();
    let switch_home = tempfile::Builder::new()
        .prefix(".test-switch-")
        .tempdir_in(&cwd)
        .unwrap();
    let target_dir = tempfile::Builder::new()
        .prefix(".test-widget-")
        .tempdir_in(&cwd)
        .unwrap();
    let state_path = target_dir.path().join("state.json");
    fs::write(
        &state_path,
        serde_json::to_vec(&serde_json::json!({
            "account": "acct-widget",
            "token": "secret-widget",
        }))
        .unwrap(),
    )
    .unwrap();
    let apps_dir = switch_home.path().join("apps.d");
    fs::create_dir_all(&apps_dir).unwrap();
    fs::write(
        apps_dir.join("widget.yaml"),
        format!(
            r#"
schema_version: 1
app:
  id: widget
  display_name: Widget
  definition_version: 1
process_probe:
  names: [widget-runner]
doctor:
  json_fields:
    - name: account
      path: {}
      json_path: $.account
    - name: token
      path: {}
      json_path: $.token
      sensitive: true
"#,
            state_path.display(),
            state_path.display()
        ),
    )
    .unwrap();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env(
            "ANY_SWITCH_PROCESS_PROBE_FIXTURE",
            "6060\tSun May 24 21:00:00 2026\t/opt/widget-runner",
        )
        .args(["doctor", "widget"])
        .assert()
        .success()
        .stdout(contains("app\twidget"))
        .stdout(contains("definition_json_field\taccount\tacct-widget"))
        .stdout(contains("definition_json_field\ttoken\tpresent"))
        .stdout(contains("process\tpid=6060"))
        .stdout(is_match("secret-widget").unwrap().not());

    let output = Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env(
            "ANY_SWITCH_PROCESS_PROBE_FIXTURE",
            "6060\tSun May 24 21:00:00 2026\t/opt/widget-runner",
        )
        .args(["doctor", "widget", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["app"]["id"], "widget");
    assert_eq!(value["app"]["process_status"], "running");
    assert_eq!(value["app"]["processes"][0]["pid"], 6060);
    let fields = value["app"]["definition"].as_array().unwrap();
    assert!(fields.iter().any(|field| {
        field["type"] == "json_field"
            && field["name"] == "account"
            && field["value"] == "acct-widget"
    }));
    assert!(fields.iter().any(|field| {
        field["type"] == "json_field" && field["name"] == "token" && field["value"] == "present"
    }));
    assert!(!String::from_utf8_lossy(&output.stdout).contains("secret-widget"));
}

#[test]
fn doctor_reports_definition_driven_non_secret_target_summary() {
    let cwd = std::env::current_dir().unwrap();
    let home = tempfile::Builder::new()
        .prefix(".test-home-")
        .tempdir_in(&cwd)
        .unwrap();
    let switch_home = home.path().join(".any-switch");
    let claude_dir = home.path().join(".claude");
    fs::create_dir_all(&claude_dir).unwrap();
    fs::write(
        claude_dir.join("settings.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "env": {
                "ANTHROPIC_AUTH_TOKEN": "settings-secret",
                "ANTHROPIC_MODEL": "claude-sonnet",
                "KEEP": "1"
            },
            "apiKeyHelper": "/usr/local/bin/secret-helper"
        }))
        .unwrap(),
    )
    .unwrap();
    write_claude_credentials(home.path(), "acct-a", "org-a", "refresh-a");
    write_claude_json(home.path(), "acct-a", "org-a", "user-a", "original");
    add_claude_oauth_account_extra_key(home.path());

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("HOME", home.path())
        .env("ANY_SWITCH_TEST_HOME", home.path())
        .env("ANY_SWITCH_HOME", &switch_home)
        .args(["doctor", "claude"])
        .assert()
        .success()
        .stdout(contains(
            "definition_target\tenv_injection\tjson_env_merge\texists",
        ))
        .stdout(contains(
            "definition_managed_keys\tenv_injection\tANTHROPIC_AUTH_TOKEN,ANTHROPIC_MODEL",
        ))
        .stdout(contains(
            "definition_json_path\tenv_injection\tjson_env_merge\tpresent\t$.env",
        ))
        .stdout(contains(
            "definition_target\toauth_capture\tjson_subtree\texists",
        ))
        .stdout(contains(
            "definition_json_path\toauth_capture\tjson_subtree\tpresent\t$.oauthAccount",
        ))
        .stdout(contains(
            "definition_json_object_schema\toauth_account\twarning:",
        ))
        .stdout(contains("extra_keys=subscriptionRegion"))
        .stdout(contains("definition_identity\toauth_capture"))
        .stdout(contains("account_uuid\":\"acct-a"))
        .stdout(contains("email\":\"work@example.test"))
        .stdout(is_match("settings-secret").unwrap().not())
        .stdout(is_match("refresh-a").unwrap().not())
        .stdout(is_match("secret-helper").unwrap().not());
}

#[test]
fn doctor_json_reports_definition_summary_without_secret_values() {
    let cwd = std::env::current_dir().unwrap();
    let home = tempfile::Builder::new()
        .prefix(".test-home-")
        .tempdir_in(&cwd)
        .unwrap();
    let switch_home = home.path().join(".any-switch");
    let claude_dir = home.path().join(".claude");
    fs::create_dir_all(&claude_dir).unwrap();
    fs::write(
        claude_dir.join("settings.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "env": {
                "ANTHROPIC_AUTH_TOKEN": "settings-secret",
                "ANTHROPIC_MODEL": "claude-sonnet",
                "KEEP": "1"
            },
            "apiKeyHelper": "/usr/local/bin/secret-helper"
        }))
        .unwrap(),
    )
    .unwrap();
    write_claude_credentials(home.path(), "acct-a", "org-a", "refresh-a");
    write_claude_json(home.path(), "acct-a", "org-a", "user-a", "original");
    add_claude_oauth_account_extra_key(home.path());

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("HOME", home.path())
        .env("ANY_SWITCH_TEST_HOME", home.path())
        .env("ANY_SWITCH_HOME", &switch_home)
        .env("PROFILE_TOKEN", "profile-token")
        .args([
            "add",
            "claude",
            "proxy",
            "--kind",
            "env_injection",
            "--field",
            "base_url=https://example.test",
            "--secret-field",
            "auth_token=@env:PROFILE_TOKEN",
        ])
        .assert()
        .success();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("HOME", home.path())
        .env("ANY_SWITCH_TEST_HOME", home.path())
        .env("ANY_SWITCH_HOME", &switch_home)
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args(["use", "claude-proxy", "--yes"])
        .assert()
        .success();

    let output = Command::cargo_bin("any-switch")
        .unwrap()
        .env("HOME", home.path())
        .env("ANY_SWITCH_TEST_HOME", home.path())
        .env("ANY_SWITCH_HOME", &switch_home)
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .env("ANTHROPIC_AUTH_TOKEN", "external-secret")
        .args(["doctor", "claude", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(!stdout.contains("settings-secret"));
    assert!(!stdout.contains("refresh-a"));
    assert!(!stdout.contains("secret-helper"));
    assert!(!stdout.contains("profile-token"));
    assert!(!stdout.contains("external-secret"));
    let value: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["app"]["id"], "claude");
    assert_eq!(value["app"]["process_status"], "ok");
    assert_eq!(value["app"]["active"]["override_status"], "warning");
    assert!(value["app"]["active"]["override_reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|reason| reason == "process_env:ANTHROPIC_AUTH_TOKEN"));
    let records = value["app"]["definition"].as_array().unwrap();
    assert!(records.iter().any(|record| {
        record["type"] == "json_object_schema"
            && record["name"] == "oauth_account"
            && record["status"] == "warning"
            && record["extra_keys"]
                .as_array()
                .unwrap()
                .iter()
                .any(|key| key == "subscriptionRegion")
    }));
    assert!(records.iter().any(|record| {
        record["type"] == "managed_keys"
            && record["kind"] == "env_injection"
            && record["keys"]
                .as_array()
                .unwrap()
                .iter()
                .any(|key| key == "ANTHROPIC_AUTH_TOKEN")
    }));
    assert!(records.iter().any(|record| {
        record["type"] == "identity"
            && record["kind"] == "oauth_capture"
            && record["identity"]["account_uuid"] == "acct-a"
    }));
}

#[test]
fn doctor_reports_missing_oauth_capture() {
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

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args([
            "import-current",
            "codex",
            "broken",
            "--kind",
            "oauth_capture",
        ])
        .assert()
        .success();
    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args(["use", "codex-broken", "--yes"])
        .assert()
        .success();

    fs::remove_file(
        switch_home
            .path()
            .join("captures")
            .join("codex-broken")
            .join("auth.json"),
    )
    .unwrap();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .args(["status", "codex"])
        .assert()
        .success()
        .stdout(contains("codex\tdrifted\tcodex-broken"))
        .stdout(contains("reason\tCaptureMissing"))
        .stdout(contains("import-current"));

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .args(["status", "codex", "--json"])
        .assert()
        .success()
        .stdout(contains("\"status\": \"drifted\""))
        .stdout(contains("\"reason\": \"CaptureMissing"))
        .stdout(contains("import-current"));

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .args(["doctor", "codex"])
        .assert()
        .success()
        .stdout(contains("capture\twarning: CaptureMissing"))
        .stdout(contains("import-current"));

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .args(["doctor", "codex", "--json"])
        .assert()
        .success()
        .stdout(contains("\"capture\""))
        .stdout(contains("\"status\": \"warning\""))
        .stdout(contains("CaptureMissing"))
        .stdout(contains("import-current"));
}
