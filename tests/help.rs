use assert_cmd::Command;

#[test]
fn help_keeps_escape_hatches_out_of_normal_examples() {
    let help = command_help(&["--help"]);
    assert!(help.contains("any-switch import-current claude work --kind oauth_capture"));
    assert!(help.contains("any-switch use claude-work"));
    assert!(!help.contains("any-switch use claude-work --assume-app-stopped"));

    let use_help = command_help(&["use", "--help"]);
    assert!(use_help.contains("any-switch use claude-work"));
    assert!(!use_help.contains("any-switch use claude-work --assume-app-stopped"));

    let import_help = command_help(&["import-current", "--help"]);
    assert!(import_help.contains("any-switch import-current claude personal --kind oauth_capture"));
    assert!(!import_help.contains(
        "any-switch import-current claude personal --kind oauth_capture --assume-app-stopped"
    ));

    let add_help = command_help(&["add", "--help"]);
    assert!(add_help.contains("@prompt"));
    assert!(add_help.contains("@stdin"));
    assert!(add_help.contains("@env:NAME"));
    assert!(add_help.contains("@file:PATH"));
    assert!(add_help.contains("key=@prompt"));
    assert!(!add_help.contains("key=value or key=@env:VAR"));

    let config_help = command_help(&["config", "--help"]);
    assert!(config_help.contains("profiles.yaml path"));
    assert!(config_help.contains("any-switch doctor"));

    let config_path_help = command_help(&["config", "path", "--help"]);
    assert!(config_path_help.contains("active profiles.yaml path"));
    assert!(!config_path_help.contains("home directory"));

    let backup_help = command_help(&["backup", "--help"]);
    assert!(backup_help.contains("Backups are created automatically"));
    assert!(backup_help.contains("any-switch backup list <app>"));
    assert!(backup_help.contains("any-switch restore-target <app> <backup-id>"));

    let backup_list_help = command_help(&["backup", "list", "--help"]);
    assert!(backup_list_help.contains("restore-target <app> <backup-id>"));
    assert!(backup_list_help.contains("status <app>"));

    let detach_help = command_help(&["detach", "--help"]);
    assert!(detach_help.contains("recommended next step"));
    assert!(detach_help.contains("any-switch import-current <app> <name>"));
    assert!(detach_help.contains("any-switch import-current claude current-state"));
    assert!(detach_help.contains("does not write back first"));
}

fn command_help(args: &[&str]) -> String {
    let stdout = Command::cargo_bin("any-switch")
        .unwrap()
        .args(args)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    String::from_utf8(stdout).unwrap()
}
