use assert_cmd::Command;
use predicates::prelude::PredicateBooleanExt;
use predicates::str::{contains, is_match};
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
mod support;
use support::*;
use tempfile::tempdir;

#[test]
fn add_rejects_sensitive_field_argument() {
    let switch_home = tempdir().unwrap();
    let mut cmd = Command::cargo_bin("any-switch").unwrap();
    cmd.env("ANY_SWITCH_HOME", switch_home.path())
        .env("ANY_SWITCH_TEST_HOME", switch_home.path().parent().unwrap())
        .args([
            "add",
            "codex",
            "bad",
            "--kind",
            "file_template",
            "--field",
            "api_key=sk-test",
        ])
        .assert()
        .failure()
        .stderr(contains("UnsafeSecretArgument"));
}

#[test]
fn add_rejects_invalid_field_keys() {
    let switch_home = tempdir().unwrap();
    for field in [
        "=value",
        ".nested=value",
        "nested.=value",
        "nested..value=x",
    ] {
        Command::cargo_bin("any-switch")
            .unwrap()
            .env("ANY_SWITCH_HOME", switch_home.path())
            .env("ANY_SWITCH_TEST_HOME", switch_home.path().parent().unwrap())
            .args([
                "add",
                "codex",
                "bad-key",
                "--kind",
                "file_template",
                "--field",
                field,
            ])
            .assert()
            .failure()
            .stderr(contains("FieldInvalid: invalid field key"));
    }
}

