use assert_cmd::Command;
use predicates::str::contains;
use std::fs;
mod support;
use support::*;

#[test]
fn codex_oauth_import_use_and_writeback() {
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

    write_codex_oauth(codex_home.path(), "acct-a", "refresh-a");
    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args(["import-current", "codex", "a", "--kind", "oauth_capture"])
        .assert()
        .success()
        .stdout(contains("imported codex-a"));

    write_codex_oauth(codex_home.path(), "acct-b", "refresh-b");
    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args(["import-current", "codex", "b", "--kind", "oauth_capture"])
        .assert()
        .success()
        .stdout(contains("imported codex-b"));

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args(["use", "codex-a", "--yes"])
        .assert()
        .success()
        .stdout(contains("switched codex to codex-a"));

    write_codex_oauth(codex_home.path(), "acct-a", "refresh-a-rotated");
    let profiles_before_writeback =
        fs::read_to_string(switch_home.path().join("profiles.yaml")).unwrap();
    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args(["use", "codex-b", "--yes"])
        .assert()
        .success()
        .stdout(contains("switched codex to codex-b"));
    let profiles_after_writeback =
        fs::read_to_string(switch_home.path().join("profiles.yaml")).unwrap();
    assert_eq!(profiles_after_writeback, profiles_before_writeback);

    let capture_a = fs::read_to_string(
        switch_home
            .path()
            .join("captures")
            .join("codex-a")
            .join("auth.json"),
    )
    .unwrap();
    assert!(capture_a.contains("refresh-a-rotated"));
    let manifest_a = fs::read_to_string(
        switch_home
            .path()
            .join("captures")
            .join("codex-a")
            .join("manifest.json"),
    )
    .unwrap();
    assert!(manifest_a.contains("last_writeback_at"));

    let backups = list_backup_ids(switch_home.path(), "codex");
    assert!(!backups.is_empty());
    let latest_backup = backups.last().unwrap();
    let backup_manifest = fs::read_to_string(
        switch_home
            .path()
            .join("backups")
            .join("codex")
            .join(latest_backup)
            .join("manifest.json"),
    )
    .unwrap();
    assert!(backup_manifest.contains("\"requires_app_stopped\": true"));

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .args(["status", "codex"])
        .assert()
        .success()
        .stdout(contains("codex\tmatched\tcodex-b"));

    assert!(!switch_home
        .path()
        .join("state")
        .join("pending-switch")
        .join("codex.json")
        .exists());
}

#[test]
fn codex_auto_import_uses_chatgpt_auth_mode_even_when_config_has_model_fields() {
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
        r#"
model = "gpt-5-codex"
model_provider = "openai"
"#,
    )
    .unwrap();
    write_codex_oauth(codex_home.path(), "acct-a", "refresh-a");

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args(["import-current", "codex", "personal"])
        .assert()
        .success()
        .stdout(contains("imported codex-personal"));

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .args(["show", "codex-personal"])
        .assert()
        .success()
        .stdout(contains("kind: oauth_capture"));
}

#[test]
fn codex_import_accepts_legacy_api_key_without_mode_when_store_is_implicit() {
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
            "OPENAI_API_KEY": "sk-legacy-without-mode"
        }))
        .unwrap(),
    )
    .unwrap();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args(["import-current", "codex", "legacy"])
        .assert()
        .success()
        .stdout(contains("imported codex-legacy"));

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .args(["show", "codex-legacy"])
        .assert()
        .success()
        .stdout(contains("kind: file_template"));
}

#[test]
fn codex_import_accepts_legacy_api_key_without_mode_even_when_config_has_model_fields() {
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
            "OPENAI_API_KEY": "sk-legacy-without-mode"
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        codex_home.path().join("config.toml"),
        r#"
model = "gpt-5.4"
model_provider = "token"
"#,
    )
    .unwrap();

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args(["import-current", "codex", "legacy"])
        .assert()
        .success()
        .stdout(contains("imported codex-legacy"));

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .args(["show", "codex-legacy"])
        .assert()
        .success()
        .stdout(contains("kind: file_template"))
        .stdout(contains("model_provider: token"));
}

#[test]
fn codex_import_rejects_mixed_chatgpt_and_api_key_auth() {
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
            "OPENAI_API_KEY": "sk-should-not-coexist",
            "tokens": {
                "account_id": "acct-a",
                "id_token": "x.eyJzdWIiOiIxMjMifQ.y",
                "refresh_token": "refresh-a"
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
        .args(["import-current", "codex", "mixed"])
        .assert()
        .failure()
        .stderr(contains("ImportAmbiguous"))
        .stderr(contains("forbidden string $.tokens.id_token"));
}

#[test]
fn codex_oauth_toml_capture_only_restores_managed_paths() {
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
        r#"
model = "gpt-work"
model_provider = "openai"

[model_providers.openai]
base_url = "https://api.openai.com/v1"

[mcp_servers.keep]
command = "before"

[projects."/tmp/demo"]
trust_level = "trusted"
"#,
    )
    .unwrap();
    write_codex_oauth(codex_home.path(), "acct-a", "refresh-a");

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args(["import-current", "codex", "toml", "--kind", "oauth_capture"])
        .assert()
        .success();

    let fragment = fs::read_to_string(
        switch_home
            .path()
            .join("captures")
            .join("codex-toml")
            .join("config.managed.toml"),
    )
    .unwrap();
    assert!(fragment.contains("model = \"gpt-work\""));
    assert!(fragment.contains("[model_providers.openai]"));
    assert!(!fragment.contains("mcp_servers"));
    assert!(!fragment.contains("projects"));

    fs::write(
        codex_home.path().join("config.toml"),
        r#"
model = "temporary"
model_provider = "other"

[mcp_servers.keep]
command = "after"

[projects."/tmp/demo"]
trust_level = "changed"
"#,
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

    Command::cargo_bin("any-switch")
        .unwrap()
        .env("ANY_SWITCH_HOME", switch_home.path())
        .env("CODEX_HOME", codex_home.path())
        .env("ANY_SWITCH_SKIP_PROCESS_PROBE", "1")
        .args(["use", "codex-toml", "--yes"])
        .assert()
        .success();

    let config = fs::read_to_string(codex_home.path().join("config.toml")).unwrap();
    assert!(config.contains("model = \"gpt-work\""));
    assert!(config.contains("model_provider = \"openai\""));
    assert!(config.contains("[model_providers.openai]"));
    assert!(config.contains("command = \"after\""));
    assert!(config.contains("trust_level = \"changed\""));
    assert!(!config.contains("command = \"before\""));
}
