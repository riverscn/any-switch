#![allow(dead_code)]

use std::fs;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

pub fn write_codex_oauth(codex_home: &std::path::Path, account_id: &str, refresh_token: &str) {
    let auth = serde_json::json!({
        "OPENAI_API_KEY": null,
        "auth_mode": "chatgpt",
        "last_refresh": "2026-05-24T00:00:00Z",
        "tokens": {
            "access_token": "access",
            "account_id": account_id,
            "id_token": "x.eyJzdWIiOiIxMjMiLCJlbWFpbCI6ImFAYi5jIiwibmFtZSI6IkEifQ.y",
            "refresh_token": refresh_token
        }
    });
    fs::write(
        codex_home.join("auth.json"),
        serde_json::to_vec_pretty(&auth).unwrap(),
    )
    .unwrap();
}

pub fn write_claude_json(
    home: &std::path::Path,
    account_uuid: &str,
    organization_uuid: &str,
    user_id: &str,
    unmanaged: &str,
) {
    let content = serde_json::json!({
        "oauthAccount": {
            "accountUuid": account_uuid,
            "organizationUuid": organization_uuid,
            "organizationName": "Example",
            "emailAddress": "work@example.test",
            "subscriptionType": "pro"
        },
        "userID": user_id,
        "unmanaged": unmanaged
    });
    fs::write(
        home.join(".claude.json"),
        serde_json::to_vec_pretty(&content).unwrap(),
    )
    .unwrap();
}

pub fn write_claude_credentials(
    home: &std::path::Path,
    account_uuid: &str,
    organization_uuid: &str,
    refresh_token: &str,
) {
    let claude_dir = home.join(".claude");
    write_claude_credentials_file(&claude_dir, account_uuid, organization_uuid, refresh_token);
}

pub fn write_claude_credentials_file(
    claude_dir: &std::path::Path,
    account_uuid: &str,
    organization_uuid: &str,
    refresh_token: &str,
) {
    fs::create_dir_all(claude_dir).unwrap();
    let credentials = serde_json::json!({
        "claudeAiOauth": {
            "accessToken": claude_access_token(account_uuid, organization_uuid),
            "refreshToken": refresh_token,
            "expiresAt": 1_800_000_000_000i64
        }
    });
    fs::write(
        claude_dir.join(".credentials.json"),
        serde_json::to_vec_pretty(&credentials).unwrap(),
    )
    .unwrap();
}

#[cfg(target_os = "macos")]
pub fn current_test_os_user() -> String {
    unsafe {
        let passwd = libc::getpwuid(libc::geteuid());
        if passwd.is_null() || (*passwd).pw_name.is_null() {
            return "unknown".to_string();
        }
        std::ffi::CStr::from_ptr((*passwd).pw_name)
            .to_string_lossy()
            .into_owned()
    }
}

pub fn claude_access_token(account_uuid: &str, organization_uuid: &str) -> String {
    let payload = match (account_uuid, organization_uuid) {
        ("acct-a", "org-a") => {
            "eyJhY2NvdW50VXVpZCI6ImFjY3QtYSIsIm9yZ2FuaXphdGlvblV1aWQiOiJvcmctYSJ9"
        }
        ("acct-b", "org-b") => {
            "eyJhY2NvdW50VXVpZCI6ImFjY3QtYiIsIm9yZ2FuaXphdGlvblV1aWQiOiJvcmctYiJ9"
        }
        other => panic!("missing test token for {other:?}"),
    };
    format!("x.{payload}.y")
}

pub fn list_backup_ids(switch_home: &std::path::Path, app: &str) -> Vec<String> {
    let mut ids = fs::read_dir(switch_home.join("backups").join(app))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().to_string())
        .collect::<Vec<_>>();
    ids.sort();
    ids
}

pub fn write_editor_script(dir: &std::path::Path, body: &str) -> std::path::PathBuf {
    let path = dir.join("editor.sh");
    fs::write(&path, body).unwrap();
    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&path, permissions).unwrap();
    }
    path
}