#[test]
fn show_redacts_secret_fields() {
    let switch_home = tempdir().unwrap();
    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("ANY_SWITCH_TEST_HOME", switch_home.path().parent().unwrap())
        .env("TEST_API_KEY", "sk-test-secret")
        .args([
            "add",
            "codex",
            "safe",
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
        .env("ANY_SWITCH_TEST_HOME", switch_home.path().parent().unwrap())
        .args(["show", "codex-safe"])
        .assert()
        .success()
        .stdout(contains("api_key: '***'"))
        .stdout(is_match("sk-test-secret").unwrap().not());
}

#[test]
fn list_json_redacts_secret_fields() {
    let switch_home = tempdir().unwrap();
    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("ANY_SWITCH_TEST_HOME", switch_home.path().parent().unwrap())
        .env("TEST_API_KEY", "sk-list-secret")
        .args([
            "add",
            "codex",
            "list-safe",
            "--kind",
            "file_template",
            "--field",
            "model=gpt-5-codex",
            "--secret-field",
            "api_key=@env:TEST_API_KEY",
        ])
        .assert()
        .success();

    let output = Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("ANY_SWITCH_TEST_HOME", switch_home.path().parent().unwrap())
        .args(["list", "codex", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(!stdout.contains("sk-list-secret"));
    let rows: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(rows[0]["fields"]["api_key"], "***");
}

#[test]
fn list_rejects_unknown_app_filter() {
    let switch_home = tempdir().unwrap();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("ANY_SWITCH_TEST_HOME", switch_home.path().parent().unwrap())
        .args(["list", "doesnotexist"])
        .assert()
        .failure()
        .stderr(contains("AppNotFound: doesnotexist"));

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("ANY_SWITCH_TEST_HOME", switch_home.path().parent().unwrap())
        .args(["list", "doesnotexist", "--json"])
        .assert()
        .failure()
        .stderr(contains("AppNotFound: doesnotexist"));
}

#[test]
fn use_and_status_do_not_modify_profiles_yaml_for_static_profiles() {
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
        .env("ANY_SWITCH_TEST_HOME", &cwd)
        .env("TEST_API_KEY", "sk-no-profile-write")
        .args([
            "add",
            "codex",
            "stable",
            "--kind",
            "file_template",
            "--field",
            "model=gpt-5-codex",
            "--secret-field",
            "api_key=@env:TEST_API_KEY",
        ])
        .assert()
        .success();

    let profiles_path = switch_home.path().join("profiles.yaml");
    let before_text = fs::read_to_string(&profiles_path).unwrap();
    let before_modified = fs::metadata(&profiles_path).unwrap().modified().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("ANY_SWITCH_TEST_HOME", &cwd)
        .env("CODEX_HOME", codex_home.path())
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args(["use", "codex-stable", "--yes"])
        .assert()
        .success();

    assert_eq!(fs::read_to_string(&profiles_path).unwrap(), before_text);
    assert_eq!(
        fs::metadata(&profiles_path).unwrap().modified().unwrap(),
        before_modified
    );

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("ANY_SWITCH_TEST_HOME", &cwd)
        .env("CODEX_HOME", codex_home.path())
        .args(["status", "codex"])
        .assert()
        .success()
        .stdout(contains("codex\tmatched\tcodex-stable"));

    assert_eq!(fs::read_to_string(&profiles_path).unwrap(), before_text);
    assert_eq!(
        fs::metadata(&profiles_path).unwrap().modified().unwrap(),
        before_modified
    );
}

#[test]
fn secret_file_must_resolve_inside_home() {
    let cwd = std::env::current_dir().unwrap();
    let home = tempfile::Builder::new()
        .prefix(".test-home-")
        .tempdir_in(&cwd)
        .unwrap();
    let switch_home = home.path().join(".any-switch");
    let outside = tempdir().unwrap();
    let secret = outside.path().join("secret.txt");
    fs::write(&secret, "sk-outside").unwrap();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("HOME", home.path())
        .env("ANY_SWITCH_TEST_HOME", home.path())
        .env("ANY_SWITCH_HOME", &switch_home)
        .args([
            "add",
            "codex",
            "outside-secret",
            "--kind",
            "file_template",
            "--field",
            "model=gpt-5-codex",
            "--secret-field",
            &format!("api_key=@file:{}", secret.display()),
        ])
        .assert()
        .failure()
        .stderr(contains("SecretFileOutsideHome"));
}

#[cfg(unix)]
#[test]
fn secret_file_rejects_group_or_other_permissions() {
    let cwd = std::env::current_dir().unwrap();
    let home = tempfile::Builder::new()
        .prefix(".test-home-")
        .tempdir_in(&cwd)
        .unwrap();
    let switch_home = home.path().join(".any-switch");
    let secret = home.path().join("secret.txt");
    fs::write(&secret, "sk-wide").unwrap();
    let mut permissions = fs::metadata(&secret).unwrap().permissions();
    permissions.set_mode(0o644);
    fs::set_permissions(&secret, permissions).unwrap();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("HOME", home.path())
        .env("ANY_SWITCH_TEST_HOME", home.path())
        .env("ANY_SWITCH_HOME", &switch_home)
        .args([
            "add",
            "codex",
            "wide-secret",
            "--kind",
            "file_template",
            "--field",
            "model=gpt-5-codex",
            "--secret-field",
            &format!("api_key=@file:{}", secret.display()),
        ])
        .assert()
        .failure()
        .stderr(contains("UnsafeSecretFilePermissions"));
}

#[test]
fn secret_prompt_requires_tty() {
    let switch_home = tempdir().unwrap();
    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("ANY_SWITCH_TEST_HOME", switch_home.path().parent().unwrap())
        .args([
            "add",
            "codex",
            "prompt-secret",
            "--kind",
            "file_template",
            "--field",
            "model=gpt-5-codex",
            "--secret-field",
            "api_key=@prompt",
        ])
        .assert()
        .failure()
        .stderr(contains("@prompt requires an interactive TTY"));
}

#[cfg(unix)]
#[test]
fn target_write_follows_existing_final_symlink() {
    let cwd = std::env::current_dir().unwrap();
    let switch_home = tempfile::Builder::new()
        .prefix(".test-switch-")
        .tempdir_in(&cwd)
        .unwrap();
    let codex_home = tempfile::Builder::new()
        .prefix(".test-codex-")
        .tempdir_in(&cwd)
        .unwrap();
    let real_auth = codex_home.path().join("real-auth.json");
    let auth_link = codex_home.path().join("auth.json");
    fs::write(&real_auth, "{}").unwrap();
    std::os::unix::fs::symlink(&real_auth, &auth_link).unwrap();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("TEST_API_KEY", "sk-symlink")
        .args([
            "add",
            "codex",
            "symlink",
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
        .args(["use", "codex-symlink", "--yes"])
        .assert()
        .success();

    assert!(fs::symlink_metadata(&auth_link)
        .unwrap()
        .file_type()
        .is_symlink());
    assert!(fs::read_to_string(real_auth)
        .unwrap()
        .contains("sk-symlink"));
}

#[test]
fn status_reports_matched_with_overrides_without_leaking_env_secret() {
    let cwd = std::env::current_dir().unwrap();
    let home = tempfile::Builder::new()
        .prefix(".test-home-")
        .tempdir_in(&cwd)
        .unwrap();
    let switch_home = home.path().join(".any-switch");

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

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("HOME", home.path())
        .env("ANY_SWITCH_TEST_HOME", home.path())
        .env("ANY_SWITCH_HOME", &switch_home)
        .env("ANTHROPIC_AUTH_TOKEN", "external-secret")
        .args(["status", "claude", "--json"])
        .assert()
        .success()
        .stdout(contains("\"status\": \"matched-with-overrides\""))
        .stdout(contains("process_env:ANTHROPIC_AUTH_TOKEN"))
        .stdout(is_match("external-secret").unwrap().not());
}

#[test]
fn status_reports_managed_policy_overrides_without_leaking_secret_values() {
    let cwd = std::env::current_dir().unwrap();
    let home = tempfile::Builder::new()
        .prefix(".test-home-")
        .tempdir_in(&cwd)
        .unwrap();
    let switch_home = home.path().join(".any-switch");
    let managed_dir = tempfile::Builder::new()
        .prefix(".test-managed-claude-")
        .tempdir_in(&cwd)
        .unwrap();
    fs::write(
        managed_dir.path().join("managed-settings.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "env": {
                "ANTHROPIC_AUTH_TOKEN": "managed-secret-token"
            }
        }))
        .unwrap(),
    )
    .unwrap();
    let drop_in = managed_dir.path().join("managed-settings.d");
    fs::create_dir_all(&drop_in).unwrap();
    fs::write(
        drop_in.join("20-auth-helper.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "apiKeyHelper": "/usr/local/bin/secret-helper"
        }))
        .unwrap(),
    )
    .unwrap();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("HOME", home.path())
        .env("ANY_SWITCH_TEST_HOME", home.path())
        .env("ANY_SWITCH_HOME", &switch_home)
        .env("PROFILE_TOKEN", "profile-token")
        .args([
            "add",
            "claude",
            "managed",
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
        .args(["use", "claude-managed", "--yes"])
        .assert()
        .success();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("HOME", home.path())
        .env("ANY_SWITCH_TEST_HOME", home.path())
        .env("ANY_SWITCH_HOME", &switch_home)
        .env(
            "ANY_SWITCH_TEST_CLAUDE_MANAGED_SETTINGS_DIR",
            managed_dir.path(),
        )
        .args(["status", "claude", "--json"])
        .assert()
        .success()
        .stdout(contains("\"status\": \"matched-with-overrides\""))
        .stdout(contains("managed_settings:"))
        .stdout(contains("env:ANTHROPIC_AUTH_TOKEN"))
        .stdout(contains("apiKeyHelper"))
        .stdout(is_match("managed-secret-token").unwrap().not())
        .stdout(is_match("secret-helper").unwrap().not());
}

#[test]
fn status_no_active_includes_import_current_hint() {
    let switch_home = tempdir().unwrap();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("ANY_SWITCH_TEST_HOME", switch_home.path().parent().unwrap())
        .args(["status", "codex"])
        .assert()
        .success()
        .stdout(contains("codex\tno-active"))
        .stdout(contains(
            "recommended next step is `any-switch import-current codex",
        ))
        .stdout(contains("not `any-switch use"));

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("ANY_SWITCH_TEST_HOME", switch_home.path().parent().unwrap())
        .args(["status", "codex", "--json"])
        .assert()
        .success()
        .stdout(contains("\"status\": \"no-active\""))
        .stdout(contains("\"hint\""))
        .stdout(contains("any-switch import-current codex"));
}

#[test]
fn status_rejects_unknown_app_filter() {
    let switch_home = tempdir().unwrap();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("ANY_SWITCH_TEST_HOME", switch_home.path().parent().unwrap())
        .args(["status", "does-not-exist"])
        .assert()
        .failure()
        .stderr(contains("AppNotFound: does-not-exist"));

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("ANY_SWITCH_TEST_HOME", switch_home.path().parent().unwrap())
        .args(["status", "does-not-exist", "--json"])
        .assert()
        .failure()
        .stderr(contains("AppNotFound: does-not-exist"));
}

#[test]
fn status_rejects_invalid_active_state_ids() {
    let switch_home = tempdir().unwrap();
    fs::create_dir_all(switch_home.path().join("state")).unwrap();
    fs::write(
        switch_home.path().join("state").join("active.json"),
        r#"{"schema_version":1,"active_profiles":{"../escape":{"id":"codex-safe"}}}"#,
    )
    .unwrap();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("ANY_SWITCH_TEST_HOME", switch_home.path().parent().unwrap())
        .args(["status"])
        .assert()
        .failure()
        .stderr(contains("StateInvalid: invalid active app id"));
}

#[test]
fn status_reports_missing_when_static_target_file_is_absent() {
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
            "missing-target",
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
        .args(["use", "codex-missing-target", "--yes"])
        .assert()
        .success();

    fs::remove_file(codex_home.path().join("auth.json")).unwrap();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .args(["status", "codex"])
        .assert()
        .success()
        .stdout(contains("codex\tmissing\tcodex-missing-target"))
        .stdout(contains("reason\ttarget_missing"));

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .args(["status", "codex", "--json"])
        .assert()
        .success()
        .stdout(contains("\"status\": \"missing\""))
        .stdout(contains("\"reason\": \"target_missing\""));
}

#[test]
fn edit_updates_static_profile_and_cleans_fragment() {
    let switch_home = tempdir().unwrap();
    let editor_dir = tempdir().unwrap();
    let editor = write_editor_script(
        editor_dir.path(),
        r#"#!/bin/sh
sed 's/model: gpt-5-codex/model: edited-model/' "$1" > "$1.next" && mv "$1.next" "$1"
"#,
    );

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("ANY_SWITCH_TEST_HOME", switch_home.path().parent().unwrap())
        .env("TEST_API_KEY", "sk-test")
        .args([
            "add",
            "codex",
            "editable",
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
        .env("ANY_SWITCH_TEST_HOME", switch_home.path().parent().unwrap())
        .env("EDITOR", &editor)
        .args(["edit", "codex-editable"])
        .assert()
        .success()
        .stdout(contains("edited codex-editable"));

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("ANY_SWITCH_TEST_HOME", switch_home.path().parent().unwrap())
        .args(["show", "codex-editable"])
        .assert()
        .success()
        .stdout(contains("edited-model"));

    let edit_dir = switch_home.path().join("state").join("edit");
    let leftover = fs::read_dir(edit_dir).unwrap().count();
    assert_eq!(leftover, 0);
}

#[cfg(unix)]
#[test]
fn edit_falls_back_to_vim_when_editor_env_is_missing() {
    let switch_home = tempdir().unwrap();
    let editor_dir = tempdir().unwrap();
    let vim = editor_dir.path().join("vim");
    fs::write(
        &vim,
        r#"#!/bin/sh
/usr/bin/sed 's/model: gpt-5-codex/model: vim-fallback/' "$1" > "$1.next" && /bin/mv "$1.next" "$1"
"#,
    )
    .unwrap();
    let mut permissions = fs::metadata(&vim).unwrap().permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&vim, permissions).unwrap();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("ANY_SWITCH_TEST_HOME", switch_home.path().parent().unwrap())
        .env("TEST_API_KEY", "sk-test")
        .args([
            "add",
            "codex",
            "fallback-edit",
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
        .env("ANY_SWITCH_TEST_HOME", switch_home.path().parent().unwrap())
        .env("PATH", editor_dir.path())
        .env_remove("VISUAL")
        .env_remove("EDITOR")
        .args(["edit", "codex-fallback-edit"])
        .assert()
        .success()
        .stdout(contains("edited codex-fallback-edit"));

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("ANY_SWITCH_TEST_HOME", switch_home.path().parent().unwrap())
        .args(["show", "codex-fallback-edit"])
        .assert()
        .success()
        .stdout(contains("vim-fallback"));
}

#[test]
fn edit_rejects_immutable_field_changes() {
    let switch_home = tempdir().unwrap();
    let editor_dir = tempdir().unwrap();
    let editor = write_editor_script(
        editor_dir.path(),
        r#"#!/bin/sh
sed 's/id: codex-immutable/id: codex-renamed/' "$1" > "$1.next" && mv "$1.next" "$1"
"#,
    );

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("ANY_SWITCH_TEST_HOME", switch_home.path().parent().unwrap())
        .env("TEST_API_KEY", "sk-test")
        .args([
            "add",
            "codex",
            "immutable",
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
        .env("ANY_SWITCH_TEST_HOME", switch_home.path().parent().unwrap())
        .env("EDITOR", &editor)
        .args(["edit", "codex-immutable"])
        .assert()
        .failure()
        .stderr(contains("ImmutableFieldChanged"));

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("ANY_SWITCH_TEST_HOME", switch_home.path().parent().unwrap())
        .args(["show", "codex-immutable"])
        .assert()
        .success()
        .stdout(contains("codex-immutable"));
}

#[test]
fn edit_does_not_update_profile_or_open_editor_when_app_is_locked() {
    let switch_home = tempdir().unwrap();
    let test_home = switch_home.path().parent().unwrap().to_path_buf();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("ANY_SWITCH_TEST_HOME", &test_home)
        .env("TEST_API_KEY", "sk-first")
        .args([
            "add",
            "codex",
            "locked-edit",
            "--kind",
            "file_template",
            "--field",
            "model=gpt-5-codex",
            "--secret-field",
            "api_key=@env:TEST_API_KEY",
        ])
        .assert()
        .success();

    let paths = any_switch::paths::Paths {
        home: test_home,
        switch_home: switch_home.path().to_path_buf(),
    };
    paths.ensure_layout().unwrap();
    let _app_lock =
        any_switch::lock::FileLock::acquire(any_switch::lock::app_lock(&paths, "codex").unwrap())
            .unwrap();

    let editor_marker = switch_home.path().join("editor-ran");
    let editor_dir = tempdir().unwrap();
    let editor = write_editor_script(
        editor_dir.path(),
        &format!(
            "#!/bin/sh\ntouch '{}'\nsed 's/model: gpt-5-codex/model: edited-model/' \"$1\" > \"$1.next\" && mv \"$1.next\" \"$1\"\n",
            editor_marker.display()
        ),
    );

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("ANY_SWITCH_TEST_HOME", paths.home)
        .env("EDITOR", &editor)
        .args(["edit", "codex-locked-edit"])
        .assert()
        .failure()
        .stderr(contains("LockBusy"));

    assert!(!editor_marker.exists());
    let profiles = fs::read_to_string(switch_home.path().join("profiles.yaml")).unwrap();
    assert!(profiles.contains("model: gpt-5-codex"));
    assert!(!profiles.contains("edited-model"));
    let edit_dir = switch_home.path().join("state").join("edit");
    if edit_dir.exists() {
        assert_eq!(fs::read_dir(edit_dir).unwrap().count(), 0);
    }
}

#[test]
fn codex_use_rejects_non_file_credential_store_before_backup() {
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
        codex_home.path().join("config.toml"),
        "cli_auth_credentials_store = \"keyring\"\n",
    )
    .unwrap();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("TEST_API_KEY", "sk-blocked")
        .args([
            "add",
            "codex",
            "keyring",
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
        .args(["use", "codex-keyring", "--yes"])
        .assert()
        .failure()
        .stderr(contains("CredentialStoreUnsupported"));

    assert!(!switch_home.path().join("backups").join("codex").exists());
    assert!(!switch_home
        .path()
        .join("state")
        .join("pending-switch")
        .join("codex.json")
        .exists());
    assert!(!codex_home.path().join("auth.json").exists());
}

#[test]
fn codex_restore_rejects_non_file_credential_store_before_pending() {
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

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args(["use", "codex-first", "--yes"])
        .assert()
        .success();

    let backup_id = list_backup_ids(switch_home.path(), "codex")
        .pop()
        .expect("backup id");
    fs::write(
        codex_home.path().join("config.toml"),
        "cli_auth_credentials_store = \"keyring\"\n",
    )
    .unwrap();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args(["restore-target", "codex", &backup_id, "--yes"])
        .assert()
        .failure()
        .stderr(contains("CredentialStoreUnsupported"));

    assert!(!switch_home
        .path()
        .join("state")
        .join("pending-switch")
        .join("codex.json")
        .exists());
}

#[test]
fn use_reports_missing_optional_capture_blob_recorded_in_manifest() {
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
    fs::write(
        codex_home.path().join("config.toml"),
        "model = \"gpt-work\"\nmodel_provider = \"openai\"\n",
    )
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
            "optional",
            "--kind",
            "oauth_capture",
        ])
        .assert()
        .success();

    fs::remove_file(
        switch_home
            .path()
            .join("captures")
            .join("codex-optional")
            .join("config.managed.toml"),
    )
    .unwrap();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args(["use", "codex-optional", "--yes"])
        .assert()
        .failure()
        .stderr(contains("CaptureMissing"))
        .stderr(contains("config.managed.toml"))
        .stderr(contains("import-current"));
}

#[test]
fn import_current_oauth_refreshes_existing_profile_by_required_identity() {
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
            "original",
            "--kind",
            "oauth_capture",
        ])
        .assert()
        .success()
        .stdout(contains("imported codex-original"));

    write_codex_oauth(codex_home.path(), "acct-a", "refresh-a-rotated");
    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args([
            "import-current",
            "codex",
            "duplicate",
            "--kind",
            "oauth_capture",
        ])
        .assert()
        .success()
        .stdout(contains("updated codex-original"));

    let profiles = fs::read_to_string(switch_home.path().join("profiles.yaml")).unwrap();
    assert!(profiles.contains("id: codex-original"));
    assert!(!profiles.contains("codex-duplicate"));

    let capture = fs::read_to_string(
        switch_home
            .path()
            .join("captures")
            .join("codex-original")
            .join("auth.json"),
    )
    .unwrap();
    assert!(capture.contains("refresh-a-rotated"));

    let history =
        fs::read_to_string(switch_home.path().join("state").join("history.jsonl")).unwrap();
    assert!(history.contains("\"profile\":\"codex-original\""));
    assert!(history.contains("\"updated_existing\":true"));
}

#[test]
fn use_oauth_records_stale_capture_warning() {
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
        switch_home.path().join("profiles.yaml"),
        r#"
schema_version: 1
preferences:
  oauth_stale_warn_days: 1
profiles: []
"#,
    )
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
            "stale",
            "--kind",
            "oauth_capture",
        ])
        .assert()
        .success();

    let manifest_path = switch_home
        .path()
        .join("captures")
        .join("codex-stale")
        .join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["captured_at"] = serde_json::Value::String("2020-01-01T00:00:00Z".to_string());
    fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args(["detach", "codex"])
        .assert()
        .success();

    let dry_run_output = Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args(["use", "codex-stale", "--dry-run", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let dry_run_text = String::from_utf8(dry_run_output.clone()).unwrap();
    let dry_run_plan: serde_json::Value = serde_json::from_slice(&dry_run_output).unwrap();
    assert!(dry_run_text.contains("oauth_capture_stale"));
    assert!(dry_run_text.contains("\"threshold_days\": 1"));
    assert!(!dry_run_text.contains("refresh-a"));
    assert_eq!(dry_run_plan["defensive_backup"]["enabled"], true);
    assert_eq!(
        dry_run_plan["defensive_backup"]["targets"][0]["requires_app_stopped"],
        true
    );
    assert_eq!(dry_run_plan["post_write_verify"]["type"], "oauth_identity");
    assert_eq!(
        dry_run_plan["post_write_verify"]["required_identity"]["account_id"],
        "acct-a"
    );
    assert_eq!(dry_run_plan["identity"]["account_id"], "acct-a");

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args(["use", "codex-stale", "--yes"])
        .assert()
        .success();

    let history =
        fs::read_to_string(switch_home.path().join("state").join("history.jsonl")).unwrap();
    assert!(history.contains("\"type\":\"oauth_capture_stale\""));
    assert!(history.contains("\"threshold_days\":1"));
}

#[test]
fn newer_profile_schema_allows_read_only_but_blocks_writes() {
    let switch_home = tempdir().unwrap();
    let profiles_path = switch_home.path().join("profiles.yaml");
    fs::write(
        &profiles_path,
        r#"
schema_version: 1
profiles:
  - id: codex-future
    app: codex
    kind: file_template
    schema_version: 2
    name: future
    created_at: "2026-05-25T00:00:00Z"
    fields:
      api_key: sk-existing
      model: gpt-5-codex
    extensions:
      future: true
"#,
    )
    .unwrap();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("ANY_SWITCH_TEST_HOME", switch_home.path().parent().unwrap())
        .args(["list"])
        .assert()
        .success()
        .stdout(contains("codex-future"));

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("ANY_SWITCH_TEST_HOME", switch_home.path().parent().unwrap())
        .env("TEST_API_KEY", "sk-new")
        .args([
            "add",
            "codex",
            "new",
            "--kind",
            "file_template",
            "--secret-field",
            "api_key=@env:TEST_API_KEY",
        ])
        .assert()
        .failure()
        .stderr(contains("SchemaTooNew"))
        .stderr(contains("codex-future"));

    let unchanged = fs::read_to_string(profiles_path).unwrap();
    assert!(unchanged.contains("schema_version: 2"));
    assert!(unchanged.contains("future: true"));
    assert!(!unchanged.contains("codex-new"));
}

#[test]
fn newer_store_schema_allows_read_only_but_blocks_writes() {
    let switch_home = tempdir().unwrap();
    let profiles_path = switch_home.path().join("profiles.yaml");
    fs::write(
        &profiles_path,
        r#"
schema_version: 2
profiles:
  - id: codex-existing
    app: codex
    kind: file_template
    schema_version: 1
    name: existing
    created_at: "2026-05-25T00:00:00Z"
    fields:
      api_key: sk-existing
      model: gpt-5-codex
"#,
    )
    .unwrap();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("ANY_SWITCH_TEST_HOME", switch_home.path().parent().unwrap())
        .args(["list"])
        .assert()
        .success()
        .stdout(contains("codex-existing"));

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("ANY_SWITCH_TEST_HOME", switch_home.path().parent().unwrap())
        .env("TEST_API_KEY", "sk-new")
        .args([
            "add",
            "codex",
            "new",
            "--kind",
            "file_template",
            "--secret-field",
            "api_key=@env:TEST_API_KEY",
        ])
        .assert()
        .failure()
        .stderr(contains("SchemaTooNew"))
        .stderr(contains("profiles.yaml schema_version 2"));

    let unchanged = fs::read_to_string(profiles_path).unwrap();
    assert!(unchanged.contains("schema_version: 2"));
    assert!(!unchanged.contains("codex-new"));
}

#[test]
fn import_current_refresh_updates_capture_timestamp_for_stale_profile() {
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
        switch_home.path().join("profiles.yaml"),
        r#"
schema_version: 1
preferences:
  oauth_stale_warn_days: 1
profiles: []
"#,
    )
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
            "stale-refresh",
            "--kind",
            "oauth_capture",
        ])
        .assert()
        .success();

    let manifest_path = switch_home
        .path()
        .join("captures")
        .join("codex-stale-refresh")
        .join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["captured_at"] = serde_json::Value::String("2020-01-01T00:00:00Z".to_string());
    fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    write_codex_oauth(codex_home.path(), "acct-a", "refresh-a-new");
    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args([
            "import-current",
            "codex",
            "same-account",
            "--kind",
            "oauth_capture",
        ])
        .assert()
        .success()
        .stdout(contains("updated codex-stale-refresh"));

    let refreshed: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    assert_ne!(refreshed["captured_at"], "2020-01-01T00:00:00Z");
    assert!(refreshed["last_writeback_at"].is_null());

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args(["use", "codex-stale-refresh", "--dry-run", "--json"])
        .assert()
        .success()
        .stdout(is_match("oauth_capture_stale").unwrap().not());
}

#[test]
fn oauth_optional_identity_mismatch_warns_without_blocking() {
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
            "optional",
            "--kind",
            "oauth_capture",
        ])
        .assert()
        .success();

    let paths = any_switch::paths::Paths {
        home: cwd,
        switch_home: switch_home.path().to_path_buf(),
    };
    let mut store = any_switch::profiles::ProfileStore::load(&paths).unwrap();
    let profile = store
        .profiles
        .iter_mut()
        .find(|profile| profile.id == "codex-optional")
        .unwrap();
    profile.identity.insert(
        "email".to_string(),
        serde_json::Value::String("old@example.test".to_string()),
    );
    store.save(&paths).unwrap();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args(["detach", "codex"])
        .assert()
        .success();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args(["use", "codex-optional", "--yes"])
        .assert()
        .success();

    let history =
        fs::read_to_string(switch_home.path().join("state").join("history.jsonl")).unwrap();
    assert!(history.contains("\"type\":\"optional_identity_mismatch\""));
    assert!(history.contains("\"field\":\"email\""));
    assert!(history.contains("old@example.test"));
    assert!(history.contains("a@b.c"));
}

