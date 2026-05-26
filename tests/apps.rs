use assert_cmd::Command;
use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;
use std::fs;
mod support;
use support::*;

#[test]
fn user_app_definition_can_drive_env_injection() {
    let cwd = std::env::current_dir().unwrap();
    let switch_home = tempfile::Builder::new()
        .prefix(".test-switch-")
        .tempdir_in(&cwd)
        .unwrap();
    let target_dir = tempfile::Builder::new()
        .prefix(".test-target-")
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
            target_dir.path().join("settings.json").display()
        ),
    )
    .unwrap();

    Command::cargo_bin("switch-cli")
        .unwrap()
        .env("SWITCH_CLI_HOME", switch_home.path())
        .args(["apps"])
        .assert()
        .success()
        .stdout(contains("toy"));

    Command::cargo_bin("switch-cli")
        .unwrap()
        .env("SWITCH_CLI_HOME", switch_home.path())
        .args([
            "add",
            "toy",
            "demo",
            "--kind",
            "env_injection",
            "--field",
            "value=hello",
        ])
        .assert()
        .success();

    Command::cargo_bin("switch-cli")
        .unwrap()
        .env("SWITCH_CLI_HOME", switch_home.path())
        .args(["use", "toy-demo", "--yes"])
        .assert()
        .success();

    let settings = fs::read_to_string(target_dir.path().join("settings.json")).unwrap();
    assert!(settings.contains("TOY_VALUE"));
    assert!(settings.contains("hello"));
}