#[test]
fn oauth_identity_verify_failure_rolls_back_immediately() {
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

    let paths = any_switch::paths::Paths {
        home: cwd,
        switch_home: switch_home.path().to_path_buf(),
    };
    let mut store = any_switch::profiles::ProfileStore::load(&paths).unwrap();
    let profile = store
        .profiles
        .iter_mut()
        .find(|profile| profile.id == "codex-broken")
        .unwrap();
    profile.identity.insert(
        "account_id".to_string(),
        serde_json::Value::String("acct-wrong".to_string()),
    );
    store.save(&paths).unwrap();

    write_codex_oauth(codex_home.path(), "acct-live", "refresh-live");
    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args(["detach", "codex"])
        .assert()
        .success();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args(["use", "codex-broken", "--yes"])
        .assert()
        .failure()
        .stderr(contains("IdentityMismatch"));

    let auth = fs::read_to_string(codex_home.path().join("auth.json")).unwrap();
    assert!(auth.contains("acct-live"));
    assert!(auth.contains("refresh-live"));
    assert!(!auth.contains("refresh-a"));
    assert!(!switch_home
        .path()
        .join("state")
        .join("pending-switch")
        .join("codex.json")
        .exists());
    let history =
        fs::read_to_string(switch_home.path().join("state").join("history.jsonl")).unwrap();
    assert!(history.contains("\"to_profile\":\"codex-broken\""));
    assert!(history.contains("\"rolled_back\":true"));
    assert!(history.contains("\"ok\":false"));
}

#[test]
fn import_current_respects_target_locks() {
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
    fs::write(
        codex_home.path().join("auth.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "auth_mode": "apikey",
            "OPENAI_API_KEY": "sk-import-lock"
        }))
        .unwrap(),
    )
    .unwrap();

    let paths = any_switch::paths::Paths {
        home: cwd.clone(),
        switch_home: switch_home.path().to_path_buf(),
    };
    paths.ensure_layout().unwrap();
    let target_id = format!("file:{}", codex_home.path().join("auth.json").display());
    let _target_lock =
        any_switch::lock::FileLock::acquire(any_switch::lock::target_lock(&paths, &target_id))
            .unwrap();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .env("ANY_SWITCH_LOCK_WAIT_MS", "1")
        .args([
            "import-current",
            "codex",
            "locked",
            "--kind",
            "file_template",
        ])
        .assert()
        .failure()
        .stderr(contains("LockBusy"));
}

#[test]
fn target_lock_waits_for_different_apps_pointing_to_same_file() {
    let cwd = std::env::current_dir().unwrap();
    let switch_home = tempfile::Builder::new()
        .prefix(".test-switch-")
        .tempdir_in(&cwd)
        .unwrap();
    let target_dir = tempfile::Builder::new()
        .prefix(".test-shared-target-")
        .tempdir_in(&cwd)
        .unwrap();
    let shared_target = target_dir.path().join("profile.conf");
    let apps_dir = switch_home.path().join("apps.d");
    fs::create_dir_all(&apps_dir).unwrap();
    for app in ["alpha", "beta"] {
        fs::write(
            apps_dir.join(format!("{app}.yaml")),
            format!(
                r#"
schema_version: 1
app:
  id: {app}
  display_name: {app}
  definition_version: 1
kinds:
  file_template:
    field_schema:
      token:
        type: string
        required: true
        sensitive: true
    targets:
      - handler: file_capture
        path: {}
        template: |
          app={app}
          token={{{{ fields.token }}}}
"#,
                shared_target.display()
            ),
        )
        .unwrap();
    }

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("ALPHA_TOKEN", "alpha-token")
        .args([
            "add",
            "alpha",
            "one",
            "--kind",
            "file_template",
            "--secret-field",
            "token=@env:ALPHA_TOKEN",
        ])
        .assert()
        .success();
    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("BETA_TOKEN", "beta-token")
        .args([
            "add",
            "beta",
            "one",
            "--kind",
            "file_template",
            "--secret-field",
            "token=@env:BETA_TOKEN",
        ])
        .assert()
        .success();
    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args(["use", "alpha-one", "--yes"])
        .assert()
        .success();

    let paths = any_switch::paths::Paths {
        home: cwd.clone(),
        switch_home: switch_home.path().to_path_buf(),
    };
    paths.ensure_layout().unwrap();
    let target_id = format!("file:{}", shared_target.display());
    let target_lock =
        any_switch::lock::FileLock::acquire(any_switch::lock::target_lock(&paths, &target_id))
            .unwrap();
    let binary = assert_cmd::cargo::cargo_bin("any-switch");
    let mut beta_child = std::process::Command::new(&binary)
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .arg("use")
        .arg("beta-one")
        .arg("--yes")
        .spawn()
        .unwrap();

    std::thread::sleep(std::time::Duration::from_millis(100));
    assert!(beta_child.try_wait().unwrap().is_none());
    drop(target_lock);

    assert!(beta_child.wait().unwrap().success());
    let rendered = fs::read_to_string(&shared_target).unwrap();
    assert!(rendered.contains("app=beta"));
    assert!(rendered.contains("token=beta-token"));

    let active: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(switch_home.path().join("state/active.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(active["active_profiles"]["alpha"]["id"], "alpha-one");
    assert_eq!(active["active_profiles"]["beta"]["id"], "beta-one");
}

#[test]
fn state_lock_waits_and_preserves_entries_for_concurrent_app_bookkeeping() {
    let cwd = std::env::current_dir().unwrap();
    let switch_home = tempfile::Builder::new()
        .prefix(".test-switch-")
        .tempdir_in(&cwd)
        .unwrap();
    let codex_home = tempfile::Builder::new()
        .prefix(".test-codex-")
        .tempdir_in(&cwd)
        .unwrap();
    let toy_target = tempfile::Builder::new()
        .prefix(".test-toy-")
        .tempdir_in(&cwd)
        .unwrap();
    let apps_dir = switch_home.path().join("apps.d");
    fs::create_dir_all(&apps_dir).unwrap();
    fs::write(
        apps_dir.join("toy.yaml"),
        format!(
            r#"
schema_version: 1
app:
  id: toy
  display_name: Toy
  definition_version: 1
kinds:
  env_injection:
    field_schema:
      value:
        type: string
        required: true
    targets:
      - handler: json_env_merge
        path: {}
        json_path: $.env
        managed_keys: [TOY_VALUE]
        mapping:
          TOY_VALUE: "{{{{ fields.value }}}}"
"#,
            toy_target.path().join("settings.json").display()
        ),
    )
    .unwrap();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("TEST_API_KEY", "sk-state-lock")
        .args([
            "add",
            "codex",
            "state",
            "--kind",
            "file_template",
            "--secret-field",
            "api_key=@env:TEST_API_KEY",
        ])
        .assert()
        .success();
    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .args([
            "add",
            "toy",
            "state",
            "--kind",
            "env_injection",
            "--field",
            "value=one",
        ])
        .assert()
        .success();
    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args(["use", "codex-state", "--yes"])
        .assert()
        .success();
    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args(["use", "toy-state", "--yes"])
        .assert()
        .success();

    let paths = any_switch::paths::Paths {
        home: cwd.clone(),
        switch_home: switch_home.path().to_path_buf(),
    };
    paths.ensure_layout().unwrap();
    let state_lock =
        any_switch::lock::FileLock::acquire(any_switch::lock::state_lock(&paths)).unwrap();
    let binary = assert_cmd::cargo::cargo_bin("any-switch");
    let mut codex_child = std::process::Command::new(&binary)
        .env("ANY_SWITCH_HOME", switch_home.path())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .arg("detach")
        .arg("codex")
        .spawn()
        .unwrap();
    let mut toy_child = std::process::Command::new(&binary)
        .env("ANY_SWITCH_HOME", switch_home.path())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .arg("detach")
        .arg("toy")
        .spawn()
        .unwrap();

    std::thread::sleep(std::time::Duration::from_millis(100));
    assert!(codex_child.try_wait().unwrap().is_none());
    assert!(toy_child.try_wait().unwrap().is_none());
    drop(state_lock);

    assert!(codex_child.wait().unwrap().success());
    assert!(toy_child.wait().unwrap().success());

    let active: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(switch_home.path().join("state/active.json")).unwrap(),
    )
    .unwrap();
    assert!(active["active_profiles"]["codex"].is_null());
    assert!(active["active_profiles"]["toy"].is_null());

    let history =
        fs::read_to_string(switch_home.path().join("state").join("history.jsonl")).unwrap();
    let mut operation_ids = std::collections::BTreeSet::new();
    let detach_count = history
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .filter(|entry| entry["operation"] == "detach")
        .inspect(|entry| {
            operation_ids.insert(entry["operation_id"].as_str().unwrap().to_string());
        })
        .count();
    assert_eq!(detach_count, 2);
    assert_eq!(operation_ids.len(), 2);
}

#[test]
fn remove_deletes_profile_capture_and_clears_active_without_touching_live_target() {
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
            "remove-me",
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
        .args(["use", "codex-remove-me", "--yes"])
        .assert()
        .success();

    let live_before = fs::read_to_string(codex_home.path().join("auth.json")).unwrap();
    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .args(["remove", "codex-remove-me", "--yes"])
        .assert()
        .success()
        .stdout(contains("removed codex-remove-me"));

    let profiles = fs::read_to_string(switch_home.path().join("profiles.yaml")).unwrap();
    assert!(!profiles.contains("id: codex-remove-me"));
    assert!(!switch_home
        .path()
        .join("captures")
        .join("codex-remove-me")
        .exists());
    let active: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(switch_home.path().join("state/active.json")).unwrap(),
    )
    .unwrap();
    assert!(active["active_profiles"]["codex"].is_null());
    assert_eq!(
        fs::read_to_string(codex_home.path().join("auth.json")).unwrap(),
        live_before
    );
    let history =
        fs::read_to_string(switch_home.path().join("state").join("history.jsonl")).unwrap();
    assert!(history.contains("\"operation\":\"remove\""));
    assert!(history.contains("\"profile\":\"codex-remove-me\""));
}