#[test]
fn user_app_definition_can_drive_file_template() {
    let cwd = std::env::current_dir().unwrap();
    let switch_home = tempfile::Builder::new()
        .prefix(".test-switch-")
        .tempdir_in(&cwd)
        .unwrap();
    let target_dir = tempfile::Builder::new()
        .prefix(".test-target-")
        .tempdir_in(&cwd)
        .unwrap();
    let target_path = target_dir.path().join("profile.conf");
    fs::write(&target_path, "model=previous\n").unwrap();
    let apps_dir = switch_home.path().join("apps.d");
    fs::create_dir_all(&apps_dir).unwrap();
    fs::write(
        apps_dir.join("template-app.yaml"),
        format!(
            r#"
schema_version: 1
app:
  id: templater
  display_name: Templater
  definition_version: 1
kinds:
  file_template:
    field_schema:
      token:
        type: string
        required: true
        sensitive: true
      model:
        type: string
        required: true
    targets:
      - handler: file_capture
        path: {}
        template: |
          token={{{{ fields.token }}}}
          model={{{{ fields.model }}}}
"#,
            target_path.display()
        ),
    )
    .unwrap();

    Command::cargo_bin("switch-cli")
        .unwrap()
        .env("SWITCH_CLI_HOME", switch_home.path())
        .env("TEMPLATE_TOKEN", "secret-token")
        .args([
            "add",
            "templater",
            "demo",
            "--kind",
            "file_template",
            "--secret-field",
            "token=@env:TEMPLATE_TOKEN",
            "--field",
            "model=large",
        ])
        .assert()
        .success();

    let dry_run_output = Command::cargo_bin("switch-cli")
        .unwrap()
        .env("SWITCH_CLI_HOME", switch_home.path())
        .args(["use", "templater-demo", "--dry-run", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let dry_run_text = String::from_utf8(dry_run_output.clone()).unwrap();
    let dry_run_plan: serde_json::Value = serde_json::from_slice(&dry_run_output).unwrap();
    assert!(dry_run_text.contains("\"secrets\": \"***\""));
    assert!(!dry_run_text.contains("secret-token"));
    assert_eq!(dry_run_plan["defensive_backup"]["enabled"], true);
    assert_eq!(dry_run_plan["post_write_verify"]["type"], "static_targets");
    assert_eq!(
        dry_run_plan["post_write_verify"]["targets"][0]["handler"],
        "file_capture"
    );

    Command::cargo_bin("switch-cli")
        .unwrap()
        .env("SWITCH_CLI_HOME", switch_home.path())
        .env("SWITCH_CLI_SKIP_PROCESS_PROBE", "1")
        .args(["use", "templater-demo", "--yes"])
        .assert()
        .success();

    let rendered = fs::read_to_string(&target_path).unwrap();
    assert!(rendered.contains("token=secret-token"));
    assert!(rendered.contains("model=large"));

    Command::cargo_bin("switch-cli")
        .unwrap()
        .env("SWITCH_CLI_HOME", switch_home.path())
        .args(["status", "templater"])
        .assert()
        .success()
        .stdout(contains("templater\tmatched\ttemplater-demo"));

    Command::cargo_bin("switch-cli")
        .unwrap()
        .env("SWITCH_CLI_HOME", switch_home.path())
        .env("SWITCH_CLI_SKIP_PROCESS_PROBE", "1")
        .args(["doctor", "templater"])
        .assert()
        .success()
        .stdout(contains("app\ttemplater"))
        .stdout(contains("active_profile\ttemplater-demo"))
        .stdout(contains(
            "definition_target\tfile_template\tfile_capture\texists",
        ));

    let backup_ids = list_backup_ids(switch_home.path(), "templater");
    assert_eq!(backup_ids.len(), 1);

    Command::cargo_bin("switch-cli")
        .unwrap()
        .env("SWITCH_CLI_HOME", switch_home.path())
        .env("SWITCH_CLI_SKIP_PROCESS_PROBE", "1")
        .args(["backup", "list", "templater", "--json"])
        .assert()
        .success()
        .stdout(contains("\"app\": \"templater\""));

    Command::cargo_bin("switch-cli")
        .unwrap()
        .env("SWITCH_CLI_HOME", switch_home.path())
        .env("SWITCH_CLI_SKIP_PROCESS_PROBE", "1")
        .args(["restore-target", "templater", &backup_ids[0], "--yes"])
        .assert()
        .success();

    assert_eq!(
        fs::read_to_string(&target_path).unwrap(),
        "model=previous\n"
    );

    Command::cargo_bin("switch-cli")
        .unwrap()
        .env("SWITCH_CLI_HOME", switch_home.path())
        .args(["remove", "templater-demo", "--yes"])
        .assert()
        .success()
        .stdout(contains("removed templater-demo"));

    assert_eq!(
        fs::read_to_string(&target_path).unwrap(),
        "model=previous\n"
    );
}

#[test]
fn user_app_definition_can_drive_toml_managed_paths() {
    let cwd = std::env::current_dir().unwrap();
    let switch_home = tempfile::Builder::new()
        .prefix(".test-switch-")
        .tempdir_in(&cwd)
        .unwrap();
    let target_dir = tempfile::Builder::new()
        .prefix(".test-target-")
        .tempdir_in(&cwd)
        .unwrap();
    let apps_dir = switch_home.path().join("apps.d");
    fs::create_dir_all(&apps_dir).unwrap();
    let config_path = target_dir.path().join("config.toml");
    fs::write(
        &config_path,
        r#"
theme = "old"

[unmanaged]
keep = "yes"
"#,
    )
    .unwrap();
    fs::write(
        apps_dir.join("toml-app.yaml"),
        format!(
            r#"
schema_version: 1
app:
  id: tomlapp
  display_name: TOML App
  definition_version: 1
kinds:
  file_template:
    field_schema:
      theme:
        type: string
        required: true
      accent:
        type: string
        required: false
      nested:
        type: object
        fields:
          value:
            type: string
            required: true
    targets:
      - handler: toml_managed_paths
        path: {}
        toml_paths:
          - theme
          - accent
          - nested.value
"#,
            config_path.display()
        ),
    )
    .unwrap();

    Command::cargo_bin("switch-cli")
        .unwrap()
        .env("SWITCH_CLI_HOME", switch_home.path())
        .args([
            "add",
            "tomlapp",
            "demo",
            "--kind",
            "file_template",
            "--field",
            "theme=dark",
            "--field",
            "nested.value=managed",
        ])
        .assert()
        .success();

    Command::cargo_bin("switch-cli")
        .unwrap()
        .env("SWITCH_CLI_HOME", switch_home.path())
        .env("SWITCH_CLI_SKIP_PROCESS_PROBE", "1")
        .args(["use", "tomlapp-demo", "--yes"])
        .assert()
        .success();

    let doc = fs::read_to_string(&config_path)
        .unwrap()
        .parse::<toml_edit::DocumentMut>()
        .unwrap();
    assert_eq!(doc["theme"].as_str(), Some("dark"));
    assert_eq!(doc["nested"]["value"].as_str(), Some("managed"));
    assert_eq!(doc["unmanaged"]["keep"].as_str(), Some("yes"));

    Command::cargo_bin("switch-cli")
        .unwrap()
        .env("SWITCH_CLI_HOME", switch_home.path())
        .args(["status", "tomlapp"])
        .assert()
        .success()
        .stdout(contains("tomlapp\tmatched\ttomlapp-demo"));

    fs::write(
        &config_path,
        r#"
theme = "dark"

[nested]
value = "managed"

[unmanaged]
keep = "changed"
"#,
    )
    .unwrap();
    Command::cargo_bin("switch-cli")
        .unwrap()
        .env("SWITCH_CLI_HOME", switch_home.path())
        .args(["status", "tomlapp"])
        .assert()
        .success()
        .stdout(contains("tomlapp\tmatched\ttomlapp-demo"));

    fs::write(
        &config_path,
        r#"
theme = "dark"
accent = "stale"

[nested]
value = "managed"

[unmanaged]
keep = "changed"
"#,
    )
    .unwrap();
    Command::cargo_bin("switch-cli")
        .unwrap()
        .env("SWITCH_CLI_HOME", switch_home.path())
        .args(["status", "tomlapp"])
        .assert()
        .success()
        .stdout(contains("tomlapp\tdrifted\ttomlapp-demo"));
}

#[test]
fn user_app_definition_can_import_oauth_capture() {
    let cwd = std::env::current_dir().unwrap();
    let switch_home = tempfile::Builder::new()
        .prefix(".test-switch-")
        .tempdir_in(&cwd)
        .unwrap();
    let target_dir = tempfile::Builder::new()
        .prefix(".test-target-")
        .tempdir_in(&cwd)
        .unwrap();
    let apps_dir = switch_home.path().join("apps.d");
    fs::create_dir_all(&apps_dir).unwrap();
    let auth_path = target_dir.path().join("auth.json");
    fs::write(
        &auth_path,
        serde_json::to_vec_pretty(&serde_json::json!({
            "tenant": "tenant-a",
            "email": "a@example.test",
            "refresh_token": "refresh-a"
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        apps_dir.join("custom-oauth.yaml"),
        format!(
            r#"
schema_version: 1
app:
  id: customoauth
  display_name: Custom OAuth
  definition_version: 1
process_probe:
  names: [customoauth]
kinds:
  oauth_capture:
    identity:
      handler: json_paths
      fields:
        tenant:
          path: $.tenant
          verify: required
        email:
          path: $.email
          verify: optional
    targets:
      - handler: file_capture
        path: {}
        requires_app_stopped: true
"#,
            auth_path.display()
        ),
    )
    .unwrap();

    Command::cargo_bin("switch-cli")
        .unwrap()
        .env("SWITCH_CLI_HOME", switch_home.path())
        .env("SWITCH_CLI_SKIP_PROCESS_PROBE", "1")
        .args([
            "import-current",
            "customoauth",
            "work",
            "--kind",
            "oauth_capture",
        ])
        .assert()
        .success()
        .stdout(contains("imported customoauth-work"));

    let capture = fs::read_to_string(
        switch_home
            .path()
            .join("captures")
            .join("customoauth-work")
            .join("auth.json"),
    )
    .unwrap();
    assert!(capture.contains("refresh-a"));

    fs::write(
        &auth_path,
        serde_json::to_vec_pretty(&serde_json::json!({
            "tenant": "tenant-b",
            "email": "b@example.test",
            "refresh_token": "refresh-b"
        }))
        .unwrap(),
    )
    .unwrap();

    Command::cargo_bin("switch-cli")
        .unwrap()
        .env("SWITCH_CLI_HOME", switch_home.path())
        .env("SWITCH_CLI_SKIP_PROCESS_PROBE", "1")
        .args(["detach", "customoauth"])
        .assert()
        .success();

    Command::cargo_bin("switch-cli")
        .unwrap()
        .env("SWITCH_CLI_HOME", switch_home.path())
        .env("SWITCH_CLI_SKIP_PROCESS_PROBE", "1")
        .args(["use", "customoauth-work", "--yes"])
        .assert()
        .success()
        .stdout(contains("switched customoauth to customoauth-work"));

    let restored = fs::read_to_string(&auth_path).unwrap();
    assert!(restored.contains("tenant-a"));
    assert!(restored.contains("refresh-a"));
    assert!(!restored.contains("tenant-b"));
}

#[test]
fn user_app_definition_can_import_oauth_capture_from_json_subtree() {
    let cwd = std::env::current_dir().unwrap();
    let switch_home = tempfile::Builder::new()
        .prefix(".test-switch-")
        .tempdir_in(&cwd)
        .unwrap();
    let target_dir = tempfile::Builder::new()
        .prefix(".test-target-")
        .tempdir_in(&cwd)
        .unwrap();
    let apps_dir = switch_home.path().join("apps.d");
    fs::create_dir_all(&apps_dir).unwrap();
    let state_path = target_dir.path().join("state.json");
    fs::write(
        &state_path,
        serde_json::to_vec_pretty(&serde_json::json!({
            "auth": {
                "tenant": "tenant-a",
                "email": "a@example.test",
                "refresh_token": "refresh-a"
            },
            "unmanaged": "keep"
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        apps_dir.join("custom-json-oauth.yaml"),
        format!(
            r#"
schema_version: 1
app:
  id: customjsonoauth
  display_name: Custom JSON OAuth
  definition_version: 1
process_probe:
  names: [customjsonoauth]
kinds:
  oauth_capture:
    identity:
      handler: json_paths
      fields:
        tenant:
          path: $.auth.tenant
          verify: required
        email:
          path: $.auth.email
          verify: optional
    targets:
      - handler: json_subtree
        path: {}
        json_path: $.auth
        requires_app_stopped: true
"#,
            state_path.display()
        ),
    )
    .unwrap();
    Command::cargo_bin("switch-cli")
        .unwrap()
        .env("SWITCH_CLI_HOME", switch_home.path())
        .env("SWITCH_CLI_SKIP_PROCESS_PROBE", "1")
        .args([
            "import-current",
            "customjsonoauth",
            "work",
            "--kind",
            "oauth_capture",
        ])
        .assert()
        .success()
        .stdout(contains("imported customjsonoauth-work"));

    let capture = fs::read_to_string(
        switch_home
            .path()
            .join("captures")
            .join("customjsonoauth-work")
            .join("auth.json"),
    )
    .unwrap();
    assert!(capture.contains("refresh-a"));
    assert!(!capture.contains("unmanaged"));

    fs::write(
        &state_path,
        serde_json::to_vec_pretty(&serde_json::json!({
            "auth": {
                "tenant": "tenant-b",
                "email": "b@example.test",
                "refresh_token": "refresh-b"
            },
            "unmanaged": "keep"
        }))
        .unwrap(),
    )
    .unwrap();

    Command::cargo_bin("switch-cli")
        .unwrap()
        .env("SWITCH_CLI_HOME", switch_home.path())
        .env("SWITCH_CLI_SKIP_PROCESS_PROBE", "1")
        .args(["detach", "customjsonoauth"])
        .assert()
        .success();

    Command::cargo_bin("switch-cli")
        .unwrap()
        .env("SWITCH_CLI_HOME", switch_home.path())
        .env("SWITCH_CLI_SKIP_PROCESS_PROBE", "1")
        .args(["use", "customjsonoauth-work", "--yes"])
        .assert()
        .success()
        .stdout(contains("switched customjsonoauth to customjsonoauth-work"));

    let restored: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert_eq!(restored["auth"]["tenant"], "tenant-a");
    assert_eq!(restored["auth"]["refresh_token"], "refresh-a");
    assert_eq!(restored["unmanaged"], "keep");
}

#[test]
fn definition_target_inside_switch_home_is_rejected() {
    let cwd = std::env::current_dir().unwrap();
    let switch_home = tempfile::Builder::new()
        .prefix(".test-switch-")
        .tempdir_in(&cwd)
        .unwrap();
    let bad_definition = switch_home.path().join("bad.yaml");
    fs::write(
        &bad_definition,
        format!(
            r#"
schema_version: 1
app:
  id: bad
  display_name: Bad
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
        managed_keys: [BAD]
        mapping:
          BAD: "{{{{ fields.value }}}}"
"#,
            switch_home.path().join("profiles.yaml").display()
        ),
    )
    .unwrap();

    Command::cargo_bin("switch-cli")
        .unwrap()
        .env("SWITCH_CLI_HOME", switch_home.path())
        .args(["apps", "validate", bad_definition.to_str().unwrap()])
        .assert()
        .failure()
        .stderr(contains("DefinitionLoadFailed"));
}

#[test]
fn definition_executable_login_field_is_rejected() {
    let cwd = std::env::current_dir().unwrap();
    let switch_home = tempfile::Builder::new()
        .prefix(".test-switch-")
        .tempdir_in(&cwd)
        .unwrap();
    let bad_definition = switch_home.path().join("login.yaml");
    fs::write(
        &bad_definition,
        r#"
schema_version: 1
app:
  id: bad
  display_name: Bad
  definition_version: 1
login:
  command: bad login
kinds:
  env_injection:
    field_schema:
      value:
        type: string
"#,
    )
    .unwrap();

    Command::cargo_bin("switch-cli")
        .unwrap()
        .env("SWITCH_CLI_HOME", switch_home.path())
        .args(["apps", "validate", bad_definition.to_str().unwrap()])
        .assert()
        .failure()
        .stderr(contains("unknown field `login`"));
}

#[test]
fn override_appends_process_probe_and_field_defaults() {
    let cwd = std::env::current_dir().unwrap();
    let switch_home = tempfile::Builder::new()
        .prefix(".test-switch-")
        .tempdir_in(&cwd)
        .unwrap();
    let overrides_dir = switch_home.path().join("overrides.d");
    fs::create_dir_all(&overrides_dir).unwrap();
    fs::write(
        overrides_dir.join("codex.yaml"),
        r#"
schema_version: 1
app:
  id: codex
  display_name: Codex override
  definition_version: 1
process_probe:
  names: [codex-alt]
kinds:
  file_template:
    field_schema:
      model:
        default: "gpt-override"
"#,
    )
    .unwrap();

    Command::cargo_bin("switch-cli")
        .unwrap()
        .env("SWITCH_CLI_HOME", switch_home.path())
        .args(["apps"])
        .assert()
        .success()
        .stdout(contains("codex"))
        .stdout(contains("Override"));

    Command::cargo_bin("switch-cli")
        .unwrap()
        .env("SWITCH_CLI_HOME", switch_home.path())
        .args(["apps", "show", "codex"])
        .assert()
        .success()
        .stdout(contains("codex-alt"))
        .stdout(contains("gpt-override"));

    let output = Command::cargo_bin("switch-cli")
        .unwrap()
        .env("SWITCH_CLI_HOME", switch_home.path())
        .args(["apps", "show", "codex", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let definition: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(definition["app"]["id"], "codex");
    assert_eq!(
        definition["kinds"]["file_template"]["field_schema"]["model"]["default"],
        "gpt-override"
    );
    assert!(definition["process_probe"]["names"]
        .as_array()
        .unwrap()
        .iter()
        .any(|name| name == "codex-alt"));
}

#[test]
fn override_cannot_replace_targets_or_handlers() {
    let cwd = std::env::current_dir().unwrap();
    let switch_home = tempfile::Builder::new()
        .prefix(".test-switch-")
        .tempdir_in(&cwd)
        .unwrap();
    let overrides_dir = switch_home.path().join("overrides.d");
    fs::create_dir_all(&overrides_dir).unwrap();
    fs::write(
        overrides_dir.join("codex.yaml"),
        r#"
schema_version: 1
app:
  id: codex
  display_name: Bad Codex override
  definition_version: 1
kinds:
  file_template:
    targets:
      - handler: file_capture
        path: ~/.codex/replaced.json
"#,
    )
    .unwrap();

    Command::cargo_bin("switch-cli")
        .unwrap()
        .env("SWITCH_CLI_HOME", switch_home.path())
        .env("SWITCH_CLI_TEST_HOME", &cwd)
        .args(["apps", "validate"])
        .assert()
        .failure()
        .stderr(contains(
            "DefinitionLoadFailed: overrides may not replace targets or handlers",
        ));
}

#[test]
fn diagnostics_commands_work_when_installed_registry_is_invalid() {
    let cwd = std::env::current_dir().unwrap();
    let switch_home = tempfile::Builder::new()
        .prefix(".test-switch-")
        .tempdir_in(&cwd)
        .unwrap();
    let overrides_dir = switch_home.path().join("overrides.d");
    fs::create_dir_all(&overrides_dir).unwrap();
    fs::write(
        overrides_dir.join("missing.yaml"),
        r#"
schema_version: 1
app:
  id: missing
  display_name: Missing
  definition_version: 1
"#,
    )
    .unwrap();
    let standalone = tempfile::Builder::new()
        .prefix(".test-definition-")
        .suffix(".yaml")
        .tempfile_in(&cwd)
        .unwrap();
    fs::write(
        standalone.path(),
        r#"
schema_version: 1
app:
  id: scratch
  display_name: Scratch
  definition_version: 1
"#,
    )
    .unwrap();

    Command::cargo_bin("switch-cli")
        .unwrap()
        .env("SWITCH_CLI_HOME", switch_home.path())
        .env("SWITCH_CLI_TEST_HOME", &cwd)
        .args(["config", "path"])
        .assert()
        .success()
        .stdout(contains("profiles.yaml"));

    Command::cargo_bin("switch-cli")
        .unwrap()
        .env("SWITCH_CLI_HOME", switch_home.path())
        .env("SWITCH_CLI_TEST_HOME", &cwd)
        .args(["apps", "validate", standalone.path().to_str().unwrap()])
        .assert()
        .success()
        .stdout(contains("ok"));

    Command::cargo_bin("switch-cli")
        .unwrap()
        .env("SWITCH_CLI_HOME", switch_home.path())
        .env("SWITCH_CLI_TEST_HOME", &cwd)
        .args(["apps", "validate"])
        .assert()
        .failure()
        .stderr(contains("references unknown app missing"));
}

#[test]
fn apps_export_respects_source_and_override_output_rules() {
    let cwd = std::env::current_dir().unwrap();
    let switch_home = tempfile::Builder::new()
        .prefix(".test-switch-")
        .tempdir_in(&cwd)
        .unwrap();
    let overrides_dir = switch_home.path().join("overrides.d");
    fs::create_dir_all(&overrides_dir).unwrap();
    fs::write(
        overrides_dir.join("codex.yaml"),
        r#"
schema_version: 1
app:
  id: codex
  display_name: Codex override
  definition_version: 1
kinds:
  file_template:
    field_schema:
      model:
        default: "gpt-override"
"#,
    )
    .unwrap();

    Command::cargo_bin("switch-cli")
        .unwrap()
        .env("SWITCH_CLI_HOME", switch_home.path())
        .args(["apps", "export", "codex", "--source", "system"])
        .assert()
        .success()
        .stdout(contains("gpt-5-codex"))
        .stdout(contains("gpt-override").not());

    Command::cargo_bin("switch-cli")
        .unwrap()
        .env("SWITCH_CLI_HOME", switch_home.path())
        .args(["apps", "export", "codex", "--source", "resolved"])
        .assert()
        .success()
        .stdout(contains("gpt-override"));

    let generated = switch_home
        .path()
        .join("overrides.d")
        .join("generated.yaml");
    Command::cargo_bin("switch-cli")
        .unwrap()
        .env("SWITCH_CLI_HOME", switch_home.path())
        .args([
            "apps",
            "export",
            "codex",
            "--as",
            "override",
            "--output",
            generated.to_str().unwrap(),
        ])
        .assert()
        .success();

    Command::cargo_bin("switch-cli")
        .unwrap()
        .env("SWITCH_CLI_HOME", switch_home.path())
        .args(["apps", "validate", generated.to_str().unwrap()])
        .assert()
        .success()
        .stdout(contains("ok"));

    Command::cargo_bin("switch-cli")
        .unwrap()
        .env("SWITCH_CLI_HOME", switch_home.path())
        .args([
            "apps",
            "export",
            "codex",
            "--as",
            "override",
            "--output",
            generated.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(contains("exists; pass --force"));
}