#[test]
fn remove_rejects_invalid_profile_id_without_deleting_outside_capture_dir() {
    let cwd = std::env::current_dir().unwrap();
    let switch_home = tempfile::Builder::new()
        .prefix(".test-switch-")
        .tempdir_in(&cwd)
        .unwrap();
    fs::create_dir_all(switch_home.path().join("escape")).unwrap();
    fs::write(switch_home.path().join("escape").join("sentinel"), "keep").unwrap();
    fs::create_dir_all(switch_home.path().join("captures")).unwrap();
    fs::write(
        switch_home.path().join("profiles.yaml"),
        r#"
schema_version: 1
profiles:
  - id: ../escape
    app: codex
    kind: file_template
    schema_version: 1
    name: escape
    created_at: "2026-05-25T00:00:00Z"
    fields:
      api_key: sk-existing
      model: gpt-5-codex
"#,
    )
    .unwrap();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("ANY_SWITCH_TEST_HOME", &cwd)
        .args(["remove", "../escape", "--yes"])
        .assert()
        .failure()
        .stderr(contains("ProfileInvalid"));

    assert!(switch_home.path().join("escape").join("sentinel").exists());
}

#[test]
fn remove_accepts_force_as_confirmation_alias() {
    let switch_home = tempdir().unwrap();
    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("ANY_SWITCH_TEST_HOME", switch_home.path().parent().unwrap())
        .env("TEST_API_KEY", "sk-test")
        .args([
            "add",
            "codex",
            "force-remove",
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
        .env("ANY_SWITCH_TEST_HOME", switch_home.path().parent().unwrap())
        .args(["remove", "codex-force-remove", "--force"])
        .assert()
        .success()
        .stdout(contains("removed codex-force-remove"));

    let profiles = fs::read_to_string(switch_home.path().join("profiles.yaml")).unwrap();
    assert!(!profiles.contains("id: codex-force-remove"));
}

#[test]
fn detach_clears_active_without_touching_profile_capture_backup_or_live_target() {
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
            "detach-me",
            "--kind",
            "oauth_capture",
        ])
        .assert()
        .success();
    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("TEST_API_KEY", "sk-detach")
        .args([
            "add",
            "codex",
            "detach-static",
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
        .args([
            "use",
            "codex-detach-static",
            "--yes",
            "--accept-resolved-change",
        ])
        .assert()
        .success();

    let profiles_before = fs::read_to_string(switch_home.path().join("profiles.yaml")).unwrap();
    let capture_manifest = switch_home
        .path()
        .join("captures")
        .join("codex-detach-me")
        .join("manifest.json");
    let capture_before = fs::read_to_string(&capture_manifest).unwrap();
    let backup_ids_before = list_backup_ids(switch_home.path(), "codex");
    assert!(!backup_ids_before.is_empty());
    let live_before = fs::read_to_string(codex_home.path().join("auth.json")).unwrap();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .args(["detach", "codex"])
        .assert()
        .success()
        .stdout(contains("codex is now detached"))
        .stdout(contains("import-current"))
        .stdout(contains("not `any-switch use"));

    assert_eq!(
        fs::read_to_string(switch_home.path().join("profiles.yaml")).unwrap(),
        profiles_before
    );
    assert_eq!(
        fs::read_to_string(&capture_manifest).unwrap(),
        capture_before
    );
    assert_eq!(
        list_backup_ids(switch_home.path(), "codex"),
        backup_ids_before
    );
    assert_eq!(
        fs::read_to_string(codex_home.path().join("auth.json")).unwrap(),
        live_before
    );
    let active: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(switch_home.path().join("state/active.json")).unwrap(),
    )
    .unwrap();
    assert!(active["active_profiles"]["codex"].is_null());
    let history =
        fs::read_to_string(switch_home.path().join("state").join("history.jsonl")).unwrap();
    let detach_entry = history
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .find(|entry| entry["operation"] == "detach" && entry["app"] == "codex")
        .expect("detach history entry");
    assert_eq!(detach_entry["from_profile"], "codex-detach-static");
    assert_eq!(detach_entry["ok"], true);
}

#[test]
fn detach_rejects_invalid_app_id_before_creating_lock_path() {
    let cwd = std::env::current_dir().unwrap();
    let switch_home = tempfile::Builder::new()
        .prefix(".test-switch-")
        .tempdir_in(&cwd)
        .unwrap();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("ANY_SWITCH_TEST_HOME", &cwd)
        .args(["detach", "../escape"])
        .assert()
        .failure()
        .stderr(contains("invalid app id"));

    assert!(!switch_home.path().join("escape.lock").exists());
}

#[test]
fn detach_rejects_unknown_app_without_writing_active_state() {
    let switch_home = tempdir().unwrap();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("ANY_SWITCH_TEST_HOME", switch_home.path().parent().unwrap())
        .args(["detach", "doesnotexist"])
        .assert()
        .failure()
        .stderr(contains("AppNotFound: doesnotexist"));

    let active_path = switch_home.path().join("state").join("active.json");
    if active_path.exists() {
        let active: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(active_path).unwrap()).unwrap();
        assert!(active["active_profiles"]["doesnotexist"].is_null());
        assert!(!active["active_profiles"]
            .as_object()
            .unwrap()
            .contains_key("doesnotexist"));
    }
}

#[test]
fn remove_does_not_delete_profile_or_capture_when_app_is_locked() {
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
            "locked-remove",
            "--kind",
            "oauth_capture",
        ])
        .assert()
        .success();

    let paths = any_switch::paths::Paths {
        home: cwd,
        switch_home: switch_home.path().to_path_buf(),
    };
    paths.ensure_layout().unwrap();
    let _app_lock =
        any_switch::lock::FileLock::acquire(any_switch::lock::app_lock(&paths, "codex").unwrap())
            .unwrap();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .args(["remove", "codex-locked-remove", "--yes"])
        .assert()
        .failure()
        .stderr(contains("LockBusy"));

    let profiles = fs::read_to_string(switch_home.path().join("profiles.yaml")).unwrap();
    assert!(profiles.contains("id: codex-locked-remove"));
    assert!(switch_home
        .path()
        .join("captures")
        .join("codex-locked-remove")
        .join("auth.json")
        .exists());
    assert!(switch_home
        .path()
        .join("captures")
        .join("codex-locked-remove")
        .join("manifest.json")
        .exists());
}

#[test]
fn import_current_marks_profile_active_with_resolved_targets() {
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
    fs::write(
        codex_home.path().join("auth.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "auth_mode": "apikey",
            "OPENAI_API_KEY": "sk-import-active"
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        codex_home.path().join("config.toml"),
        "model = \"gpt-5-codex\"\nmodel_provider = \"openai\"\n",
    )
    .unwrap();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args([
            "import-current",
            "codex",
            "active",
            "--kind",
            "file_template",
        ])
        .assert()
        .success()
        .stdout(contains("imported codex-active"));

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .args(["status", "codex"])
        .assert()
        .success()
        .stdout(contains("codex\tmatched\tcodex-active"));

    let active_state =
        fs::read_to_string(switch_home.path().join("state").join("active.json")).unwrap();
    assert!(active_state.contains("\"id\": \"codex-active\""));
    assert!(active_state.contains(&codex_home.path().join("auth.json").display().to_string()));
}

#[test]
fn restore_claude_oauth_backup_restores_json_subtrees_not_whole_file() {
    let cwd = std::env::current_dir().unwrap();
    let home = tempfile::Builder::new()
        .prefix(".test-home-")
        .tempdir_in(&cwd)
        .unwrap();
    let switch_home = home.path().join(".any-switch");
    let claude_dir = home.path().join(".claude");
    fs::create_dir_all(&claude_dir).unwrap();
    fs::write(
        claude_dir.join(".credentials.json"),
        br#"{"refreshToken":"refresh-a"}"#,
    )
    .unwrap();
    write_claude_json(home.path(), "acct-a", "org-a", "user-a", "original");

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("HOME", home.path())
        .env("ANY_SWITCH_TEST_HOME", home.path())
        .env("ANY_SWITCH_HOME", &switch_home)
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args([
            "import-current",
            "claude",
            "work",
            "--kind",
            "oauth_capture",
        ])
        .assert()
        .success();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("HOME", home.path())
        .env("ANY_SWITCH_TEST_HOME", home.path())
        .env("ANY_SWITCH_HOME", &switch_home)
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args(["detach", "claude"])
        .assert()
        .success();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("HOME", home.path())
        .env("ANY_SWITCH_TEST_HOME", home.path())
        .env("ANY_SWITCH_HOME", &switch_home)
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args(["use", "claude-work", "--yes"])
        .assert()
        .success();

    let backup_id = list_backup_ids(&switch_home, "claude")
        .pop()
        .expect("backup id");
    let manifest = fs::read_to_string(
        switch_home
            .join("backups")
            .join("claude")
            .join(&backup_id)
            .join("manifest.json"),
    )
    .unwrap();
    assert!(manifest.contains("\"type\": \"json_subtree\""));
    assert!(manifest.contains("\"json_path\": \"$.oauthAccount\""));

    fs::write(
        home.path().join(".claude.json"),
        br#"{"before":"keep","oauthAccount":{"accountUuid":"acct-b","organizationUuid":"org-b","organizationName":"Changed","emailAddress":"changed@example.test"},"between":{"nested":true},"userID":"user-b","after":"keep"}"#,
    )
    .unwrap();
    Command::cargo_bin("any-switch")
        .unwrap()
        .env("HOME", home.path())
        .env("ANY_SWITCH_TEST_HOME", home.path())
        .env("ANY_SWITCH_HOME", &switch_home)
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args(["restore-target", "claude", &backup_id, "--yes"])
        .assert()
        .success();

    let restored_text = fs::read_to_string(home.path().join(".claude.json")).unwrap();
    assert!(!restored_text.contains('\n'));
    assert!(restored_text.starts_with(r#"{"before":"keep","oauthAccount":"#));
    assert!(restored_text.contains(r#","between":{"nested":true},"userID":"#));
    assert!(restored_text.ends_with(r#","after":"keep"}"#));
    let restored: serde_json::Value = serde_json::from_str(&restored_text).unwrap();
    assert_eq!(restored["oauthAccount"]["accountUuid"], "acct-a");
    assert_eq!(restored["oauthAccount"]["organizationUuid"], "org-a");
    assert_eq!(restored["userID"], "user-b");
    assert_eq!(restored["before"], "keep");
    assert_eq!(restored["between"]["nested"], true);
    assert_eq!(restored["after"], "keep");
}

#[test]
fn claude_oauth_use_clears_managed_settings_env_keys() {
    let cwd = std::env::current_dir().unwrap();
    let home = tempfile::Builder::new()
        .prefix(".test-home-")
        .tempdir_in(&cwd)
        .unwrap();
    let switch_home = home.path().join(".any-switch");
    let claude_dir = home.path().join(".claude");
    fs::create_dir_all(&claude_dir).unwrap();
    write_claude_credentials(home.path(), "acct-a", "org-a", "refresh-a");
    write_claude_json(home.path(), "acct-a", "org-a", "user-a", "original");
    fs::write(
        claude_dir.join("settings.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "env": {
                "KEEP": "1",
                "ANTHROPIC_AUTH_TOKEN": "old-token",
                "ANTHROPIC_MODEL": "old-model"
            },
            "other": true
        }))
        .unwrap(),
    )
    .unwrap();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("HOME", home.path())
        .env("ANY_SWITCH_TEST_HOME", home.path())
        .env("ANY_SWITCH_HOME", &switch_home)
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args([
            "import-current",
            "claude",
            "work",
            "--kind",
            "oauth_capture",
        ])
        .assert()
        .success();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("HOME", home.path())
        .env("ANY_SWITCH_TEST_HOME", home.path())
        .env("ANY_SWITCH_HOME", &switch_home)
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args(["detach", "claude"])
        .assert()
        .success();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("HOME", home.path())
        .env("ANY_SWITCH_TEST_HOME", home.path())
        .env("ANY_SWITCH_HOME", &switch_home)
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args(["use", "claude-work", "--yes"])
        .assert()
        .success();

    let settings: serde_json::Value =
        serde_json::from_slice(&fs::read(claude_dir.join("settings.json")).unwrap()).unwrap();
    assert_eq!(settings["env"]["KEEP"], "1");
    assert!(settings["env"].get("ANTHROPIC_AUTH_TOKEN").is_none());
    assert!(settings["env"].get("ANTHROPIC_MODEL").is_none());
    assert_eq!(settings["other"], true);

    let latest_backup = list_backup_ids(&switch_home, "claude")
        .pop()
        .expect("backup id");
    let manifest = fs::read_to_string(
        switch_home
            .join("backups")
            .join("claude")
            .join(latest_backup)
            .join("manifest.json"),
    )
    .unwrap();
    assert!(manifest.contains("settings.json"));

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("HOME", home.path())
        .env("ANY_SWITCH_TEST_HOME", home.path())
        .env("ANY_SWITCH_HOME", &switch_home)
        .args(["status", "claude"])
        .assert()
        .success()
        .stdout(contains("claude\tmatched\tclaude-work"));
}

#[test]
fn restore_target_rolls_back_and_clears_pending_when_apply_fails() {
    let cwd = std::env::current_dir().unwrap();
    let home = tempfile::Builder::new()
        .prefix(".test-home-")
        .tempdir_in(&cwd)
        .unwrap();
    let switch_home = home.path().join(".any-switch");
    write_claude_json(
        home.path(),
        "acct-live",
        "org-live",
        "user-live",
        "unchanged",
    );
    let target = home.path().join(".claude.json");
    let backup_id = "20260525T010000.000Z-invalid-json";
    let backup_dir = switch_home.join("backups").join("claude").join(backup_id);
    fs::create_dir_all(&backup_dir).unwrap();
    let blob = b"not-json";
    fs::write(backup_dir.join("target-0.bak"), blob).unwrap();
    fs::write(
        backup_dir.join("manifest.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": 1,
            "operation_id": "invalid-json-restore",
            "app": "claude",
            "created_at": "2026-05-25T01:00:00Z",
            "targets": [
                {
                    "target_id": format!("json:{}#$.oauthAccount", target.display()),
                    "type": "json_subtree",
                    "requires_app_stopped": false,
                    "path": target.display().to_string(),
                    "resolved_path": target.display().to_string(),
                    "json_path": "$.oauthAccount",
                    "stored_as": "target-0.bak",
                    "sha256": any_switch::backup::sha256_hex(blob)
                }
            ]
        }))
        .unwrap(),
    )
    .unwrap();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("HOME", home.path())
        .env("ANY_SWITCH_TEST_HOME", home.path())
        .env("ANY_SWITCH_HOME", &switch_home)
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args(["restore-target", "claude", backup_id, "--yes"])
        .assert()
        .failure();

    assert!(!switch_home
        .join("state")
        .join("pending-switch")
        .join("claude.json")
        .exists());
    let restored: serde_json::Value =
        serde_json::from_slice(&fs::read(home.path().join(".claude.json")).unwrap()).unwrap();
    assert_eq!(restored["oauthAccount"]["accountUuid"], "acct-live");
    assert_eq!(restored["unmanaged"], "unchanged");
    let history = fs::read_to_string(switch_home.join("state").join("history.jsonl")).unwrap();
    assert!(history.contains("\"operation\":\"restore-target\""));
    assert!(history.contains("\"rolled_back\":true"));
    assert!(history.contains("\"ok\":false"));
}

#[test]
fn claude_import_uses_oauth_account_identity_when_credentials_are_opaque() {
    let cwd = std::env::current_dir().unwrap();
    let home = tempfile::Builder::new()
        .prefix(".test-home-")
        .tempdir_in(&cwd)
        .unwrap();
    let switch_home = home.path().join(".any-switch");
    write_claude_credentials(home.path(), "acct-b", "org-b", "refresh-b");
    write_claude_json(home.path(), "acct-a", "org-a", "user-a", "original");

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("HOME", home.path())
        .env("ANY_SWITCH_TEST_HOME", home.path())
        .env("ANY_SWITCH_HOME", &switch_home)
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args([
            "import-current",
            "claude",
            "opaque",
            "--kind",
            "oauth_capture",
        ])
        .assert()
        .success()
        .stdout(contains("imported claude-opaque"));

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("HOME", home.path())
        .env("ANY_SWITCH_TEST_HOME", home.path())
        .env("ANY_SWITCH_HOME", &switch_home)
        .args(["show", "claude-opaque"])
        .assert()
        .success()
        .stdout(contains("account_uuid: acct-a"))
        .stdout(contains("organization_uuid: org-a"));
}

#[cfg(target_os = "macos")]
#[test]
fn claude_import_can_capture_macos_keychain_fixture() {
    let cwd = std::env::current_dir().unwrap();
    let home = tempfile::Builder::new()
        .prefix(".test-home-")
        .tempdir_in(&cwd)
        .unwrap();
    let switch_home = home.path().join(".any-switch");
    let keychain_fixture = home.path().join(".keychain-fixture");
    fs::create_dir_all(&keychain_fixture).unwrap();
    let account = current_test_os_user();
    let fixture_name =
        any_switch::backup::sha256_hex(format!("Claude Code-credentials\0{account}").as_bytes());
    fs::write(
        keychain_fixture.join(format!("{fixture_name}.secret")),
        serde_json::to_vec_pretty(&serde_json::json!({
            "claudeAiOauth": {
                "accessToken": claude_access_token("acct-a", "org-a"),
                "refreshToken": "refresh-keychain",
                "expiresAt": 1_800_000_000_000i64
            }
        }))
        .unwrap(),
    )
    .unwrap();
    write_claude_json(home.path(), "acct-a", "org-a", "user-a", "original");

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("HOME", home.path())
        .env("ANY_SWITCH_TEST_HOME", home.path())
        .env("ANY_SWITCH_HOME", &switch_home)
        .env("ANY_SWITCH_KEYCHAIN_FIXTURE_DIR", &keychain_fixture)
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args([
            "import-current",
            "claude",
            "keychain",
            "--kind",
            "oauth_capture",
        ])
        .assert()
        .success()
        .stdout(contains("imported claude-keychain"));

    let keychain_capture =
        fs::read_to_string(switch_home.join("captures/claude-keychain/keychain.json")).unwrap();
    assert!(keychain_capture.contains("refresh-keychain"));
    let profile_text = fs::read_to_string(switch_home.join("profiles.yaml")).unwrap();
    assert!(profile_text.contains("stored_as: keychain.json"));
}

#[test]
fn claude_import_uses_claude_config_dir_for_file_backed_credentials() {
    let cwd = std::env::current_dir().unwrap();
    let home = tempfile::Builder::new()
        .prefix(".test-home-")
        .tempdir_in(&cwd)
        .unwrap();
    let switch_home = home.path().join(".any-switch");
    let config_dir = home.path().join("custom-claude-config");
    write_claude_credentials_file(&config_dir, "acct-a", "org-a", "refresh-config-dir");
    write_claude_json(home.path(), "acct-a", "org-a", "user-a", "original");

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("HOME", home.path())
        .env("ANY_SWITCH_TEST_HOME", home.path())
        .env("ANY_SWITCH_HOME", &switch_home)
        .env("CLAUDE_CONFIG_DIR", &config_dir)
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args([
            "import-current",
            "claude",
            "config-dir",
            "--kind",
            "oauth_capture",
        ])
        .assert()
        .success()
        .stdout(contains("imported claude-config-dir"));

    let credentials_capture = fs::read_to_string(
        switch_home
            .join("captures")
            .join("claude-config-dir")
            .join("credentials.json"),
    )
    .unwrap();
    assert!(credentials_capture.contains("refresh-config-dir"));
    assert!(!home.path().join(".claude/.credentials.json").exists());
}

#[test]
fn claude_status_and_writeback_detect_oauth_account_identity_mismatch() {
    let cwd = std::env::current_dir().unwrap();
    let home = tempfile::Builder::new()
        .prefix(".test-home-")
        .tempdir_in(&cwd)
        .unwrap();
    let switch_home = home.path().join(".any-switch");
    write_claude_credentials(home.path(), "acct-a", "org-a", "refresh-a");
    write_claude_json(home.path(), "acct-a", "org-a", "user-a", "original");

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("HOME", home.path())
        .env("ANY_SWITCH_TEST_HOME", home.path())
        .env("ANY_SWITCH_HOME", &switch_home)
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args([
            "import-current",
            "claude",
            "work",
            "--kind",
            "oauth_capture",
        ])
        .assert()
        .success();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("HOME", home.path())
        .env("ANY_SWITCH_TEST_HOME", home.path())
        .env("ANY_SWITCH_HOME", &switch_home)
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args(["use", "claude-work", "--yes"])
        .assert()
        .success();

    write_claude_json(home.path(), "acct-b", "org-b", "user-b", "changed");
    Command::cargo_bin("any-switch")
        .unwrap()
        .env("HOME", home.path())
        .env("ANY_SWITCH_TEST_HOME", home.path())
        .env("ANY_SWITCH_HOME", &switch_home)
        .args(["status", "claude"])
        .assert()
        .success()
        .stdout(contains("claude\tdrifted\tclaude-work"));

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("HOME", home.path())
        .env("ANY_SWITCH_TEST_HOME", home.path())
        .env("ANY_SWITCH_HOME", &switch_home)
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args(["doctor", "claude"])
        .assert()
        .success()
        .stdout(contains("identity_check\twarning"))
        .stdout(contains("restored identity does not match"))
        .stdout(is_match("refresh-a").unwrap().not());

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("HOME", home.path())
        .env("ANY_SWITCH_TEST_HOME", home.path())
        .env("ANY_SWITCH_HOME", &switch_home)
        .env("PROXY_TOKEN", "proxy-secret")
        .args([
            "add",
            "claude",
            "proxy",
            "--kind",
            "env_injection",
            "--field",
            "base_url=https://example.test",
            "--secret-field",
            "auth_token=@env:PROXY_TOKEN",
        ])
        .assert()
        .success();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("HOME", home.path())
        .env("ANY_SWITCH_TEST_HOME", home.path())
        .env("ANY_SWITCH_HOME", &switch_home)
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args(["use", "claude-proxy", "--yes", "--accept-resolved-change"])
        .assert()
        .failure()
        .stderr(contains("DriftBeforeWriteback"));
}

#[test]
fn restore_rejects_backup_with_hash_mismatch() {
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
            "hash",
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
        .args(["use", "codex-hash", "--yes"])
        .assert()
        .success();

    let backup_id = list_backup_ids(switch_home.path(), "codex")
        .pop()
        .expect("backup id");
    fs::write(
        switch_home
            .path()
            .join("backups")
            .join("codex")
            .join(&backup_id)
            .join("target-0.bak"),
        b"tampered",
    )
    .unwrap();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args(["restore-target", "codex", &backup_id, "--yes"])
        .assert()
        .failure()
        .stderr(contains("BackupInvalid"))
        .stderr(contains("hash mismatch"));

    let auth = fs::read_to_string(codex_home.path().join("auth.json")).unwrap();
    assert!(auth.contains("sk-test"));
}

#[test]
fn use_oauth_rejects_missing_capture_before_pending_or_backup() {
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
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args(["use", "codex-broken", "--yes"])
        .assert()
        .failure()
        .stderr(contains("CaptureMissing"))
        .stderr(contains("import-current"));

    assert!(!switch_home.path().join("backups").join("codex").exists());
    assert!(!switch_home
        .path()
        .join("state")
        .join("pending-switch")
        .join("codex.json")
        .exists());
}

#[test]
fn pending_switch_blocks_writes_and_status_reports_interrupted() {
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
            "blocked",
            "--kind",
            "file_template",
            "--field",
            "model=gpt-5-codex",
            "--secret-field",
            "api_key=@env:TEST_API_KEY",
        ])
        .assert()
        .success();

    let pending_dir = switch_home.path().join("state").join("pending-switch");
    fs::create_dir_all(&pending_dir).unwrap();
    fs::write(
        pending_dir.join("codex.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": 1,
            "operation": "use",
            "operation_id": "test-op",
            "app": "codex",
            "to_profile": "codex-blocked",
            "backup_id": "backup-before-crash",
            "targets": [],
            "stage": "applying"
        }))
        .unwrap(),
    )
    .unwrap();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .args(["status", "codex"])
        .assert()
        .success()
        .stdout(contains("codex\tinterrupted"));

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .args(["use", "codex-blocked", "--yes"])
        .assert()
        .failure()
        .stderr(contains("InterruptedSwitch"));
}

#[test]
fn add_force_refuses_to_replace_profile_when_app_has_pending_switch() {
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
        .env("FIRST_API_KEY", "sk-first")
        .args([
            "add",
            "codex",
            "blocked",
            "--kind",
            "file_template",
            "--field",
            "model=gpt-5-codex",
            "--secret-field",
            "api_key=@env:FIRST_API_KEY",
        ])
        .assert()
        .success();

    let pending_dir = switch_home.path().join("state").join("pending-switch");
    fs::create_dir_all(&pending_dir).unwrap();
    fs::write(
        pending_dir.join("codex.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": 1,
            "operation": "use",
            "operation_id": "test-op",
            "app": "codex",
            "to_profile": "codex-blocked",
            "backup_id": "backup-before-crash",
            "targets": [],
            "stage": "applying"
        }))
        .unwrap(),
    )
    .unwrap();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("SECOND_API_KEY", "sk-second")
        .args([
            "add",
            "codex",
            "blocked",
            "--kind",
            "file_template",
            "--field",
            "model=o3",
            "--secret-field",
            "api_key=@env:SECOND_API_KEY",
            "--force",
        ])
        .assert()
        .failure()
        .stderr(contains("InterruptedSwitch"));

    let profiles = fs::read_to_string(switch_home.path().join("profiles.yaml")).unwrap();
    assert!(profiles.contains("sk-first"));
    assert!(profiles.contains("gpt-5-codex"));
    assert!(!profiles.contains("sk-second"));
    assert!(!profiles.contains("model: o3"));
}

#[test]
fn add_force_does_not_replace_profile_when_app_is_locked() {
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
        .env("FIRST_API_KEY", "sk-first")
        .args([
            "add",
            "codex",
            "locked-force",
            "--kind",
            "file_template",
            "--field",
            "model=gpt-5-codex",
            "--secret-field",
            "api_key=@env:FIRST_API_KEY",
        ])
        .assert()
        .success();

    let paths = any_switch::paths::Paths {
        home: cwd,
        switch_home: switch_home.path().to_path_buf(),
    };
    paths.ensure_layout().unwrap();
    let _app_lock =
        any_switch::lock::FileLock::acquire(any_switch::lock::app_lock(&paths, "codex").unwrap())
            .unwrap();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("SECOND_API_KEY", "sk-second")
        .args([
            "add",
            "codex",
            "locked-force",
            "--kind",
            "file_template",
            "--field",
            "model=o3",
            "--secret-field",
            "api_key=@env:SECOND_API_KEY",
            "--force",
        ])
        .assert()
        .failure()
        .stderr(contains("LockBusy"));

    let profiles = fs::read_to_string(switch_home.path().join("profiles.yaml")).unwrap();
    assert!(profiles.contains("sk-first"));
    assert!(profiles.contains("gpt-5-codex"));
    assert!(!profiles.contains("sk-second"));
    assert!(!profiles.contains("model: o3"));
}

#[test]
fn pending_applying_with_backup_rolls_back_before_next_write() {
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

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
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

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args(["use", "codex-first", "--yes"])
        .assert()
        .success();
    let backup_id = list_backup_ids(switch_home.path(), "codex")
        .pop()
        .expect("backup id");
    fs::write(codex_home.path().join("auth.json"), b"{partial").unwrap();

    let pending_dir = switch_home.path().join("state").join("pending-switch");
    fs::create_dir_all(&pending_dir).unwrap();
    let auth_path = codex_home.path().join("auth.json");
    fs::write(
        pending_dir.join("codex.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": 1,
            "operation": "use",
            "operation_id": "recover-applying-op",
            "app": "codex",
            "from_profile": "codex-first",
            "to_profile": "codex-second",
            "backup_id": backup_id,
            "targets": [{
                "target_id": format!("file:{}", auth_path.display()),
                "resolved_path": auth_path.display().to_string()
            }],
            "stage": "applying",
            "expected": {"kind": "file_template", "profile": "codex-second"}
        }))
        .unwrap(),
    )
    .unwrap();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args(["use", "codex-second", "--yes"])
        .assert()
        .success()
        .stdout(contains("switched codex to codex-second"));

    assert!(!pending_dir.join("codex.json").exists());
    let auth = fs::read_to_string(codex_home.path().join("auth.json")).unwrap();
    assert!(auth.contains("sk-second"));
    assert!(!auth.contains("partial"));
    let history =
        fs::read_to_string(switch_home.path().join("state").join("history.jsonl")).unwrap();
    assert!(history.contains("\"operation_id\":\"recover-applying-op\""));
    assert!(history.contains("\"rolled_back\":true"));
}

#[test]
fn pending_use_applying_commits_when_live_matches_target_profile() {
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

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
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

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args(["use", "codex-first", "--yes"])
        .assert()
        .success();

    let paths = any_switch::paths::Paths {
        home: cwd,
        switch_home: switch_home.path().to_path_buf(),
    };
    let auth_path = codex_home.path().join("auth.json");
    let config_path = codex_home.path().join("config.toml");
    let rollback_id = any_switch::backup::create_backup(
        &paths,
        "codex",
        &[auth_path.clone(), config_path.clone()],
    )
    .unwrap();

    fs::write(
        &auth_path,
        serde_json::to_vec_pretty(&serde_json::json!({
            "auth_mode": "apikey",
            "OPENAI_API_KEY": "sk-second"
        }))
        .unwrap(),
    )
    .unwrap();

    let pending_dir = switch_home.path().join("state").join("pending-switch");
    fs::create_dir_all(&pending_dir).unwrap();
    fs::write(
        pending_dir.join("codex.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": 1,
            "operation": "use",
            "operation_id": "recover-use-applying-commit-op",
            "app": "codex",
            "from_profile": "codex-first",
            "to_profile": "codex-second",
            "backup_id": rollback_id,
            "targets": [
                {
                    "target_id": format!("file:{}", auth_path.display()),
                    "resolved_path": auth_path.display().to_string()
                },
                {
                    "target_id": format!("file:{}", config_path.display()),
                    "resolved_path": config_path.display().to_string()
                }
            ],
            "stage": "applying",
            "expected": {"kind": "file_template", "profile": "codex-second"}
        }))
        .unwrap(),
    )
    .unwrap();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args(["use", "codex-second", "--yes"])
        .assert()
        .success();

    assert!(!pending_dir.join("codex.json").exists());
    let auth = fs::read_to_string(&auth_path).unwrap();
    assert!(auth.contains("sk-second"));
    assert!(!auth.contains("sk-first"));
    let active = fs::read_to_string(switch_home.path().join("state").join("active.json")).unwrap();
    assert!(active.contains("codex-second"));
    let history =
        fs::read_to_string(switch_home.path().join("state").join("history.jsonl")).unwrap();
    assert!(history.contains("\"operation_id\":\"recover-use-applying-commit-op\""));
    assert!(history.contains("\"completed_after_apply\":true"));
    assert!(
        !history.contains("\"operation_id\":\"recover-use-applying-commit-op\",\"rolled_back\"")
    );
}

#[test]
fn process_probe_blocks_static_write_unless_allow_running() {
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
            "running",
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
        .env(
            "ANY_SWITCH_PROCESS_PROBE_FIXTURE",
            "4242\tSun May 24 20:00:00 2026\t/usr/local/bin/codex",
        )
        .args(["use", "codex-running", "--yes"])
        .assert()
        .failure()
        .stderr(contains("AppRunning"))
        .stderr(contains("pid=4242"))
        .stderr(contains("start_time=Sun May 24 20:00:00 2026"));

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env(
            "ANY_SWITCH_PROCESS_PROBE_FIXTURE",
            "4242\tSun May 24 20:00:00 2026\t/usr/local/bin/codex",
        )
        .args(["use", "codex-running", "--yes", "--allow-running"])
        .assert()
        .success();
}

#[test]
fn oauth_assume_app_stopped_requires_yes_and_can_escape_probe() {
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
        .env(
            "ANY_SWITCH_PROCESS_PROBE_FIXTURE",
            "4242\t/usr/local/bin/codex",
        )
        .args([
            "import-current",
            "codex",
            "a",
            "--kind",
            "oauth_capture",
            "--assume-app-stopped",
        ])
        .assert()
        .failure()
        .stderr(contains("--assume-app-stopped requires --yes"));

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env(
            "ANY_SWITCH_PROCESS_PROBE_FIXTURE",
            "4242\t/usr/local/bin/codex",
        )
        .args([
            "import-current",
            "codex",
            "a",
            "--kind",
            "oauth_capture",
            "--assume-app-stopped",
            "--yes",
        ])
        .assert()
        .success()
        .stdout(contains("imported codex-a"));

    let history =
        fs::read_to_string(switch_home.path().join("state").join("history.jsonl")).unwrap();
    assert!(history.contains("\"operation\":\"import-current\""));
    assert!(history.contains("\"type\":\"assume_app_stopped\""));
    assert!(history.contains("\"pid\":4242"));
    assert!(history.contains("/usr/local/bin/codex"));
}

#[test]
fn pending_bookkeeping_is_recovered_before_next_write() {
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
            "recovered",
            "--kind",
            "file_template",
            "--field",
            "model=gpt-5-codex",
            "--secret-field",
            "api_key=@env:TEST_API_KEY",
        ])
        .assert()
        .success();

    let pending_dir = switch_home.path().join("state").join("pending-switch");
    fs::create_dir_all(&pending_dir).unwrap();
    fs::write(
        pending_dir.join("codex.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": 1,
            "operation": "use",
            "operation_id": "recover-op",
            "app": "codex",
            "to_profile": "codex-recovered",
            "backup_id": "backup-before-crash",
            "targets": [{
                "target_id": "file:/tmp/auth.json",
                "resolved_path": "/tmp/auth.json"
            }],
            "stage": "bookkeeping"
        }))
        .unwrap(),
    )
    .unwrap();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args([
            "use",
            "codex-recovered",
            "--yes",
            "--accept-resolved-change",
        ])
        .assert()
        .success();

    assert!(!pending_dir.join("codex.json").exists());
    let history =
        fs::read_to_string(switch_home.path().join("state").join("history.jsonl")).unwrap();
    assert!(history.contains("\"operation_id\":\"recover-op\""));
    assert!(history.contains("\"recovered\":true"));
}

#[test]
fn restore_target_bookkeeping_recovery_does_not_update_active() {
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
        .env("FIRST_API_KEY", "sk-first")
        .args([
            "add",
            "codex",
            "active",
            "--kind",
            "file_template",
            "--field",
            "model=gpt-5-codex",
            "--secret-field",
            "api_key=@env:FIRST_API_KEY",
        ])
        .assert()
        .success();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("SECOND_API_KEY", "sk-second")
        .args([
            "add",
            "codex",
            "next",
            "--kind",
            "file_template",
            "--field",
            "model=gpt-5-codex",
            "--secret-field",
            "api_key=@env:SECOND_API_KEY",
        ])
        .assert()
        .success();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args(["use", "codex-active", "--yes"])
        .assert()
        .success();
    let backup_id = list_backup_ids(switch_home.path(), "codex")
        .pop()
        .expect("backup id");

    let pending_dir = switch_home.path().join("state").join("pending-switch");
    fs::create_dir_all(&pending_dir).unwrap();
    fs::write(
        pending_dir.join("codex.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": 1,
            "operation": "restore-target",
            "operation_id": "recover-restore-bookkeeping-op",
            "app": "codex",
            "backup_id": "rollback-before-restore",
            "restore_from_backup_id": backup_id,
            "targets": [],
            "stage": "bookkeeping",
            "expected": {"backup_id": backup_id}
        }))
        .unwrap(),
    )
    .unwrap();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args(["use", "codex-next", "--yes"])
        .assert()
        .success();

    assert!(!pending_dir.join("codex.json").exists());
    let active = fs::read_to_string(switch_home.path().join("state").join("active.json")).unwrap();
    assert!(active.contains("codex-next"));
    assert!(!active.contains("recover-restore-bookkeeping-op"));
    let history =
        fs::read_to_string(switch_home.path().join("state").join("history.jsonl")).unwrap();
    assert!(history.contains("\"operation_id\":\"recover-restore-bookkeeping-op\""));
    assert!(history.contains("\"operation\":\"restore-target\""));
    assert!(history.contains("\"recovered\":true"));
}

#[test]
fn restore_target_applying_recovery_commits_when_live_matches_restore_backup() {
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

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
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

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args(["use", "codex-first", "--yes"])
        .assert()
        .success();
    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args(["use", "codex-second", "--yes"])
        .assert()
        .success();

    let backup_id = list_backup_ids(switch_home.path(), "codex")
        .pop()
        .expect("backup id");
    let paths = any_switch::paths::Paths {
        home: cwd,
        switch_home: switch_home.path().to_path_buf(),
    };
    any_switch::backup::restore_backup(&paths, "codex", &backup_id).unwrap();
    let auth_path = codex_home.path().join("auth.json");
    assert!(fs::read_to_string(&auth_path).unwrap().contains("sk-first"));

    let pending_dir = switch_home.path().join("state").join("pending-switch");
    fs::create_dir_all(&pending_dir).unwrap();
    fs::write(
        pending_dir.join("codex.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": 1,
            "operation": "restore-target",
            "operation_id": "recover-restore-applying-op",
            "app": "codex",
            "backup_id": "missing-rollback-backup",
            "restore_from_backup_id": backup_id,
            "targets": [{
                "target_id": format!("file:{}", auth_path.display()),
                "resolved_path": auth_path.display().to_string()
            }],
            "stage": "applying",
            "expected": {"backup_id": backup_id}
        }))
        .unwrap(),
    )
    .unwrap();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args(["use", "codex-second", "--yes"])
        .assert()
        .success();

    assert!(!pending_dir.join("codex.json").exists());
    let history =
        fs::read_to_string(switch_home.path().join("state").join("history.jsonl")).unwrap();
    assert!(history.contains("\"operation_id\":\"recover-restore-applying-op\""));
    assert!(history.contains("\"completed_after_apply\":true"));
    assert!(!history.contains("\"rolled_back\":true"));
}

#[test]
fn resolved_target_change_reports_drift_and_requires_acceptance() {
    let cwd = std::env::current_dir().unwrap();
    let switch_home = tempfile::Builder::new()
        .prefix(".test-switch-")
        .tempdir_in(&cwd)
        .unwrap();
    let codex_home_a = tempfile::Builder::new()
        .prefix(".test-codex-a-")
        .tempdir_in(&cwd)
        .unwrap();
    let codex_home_b = tempfile::Builder::new()
        .prefix(".test-codex-b-")
        .tempdir_in(&cwd)
        .unwrap();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home_a.path())
        .env("TEST_API_KEY", "sk-test")
        .args([
            "add",
            "codex",
            "moving",
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
        .env("CODEX_HOME", codex_home_a.path())
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args(["use", "codex-moving", "--yes"])
        .assert()
        .success();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home_b.path())
        .args(["status", "codex"])
        .assert()
        .success()
        .stdout(contains("codex\tdrifted\tcodex-moving"))
        .stdout(contains("reason\tresolved_targets_changed"))
        .stdout(contains("old_target"))
        .stdout(contains(
            codex_home_a.path().join("auth.json").display().to_string(),
        ))
        .stdout(contains("new_target"))
        .stdout(contains(
            codex_home_b.path().join("auth.json").display().to_string(),
        ));

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home_b.path())
        .args(["status", "codex", "--json"])
        .assert()
        .success()
        .stdout(contains("\"status\": \"drifted\""))
        .stdout(contains("resolved_targets_changed"));

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home_b.path())
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args(["use", "codex-moving", "--yes"])
        .assert()
        .failure()
        .stderr(contains("ResolvedTargetChanged"));

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home_b.path())
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args(["use", "codex-moving", "--yes", "--accept-resolved-change"])
        .assert()
        .success();
}
