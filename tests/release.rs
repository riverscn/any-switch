use std::fs;

#[test]
fn production_source_does_not_branch_on_builtin_app_ids() {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let source_root = manifest_dir.join("src");
    let mut rust_files = Vec::new();
    collect_rust_files(&source_root, &mut rust_files);
    rust_files.push(manifest_dir.join("build.rs"));

    let banned_fragments = [
        "\"codex\"",
        "\"claude\"",
        "==\"codex\"",
        "==\"claude\"",
        "\"codex\"==",
        "\"claude\"==",
        "!=\"codex\"",
        "!=\"claude\"",
        "\"codex\"!=",
        "\"claude\"!=",
        "\"codex\"=>",
        "\"claude\"=>",
        "detect_codex",
        "detect_claude",
        "ensure_codex",
        "ensure_claude",
        "read_codex",
        "read_claude",
        "extract_codex",
        "extract_claude",
        "codex_",
        "claude_",
    ];
    let mut failures = Vec::new();

    for file in rust_files {
        let text = fs::read_to_string(&file).unwrap();
        let production_text = text.split("\n#[cfg(test)]").next().unwrap_or(&text);
        for (line_index, line) in production_text.lines().enumerate() {
            let compact = line
                .chars()
                .filter(|ch| !ch.is_whitespace())
                .collect::<String>();
            for fragment in banned_fragments {
                if compact.contains(fragment) {
                    failures.push(format!(
                        "{}:{} contains app-specific production branch fragment `{}`",
                        file.strip_prefix(env!("CARGO_MANIFEST_DIR"))
                            .unwrap_or(&file)
                            .display(),
                        line_index + 1,
                        fragment
                    ));
                }
            }
        }
    }

    assert!(
        failures.is_empty(),
        "production code must stay app-definition driven and must not name built-in app ids:\n{}",
        failures.join("\n")
    );
}

#[test]
fn workflow_yaml_files_parse_with_rust_dependencies() {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for path in [".github/workflows/ci.yml", ".github/workflows/release.yml"] {
        let text = fs::read_to_string(manifest_dir.join(path)).unwrap();
        let value: serde_yaml::Value = serde_yaml::from_str(&text).unwrap();
        assert!(value.get("jobs").is_some(), "{path} missing jobs");
    }
    for path in [
        ".github/dependabot.yml",
        ".github/ISSUE_TEMPLATE/config.yml",
        ".github/ISSUE_TEMPLATE/bug_report.yml",
        ".github/ISSUE_TEMPLATE/feature_request.yml",
        ".github/ISSUE_TEMPLATE/release_checklist.yml",
    ] {
        let text = fs::read_to_string(manifest_dir.join(path)).unwrap();
        let value: serde_yaml::Value = serde_yaml::from_str(&text).unwrap();
        assert!(
            value.get("body").is_some()
                || value.get("contact_links").is_some()
                || value.get("updates").is_some(),
            "{path} missing expected GitHub config content"
        );
    }
    let pr_template =
        fs::read_to_string(manifest_dir.join(".github/pull_request_template.md")).unwrap();
    assert!(pr_template.contains("scripts/verify-local.sh"));
    assert!(pr_template.contains("docs/manual-verification.md"));
    assert!(pr_template.contains("docs/manual-evidence-template.md"));
    let release_checklist =
        fs::read_to_string(manifest_dir.join(".github/ISSUE_TEMPLATE/release_checklist.yml"))
            .unwrap();
    assert_contains_all(
        &release_checklist,
        &[
            "durable release evidence",
            "manual-evidence-*.md",
            "Do not rely only on an ignored local file path",
            "2: macOS Claude OAuth import: passed",
            "any-switch status claude` reported `matched`",
        ],
        "release checklist should distinguish ignored local evidence from durable release evidence",
    );
    let ci_text = fs::read_to_string(manifest_dir.join(".github/workflows/ci.yml")).unwrap();
    let ci_workflow: serde_yaml::Value = serde_yaml::from_str(&ci_text).unwrap();
    assert_checkout_steps_do_not_persist_credentials(&ci_workflow, ".github/workflows/ci.yml");
    assert_eq!(
        ci_workflow["permissions"]["contents"].as_str(),
        Some("read"),
        "CI should run with read-only repository contents permission"
    );
    assert_eq!(
        ci_workflow["concurrency"]["group"].as_str(),
        Some("ci-${{ github.workflow }}-${{ github.ref }}"),
        "CI should group duplicate runs by workflow and ref"
    );
    assert_eq!(
        ci_workflow["concurrency"]["cancel-in-progress"].as_bool(),
        Some(true),
        "CI should cancel stale duplicate runs for the same ref"
    );
    assert!(
        ci_workflow.get("env").is_none()
            || ci_workflow["env"]
                .get("FORCE_JAVASCRIPT_ACTIONS_TO_NODE24")
                .is_none(),
        "CI should use Node 24-compatible action versions instead of relying on a temporary runtime override"
    );
    assert!(
        ci_text.contains("actions/checkout@v6"),
        "CI should use actions/checkout@v6 for Node 24 runtime support"
    );
    assert!(
        !ci_text.contains("actions/checkout@v4"),
        "CI must not use actions/checkout@v4 because it runs on deprecated Node.js 20"
    );
    assert!(
        !ci_text.contains("Swatinem/rust-cache"),
        "CI should not rely on uncategorized JavaScript cache actions while Node runtime migration is active"
    );
    assert!(ci_text.contains("windows-latest"));
    assert!(ci_text.contains("x86_64-pc-windows-msvc"));
    assert!(ci_text.contains("cargo clippy --locked --target x86_64-pc-windows-msvc"));
    assert!(ci_text.contains("lock::tests::second_lock_is_busy"));
    assert!(ci_text.contains("paths::tests::current_os_user_prefers_username_on_windows"));
    assert!(ci_text.contains("process::tests::matches_windows_exe_names_case_insensitively"));
    assert!(ci_text.contains("process::tests::parses_quoted_csv_fields"));
    assert!(ci_text.contains(
        "powershell -NoProfile -ExecutionPolicy Bypass -File scripts/manual-evidence.ps1 -Help"
    ));
    assert!(ci_text.contains(
        "bash scripts/package-release.sh ci-windows x86_64-pc-windows-msvc target/x86_64-pc-windows-msvc/release/any-switch.exe ."
    ));
    assert!(ci_text.contains("tar -tzf any-switch-ci-windows-x86_64-pc-windows-msvc.tar.gz"));
    assert!(ci_text.contains("any-switch-ci-windows-x86_64-pc-windows-msvc/any-switch.exe"));
    let release_text =
        fs::read_to_string(manifest_dir.join(".github/workflows/release.yml")).unwrap();
    let release_workflow: serde_yaml::Value = serde_yaml::from_str(&release_text).unwrap();
    assert_checkout_steps_do_not_persist_credentials(
        &release_workflow,
        ".github/workflows/release.yml",
    );
    assert_eq!(
        release_workflow["permissions"]["contents"].as_str(),
        Some("read"),
        "release workflow should default to read-only repository contents permission"
    );
    assert_eq!(
        release_workflow["concurrency"]["group"].as_str(),
        Some("release-${{ github.ref }}"),
        "release workflow should group duplicate runs by tag ref"
    );
    assert_eq!(
        release_workflow["concurrency"]["cancel-in-progress"].as_bool(),
        Some(false),
        "release workflow must not cancel an in-progress tag publish"
    );
    assert!(
        release_workflow.get("env").is_none()
            || release_workflow["env"]
                .get("FORCE_JAVASCRIPT_ACTIONS_TO_NODE24")
                .is_none(),
        "release should use Node 24-compatible action versions instead of relying on a temporary runtime override"
    );
    assert_eq!(
        release_workflow["jobs"]["build"]["permissions"]["contents"].as_str(),
        Some("read"),
        "release build job should not need GitHub Release write permission"
    );
    assert_eq!(
        release_workflow["jobs"]["build"]["permissions"]["artifact-metadata"].as_str(),
        Some("write"),
        "release build job should grant artifact metadata write for upload-artifact"
    );
    assert_eq!(
        release_workflow["jobs"]["publish"]["permissions"]["actions"].as_str(),
        Some("read"),
        "release publish job should grant actions read for download-artifact"
    );
    assert_eq!(
        release_workflow["jobs"]["publish"]["permissions"]["artifact-metadata"].as_str(),
        Some("read"),
        "release publish job should grant artifact metadata read for downloaded artifact records"
    );
    assert_eq!(
        release_workflow["jobs"]["publish"]["permissions"]["contents"].as_str(),
        Some("write"),
        "release publish job must opt into contents: write for GitHub Release uploads"
    );
    assert_contains_all(
        &release_text,
        &[
            "actions/checkout@v6",
            "softprops/action-gh-release@v3",
            "actions/upload-artifact@v7",
            "actions/download-artifact@v7",
        ],
        "release workflow should use current Node 24-compatible action versions",
    );
    assert_not_contains_any(
        &release_text,
        &[
            "actions/checkout@v4",
            "softprops/action-gh-release@v2",
            "Swatinem/rust-cache",
        ],
        "release workflow should avoid deprecated or uncategorized JavaScript actions",
    );
    assert_contains_all(
        &release_text,
        &[
            "Verify tag matches Cargo version",
            "cargo pkgid",
            "GITHUB_REF_NAME",
            "body_path: CHANGELOG.md",
            "cargo clippy --locked --target x86_64-pc-windows-msvc",
            "lock::tests::second_lock_is_busy",
            "paths::tests::current_os_user_prefers_username_on_windows",
            "process::tests::matches_windows_exe_names_case_insensitively",
            "process::tests::parses_quoted_csv_fields",
            "powershell -NoProfile -ExecutionPolicy Bypass -File scripts/manual-evidence.ps1 -Help",
        ],
        "release workflow should verify tag alignment and Windows target behavior",
    );
    let verify_text = fs::read_to_string(manifest_dir.join("scripts/verify-local.sh")).unwrap();
    assert!(verify_text.contains("git diff --check"));
    assert!(verify_text.contains("command -v shasum"));
    assert!(verify_text.contains("sha256sum -c"));
}

#[test]
fn cargo_source_package_excludes_local_agent_and_evidence_files() {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let manifest_text = fs::read_to_string(manifest_dir.join("Cargo.toml")).unwrap();
    let manifest: toml_edit::DocumentMut = manifest_text.parse().unwrap();
    let package = manifest["package"].as_table().unwrap();
    assert_eq!(
        package["license"].as_str(),
        Some("MIT"),
        "Cargo package metadata should declare the repository license"
    );
    assert_eq!(
        package["description"].as_str(),
        Some("Local app profile/state switcher."),
        "Cargo package metadata should describe the user-facing tool"
    );
    assert_eq!(
        package["readme"].as_str(),
        Some("README.md"),
        "Cargo package metadata should point to the packaged README"
    );
    assert_eq!(
        package["repository"].as_str(),
        Some("https://github.com/riverscn/any-switch"),
        "Cargo package metadata should point to the remote repository"
    );
    assert_eq!(
        package["homepage"].as_str(),
        Some("https://github.com/riverscn/any-switch"),
        "Cargo package metadata should point to the project homepage"
    );
    assert_eq!(
        package["documentation"].as_str(),
        Some("https://github.com/riverscn/any-switch/tree/main/docs"),
        "Cargo package metadata should point users to the documentation"
    );
    assert_eq!(
        package["rust-version"].as_str(),
        Some("1.95"),
        "Cargo package metadata should declare the minimum Rust version"
    );
    let keywords = package["keywords"]
        .as_array()
        .expect("Cargo package metadata should declare keywords")
        .iter()
        .filter_map(|value| value.as_str())
        .collect::<Vec<_>>();
    assert!(
        keywords.contains(&"cli")
            && keywords.contains(&"profiles")
            && keywords.contains(&"credentials")
            && keywords.contains(&"switcher"),
        "Cargo package keywords should make the tool discoverable"
    );
    let categories = package["categories"]
        .as_array()
        .expect("Cargo package metadata should declare categories")
        .iter()
        .filter_map(|value| value.as_str())
        .collect::<Vec<_>>();
    assert!(
        categories.contains(&"command-line-utilities") && categories.contains(&"config"),
        "Cargo package categories should classify the CLI and config use case"
    );
    let exclude = package["exclude"]
        .as_array()
        .expect("Cargo package metadata should exclude local state")
        .iter()
        .filter_map(|value| value.as_str())
        .collect::<Vec<_>>();
    for pattern in [
        ".claude/**",
        ".codex/**",
        ".tmp/**",
        "/manual-evidence-*.md",
    ] {
        assert!(
            exclude.contains(&pattern),
            "Cargo package exclude should contain local-only pattern {pattern}"
        );
    }
    let rust_toolchain = fs::read_to_string(manifest_dir.join("rust-toolchain.toml")).unwrap();
    assert!(
        rust_toolchain.contains("channel = \"1.95.0\""),
        "rust-toolchain.toml should pin the CI/release toolchain"
    );
    let license = fs::read_to_string(manifest_dir.join("LICENSE")).unwrap();
    assert!(
        license.starts_with("MIT License"),
        "LICENSE should contain the MIT license text advertised by Cargo metadata"
    );
    let readme = fs::read_to_string(manifest_dir.join("README.md")).unwrap();
    let user_guide = fs::read_to_string(manifest_dir.join("docs/user-guide.md")).unwrap();
    assert!(
        readme.contains("[LICENSE](LICENSE)"),
        "README license section should link to the packaged LICENSE file"
    );
    assert!(
        readme.contains("Rust `1.95.0`")
            && readme.contains("rust-version = \"1.95\"")
            && readme.contains("rust-toolchain.toml"),
        "README should document the source-build Rust toolchain requirement"
    );
    let normalized_readme = readme.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        normalized_readme.contains("cloud-synced folders")
            && normalized_readme.contains("iCloud Drive")
            && normalized_readme.contains("Dropbox")
            && normalized_readme.contains("OneDrive")
            && normalized_readme.contains("Google Drive")
            && normalized_readme.contains("doctor"),
        "README should warn users not to put credential state in cloud sync roots"
    );
    let security = fs::read_to_string(manifest_dir.join("SECURITY.md")).unwrap();
    assert!(
        security.contains("ANY_SWITCH_HOME")
            && security.contains("cloud-synced folders")
            && security.contains("doctor"),
        "SECURITY.md should document cloud-sync risk for local credential state"
    );
    let contributing = fs::read_to_string(manifest_dir.join("CONTRIBUTING.md")).unwrap();
    assert!(
        contributing.contains("cargo fmt -- --check")
            && contributing.contains("cargo test --locked --all-targets")
            && contributing.contains("cargo clippy --locked --all-targets -- -D warnings")
            && contributing.contains("scripts/verify-local.sh")
            && contributing.contains("offline source-package verification"),
        "CONTRIBUTING.md should document the same locked local checks used by CI/release gates"
    );
    assert!(
        readme.contains("any-switch import-current claude work --kind oauth_capture"),
        "README should show the normal Claude import path without a process-safety escape hatch"
    );
    assert!(
        user_guide.contains("any-switch import-current claude personal --kind oauth_capture"),
        "user guide should show the normal Claude import path without a process-safety escape hatch"
    );
    assert!(
        !readme.contains(
            "any-switch import-current claude work --kind oauth_capture --assume-app-stopped --yes"
        ),
        "README should not present --assume-app-stopped --yes as the normal Claude import path"
    );
    assert!(
        !user_guide.contains("any-switch import-current claude personal --assume-app-stopped"),
        "user guide should not present --assume-app-stopped as the normal Claude import path"
    );
    assert!(
        readme.contains("macOS-evidenced stage release")
            && readme.contains("Linux")
            && readme.contains("Windows real-app evidence")
            && readme.contains("does not")
            && readme.contains("claim full `docs/design.md` section 13 coverage"),
        "README should describe the current stage release scope before installation"
    );
    assert!(
        user_guide.contains("macOS-evidenced stage release")
            && user_guide.contains("Linux and Windows")
            && user_guide.contains("claims full")
            && user_guide.contains("`docs/design.md` section 13 coverage"),
        "user guide should describe the current stage release scope before first-run instructions"
    );
    let changelog = fs::read_to_string(manifest_dir.join("CHANGELOG.md")).unwrap();
    assert!(
        changelog.contains("macOS-evidenced stage release")
            && changelog.contains("Do not treat this release")
            && changelog.contains("full `docs/design.md` section 13 coverage"),
        "CHANGELOG should describe the current stage release scope without over-claiming manual evidence"
    );
    assert!(
        changelog.contains(&format!("## {} - ", env!("CARGO_PKG_VERSION"))),
        "CHANGELOG should have a public release section for the package version used as the GitHub Release body"
    );
    assert!(
        readme.contains("Do not pass it preemptively")
            && user_guide.contains("If no matching process was")
            && user_guide.contains("remove the flag and rerun")
            && changelog.contains("preemptive default flag"),
        "user-facing docs should explain that --assume-app-stopped is only a process-probe false-positive escape hatch"
    );
    let output = std::process::Command::new("cargo")
        .current_dir(&manifest_dir)
        .args(["package", "--locked", "--allow-dirty", "--list"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "cargo package --list failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let listing = String::from_utf8(output.stdout).unwrap();
    assert!(
        listing.contains("docs/manual-evidence-template.md"),
        "cargo source package must include the manual evidence template"
    );
    for required in [
        ".editorconfig",
        ".gitattributes",
        "CHANGELOG.md",
        "CODE_OF_CONDUCT.md",
        "LICENSE",
        "README.md",
        "rust-toolchain.toml",
        ".github/ISSUE_TEMPLATE/bug_report.yml",
        ".github/ISSUE_TEMPLATE/config.yml",
        ".github/ISSUE_TEMPLATE/feature_request.yml",
        ".github/ISSUE_TEMPLATE/release_checklist.yml",
        ".github/dependabot.yml",
        ".github/pull_request_template.md",
    ] {
        assert!(
            listing.contains(required),
            "cargo source package must include {required}"
        );
    }
    for forbidden in [
        ".claude/",
        ".codex/",
        ".any-switch/",
        ".any-switch-",
        ".DS_Store",
        ".smoke-",
        ".test-",
        ".tmp/",
        ".tmp-",
        "\nmanual-evidence-",
    ] {
        assert!(
            !listing.contains(forbidden),
            "cargo source package must not include local-only file pattern {forbidden}"
        );
    }
}

#[test]
fn release_workflow_uploads_documented_binary_artifacts() {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workflow_text =
        fs::read_to_string(manifest_dir.join(".github/workflows/release.yml")).unwrap();
    let workflow: serde_yaml::Value = serde_yaml::from_str(&workflow_text).unwrap();
    let release_doc = fs::read_to_string(manifest_dir.join("docs/release.md")).unwrap();

    assert!(workflow_text.contains("tags:"));
    assert!(workflow_text.contains("\"v*\""));
    assert!(workflow_text.contains("Verify tag matches Cargo version"));
    assert!(
        workflow_text.contains(r#"sed -n 's/.*[#@]//p'"#),
        "release workflow must parse both path pkgids like path+file://...#0.1.0 and registry pkgids like name@0.1.0"
    );
    assert!(workflow_text.contains("tag ${GITHUB_REF_NAME} does not match Cargo.toml version"));
    assert!(release_doc.contains("tag must match the package version in `Cargo.toml`"));
    assert_eq!(workflow["permissions"]["contents"].as_str(), Some("read"));
    assert_eq!(
        workflow["jobs"]["build"]["permissions"]["contents"].as_str(),
        Some("read")
    );
    assert_eq!(
        workflow["jobs"]["build"]["permissions"]["artifact-metadata"].as_str(),
        Some("write")
    );
    assert_eq!(
        workflow["jobs"]["publish"]["permissions"]["actions"].as_str(),
        Some("read")
    );
    assert_eq!(
        workflow["jobs"]["publish"]["permissions"]["artifact-metadata"].as_str(),
        Some("read")
    );
    assert_eq!(
        workflow["jobs"]["publish"]["permissions"]["contents"].as_str(),
        Some("write")
    );
    assert_eq!(
        workflow["jobs"]["publish"]["needs"].as_str(),
        Some("build"),
        "publish job must wait for all release target builds"
    );

    let include = workflow["jobs"]["build"]["strategy"]["matrix"]["include"]
        .as_sequence()
        .unwrap();
    let mut targets = include
        .iter()
        .map(|entry| entry["target"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    targets.sort();
    assert_eq!(
        targets,
        [
            "aarch64-apple-darwin",
            "x86_64-apple-darwin",
            "x86_64-pc-windows-msvc",
            "x86_64-unknown-linux-gnu"
        ]
    );
    for target in &targets {
        assert!(
            release_doc.contains(target),
            "docs/release.md must document release target {target}"
        );
    }
    assert!(
        release_doc.contains("macOS-evidenced stage release"),
        "docs/release.md must describe the current manual-evidence release scope"
    );
    assert!(
        release_doc.contains("copy only a short redacted")
            && release_doc.contains("Do not attach Keychain values")
            && release_doc.contains("any-switch status claude reported matched")
            && release_doc.contains("tracked in docs/evidence-followups.md"),
        "docs/release.md must provide a safe minimal evidence summary for the current stage release"
    );
    assert!(
        release_doc.contains("must not claim")
            && release_doc.contains("full MVP / section 13 coverage"),
        "docs/release.md must prevent over-claiming deferred manual evidence"
    );
    assert!(
        release_doc.contains("Actions Runner `v2.329.0`")
            && release_doc.contains("Actions Runner `v2.327.1`"),
        "docs/release.md must document self-hosted runner requirements for Node 24 actions"
    );
    let design_doc = fs::read_to_string(manifest_dir.join("docs/design.md")).unwrap();
    let acceptance_doc = fs::read_to_string(manifest_dir.join("docs/acceptance.md")).unwrap();
    let manual_verification =
        fs::read_to_string(manifest_dir.join("docs/manual-verification.md")).unwrap();
    assert!(
        design_doc.contains("macOS-evidenced stage release")
            && design_doc.contains("release blocker")
            && design_doc.contains("deferred follow-up")
            && design_doc.contains("不得宣称 full §13 coverage"),
        "docs/design.md must match the current staged release evidence policy"
    );
    assert!(
        !design_doc.contains("前置实测（§13 末尾 A–E 项）必须在 M1 发布前全部完成，不可推迟"),
        "docs/design.md must not contradict the staged release evidence policy"
    );
    assert!(
        acceptance_doc.contains("Current-stage real macOS Claude Code Keychain import evidence")
            && acceptance_doc.contains("release checklist")
            && acceptance_doc.contains("repository automation cannot prove the real local Keychain import by itself")
            && !acceptance_doc.contains("Real Claude Code Keychain import remains manual"),
        "docs/acceptance.md must distinguish current-stage real macOS evidence from automated coverage"
    );
    assert!(
        manual_verification.contains("Current-stage blocker steps")
            && manual_verification.contains("Deferred full-coverage experiments")
            && manual_verification.contains("deferred full section 13 evidence items A, B, and C"),
        "docs/manual-verification.md must distinguish the current-stage blocker from deferred full-coverage experiments"
    );
    assert!(release_doc.contains("sha256sum -c any-switch-<tag>-<target>.tar.gz.sha256"));
    for secret in [
        "APPLE_DEVELOPER_ID_CERTIFICATE_BASE64",
        "APPLE_DEVELOPER_ID_CERTIFICATE_PASSWORD",
        "APPLE_CODESIGN_IDENTITY",
        "APPLE_ID",
        "APPLE_APP_SPECIFIC_PASSWORD",
        "APPLE_TEAM_ID",
    ] {
        assert!(
            workflow_text.contains(secret),
            "release workflow must expose optional macOS signing secret {secret}"
        );
        assert!(
            release_doc.contains(secret),
            "docs/release.md must document macOS signing secret {secret}"
        );
    }
    let signing_script =
        fs::read_to_string(manifest_dir.join("scripts/sign-macos-binary.sh")).unwrap();
    assert!(
        signing_script.contains("base64 --decode") && signing_script.contains("base64 -D"),
        "macOS signing script should decode certificates on both GNU and BSD/macOS base64"
    );

    let steps = workflow["jobs"]["build"]["steps"].as_sequence().unwrap();
    assert!(steps.iter().any(|step| step["run"]
        .as_str()
        .is_some_and(|run| run.contains("scripts/sign-macos-binary.sh"))));
    assert!(steps.iter().any(|step| step["run"]
        .as_str()
        .is_some_and(|run| run.contains("scripts/package-release.sh"))));
    let upload_step = steps
        .iter()
        .find(|step| {
            step["uses"]
                .as_str()
                .is_some_and(|uses| uses == "actions/upload-artifact@v7")
        })
        .expect("release build job must upload per-target workflow artifacts");
    assert_eq!(
        upload_step["with"]["if-no-files-found"].as_str(),
        Some("error")
    );
    assert_eq!(upload_step["with"]["retention-days"].as_i64(), Some(7));
    let upload_paths = upload_step["with"]["path"].as_str().unwrap();
    assert!(upload_paths.contains("any-switch-${{ github.ref_name }}-${{ matrix.target }}.tar.gz"));
    assert!(upload_paths
        .contains("any-switch-${{ github.ref_name }}-${{ matrix.target }}.tar.gz.sha256"));

    let publish_steps = workflow["jobs"]["publish"]["steps"].as_sequence().unwrap();
    let download_step = publish_steps
        .iter()
        .find(|step| {
            step["uses"]
                .as_str()
                .is_some_and(|uses| uses == "actions/download-artifact@v7")
        })
        .expect("release publish job must download built artifacts after all targets pass");
    assert_eq!(
        download_step["with"]["pattern"].as_str(),
        Some("any-switch-${{ github.ref_name }}-*")
    );
    assert_eq!(
        download_step["with"]["merge-multiple"].as_bool(),
        Some(true)
    );
    let verify_artifacts = publish_steps
        .iter()
        .find(|step| {
            step["name"]
                .as_str()
                .is_some_and(|name| name == "Verify release artifacts")
        })
        .and_then(|step| step["run"].as_str())
        .expect("release publish job must verify every expected target artifact");
    assert!(verify_artifacts
        .contains(r#"test -f "release-artifacts/any-switch-${GITHUB_REF_NAME}-${target}.tar.gz""#));
    assert!(verify_artifacts.contains(
        r#"test -f "release-artifacts/any-switch-${GITHUB_REF_NAME}-${target}.tar.gz.sha256""#
    ));
    assert!(verify_artifacts.contains("cd release-artifacts"));
    assert!(verify_artifacts.contains("command -v shasum"));
    assert!(verify_artifacts
        .contains(r#"shasum -a 256 -c "any-switch-${GITHUB_REF_NAME}-${target}.tar.gz.sha256""#));
    assert!(verify_artifacts
        .contains(r#"sha256sum -c "any-switch-${GITHUB_REF_NAME}-${target}.tar.gz.sha256""#));
    for target in &targets {
        assert!(
            verify_artifacts.contains(target),
            "publish job must include expected target {target} in its artifact verification loop"
        );
    }

    let release_step = publish_steps
        .iter()
        .find(|step| {
            step["uses"]
                .as_str()
                .is_some_and(|uses| uses.starts_with("softprops/action-gh-release@"))
        })
        .expect("release workflow must upload artifacts to a GitHub release");
    assert_eq!(
        release_step["uses"].as_str(),
        Some("softprops/action-gh-release@v3")
    );
    assert_eq!(
        release_step["with"]["fail_on_unmatched_files"].as_bool(),
        Some(true),
        "release upload must fail if an expected artifact is missing"
    );
    assert_eq!(
        release_step["with"]["body_path"].as_str(),
        Some("CHANGELOG.md"),
        "GitHub Release notes must use the checked-in changelog so stage-release warnings are public"
    );
    assert!(
        release_step["with"].get("generate_release_notes").is_none(),
        "generated-only release notes can omit required manual-evidence scope warnings"
    );
    let files = release_step["with"]["files"].as_str().unwrap();
    assert!(files.contains("release-artifacts/*.tar.gz"));
    assert!(files.contains("release-artifacts/*.tar.gz.sha256"));
}

#[test]
fn release_package_script_uses_exe_name_for_windows_archives() {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let out_dir = tempfile::Builder::new()
        .prefix(".test-windows-release-package-")
        .tempdir_in(&manifest_dir)
        .unwrap();

    let output = std::process::Command::new("bash")
        .current_dir(&manifest_dir)
        .arg("scripts/package-release.sh")
        .arg("v9.9.9")
        .arg("x86_64-pc-windows-msvc")
        .arg(env!("CARGO_BIN_EXE_any-switch"))
        .arg(out_dir.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "packaging failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let archive = out_dir
        .path()
        .join("any-switch-v9.9.9-x86_64-pc-windows-msvc.tar.gz");
    let listing = std::process::Command::new("tar")
        .arg("-tzf")
        .arg(&archive)
        .output()
        .unwrap();
    assert!(listing.status.success());
    let listing = String::from_utf8(listing.stdout).unwrap();
    assert!(listing.contains("any-switch-v9.9.9-x86_64-pc-windows-msvc/any-switch.exe"));
    assert!(!listing.contains("any-switch-v9.9.9-x86_64-pc-windows-msvc/any-switch\n"));
}

#[test]
fn macos_signing_script_skips_without_blocking_unsigned_artifacts() {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let output = std::process::Command::new("bash")
        .current_dir(&manifest_dir)
        .arg("scripts/sign-macos-binary.sh")
        .arg("x86_64-unknown-linux-gnu")
        .arg(env!("CARGO_BIN_EXE_any-switch"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "non-macOS signing skip should not block release artifacts\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout)
        .contains("macOS signing: skipped for non-macOS target"));

    let signing_script =
        fs::read_to_string(manifest_dir.join("scripts/sign-macos-binary.sh")).unwrap();
    assert!(
        signing_script.contains("skipped; missing ${missing_signing[*]}")
            && signing_script.contains("exit 0"),
        "missing macOS signing secrets must skip instead of failing unsigned releases"
    );
}

#[test]
fn release_archive_includes_auditable_builtin_definitions() {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workflow = fs::read_to_string(manifest_dir.join(".github/workflows/release.yml")).unwrap();
    let release_doc = fs::read_to_string(manifest_dir.join("docs/release.md")).unwrap();
    let mut builtin_definition_names =
        fs::read_dir(manifest_dir.join("src/app_definitions/builtin"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().to_string())
            .filter(|name| name.ends_with(".yaml"))
            .collect::<Vec<_>>();
    builtin_definition_names.sort();
    assert!(!builtin_definition_names.is_empty());
    let claude_definition_text =
        fs::read_to_string(manifest_dir.join("src/app_definitions/builtin/claude.yaml")).unwrap();
    let claude_definition: serde_yaml::Value =
        serde_yaml::from_str(&claude_definition_text).unwrap();
    let claude_oauth_sources = claude_definition["kinds"]["oauth_capture"]["capture_sources"]
        .as_sequence()
        .unwrap();
    let claude_file_source = claude_oauth_sources
        .iter()
        .find(|source| source["stored_as"].as_str() == Some("credentials.json"))
        .expect("Claude file-backed OAuth source should be auditable");
    let claude_file_platforms = claude_file_source["platforms"]
        .as_sequence()
        .expect("Claude file-backed OAuth source must declare explicit platforms")
        .iter()
        .map(|platform| platform.as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        claude_file_platforms,
        ["linux"],
        "Claude file-backed OAuth bytes are not portable to macOS Keychain or unverified Windows capture"
    );
    let out_dir = tempfile::Builder::new()
        .prefix(".test-release-package-")
        .tempdir_in(&manifest_dir)
        .unwrap();

    let output = std::process::Command::new("bash")
        .current_dir(&manifest_dir)
        .arg("scripts/package-release.sh")
        .arg("v9.9.9")
        .arg("test-target")
        .arg(env!("CARGO_BIN_EXE_any-switch"))
        .arg(out_dir.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "packaging failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_package_output_has_no_temporary_files(out_dir.path());

    let archive = out_dir.path().join("any-switch-v9.9.9-test-target.tar.gz");
    let checksum = out_dir
        .path()
        .join("any-switch-v9.9.9-test-target.tar.gz.sha256");
    assert!(archive.exists());
    assert!(checksum.exists());
    let checksum_text = fs::read_to_string(&checksum).unwrap();
    let expected_checksum_suffix = "  any-switch-v9.9.9-test-target.tar.gz\n";
    assert!(
        checksum_text.ends_with(expected_checksum_suffix),
        "checksum should use a portable archive basename, got {checksum_text:?}"
    );
    assert!(!checksum_text.contains(out_dir.path().to_str().unwrap()));
    let listing = std::process::Command::new("tar")
        .arg("-tzf")
        .arg(&archive)
        .output()
        .unwrap();
    assert!(listing.status.success());
    let listing = String::from_utf8(listing.stdout).unwrap();
    for path in [
        "any-switch-v9.9.9-test-target/any-switch",
        "any-switch-v9.9.9-test-target/README.md",
        "any-switch-v9.9.9-test-target/CHANGELOG.md",
        "any-switch-v9.9.9-test-target/CODE_OF_CONDUCT.md",
        "any-switch-v9.9.9-test-target/CONTRIBUTING.md",
        "any-switch-v9.9.9-test-target/SECURITY.md",
        "any-switch-v9.9.9-test-target/LICENSE",
        "any-switch-v9.9.9-test-target/docs/user-guide.md",
        "any-switch-v9.9.9-test-target/docs/design.md",
        "any-switch-v9.9.9-test-target/docs/release.md",
        "any-switch-v9.9.9-test-target/docs/acceptance.md",
        "any-switch-v9.9.9-test-target/docs/evidence-followups.md",
        "any-switch-v9.9.9-test-target/docs/manual-verification.md",
        "any-switch-v9.9.9-test-target/docs/manual-evidence-template.md",
        "any-switch-v9.9.9-test-target/scripts/manual-evidence.sh",
        "any-switch-v9.9.9-test-target/scripts/manual-evidence.ps1",
    ] {
        assert!(listing.contains(path), "archive missing {path}");
    }
    for name in builtin_definition_names {
        let path = format!("any-switch-v9.9.9-test-target/app_definitions/builtin/{name}");
        assert!(listing.contains(&path), "archive missing {path}");
    }

    let checksum_output = std::process::Command::new("shasum")
        .current_dir(out_dir.path())
        .args([
            "-a",
            "256",
            "-c",
            "any-switch-v9.9.9-test-target.tar.gz.sha256",
        ])
        .output()
        .unwrap();
    assert!(
        checksum_output.status.success(),
        "checksum failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&checksum_output.stdout),
        String::from_utf8_lossy(&checksum_output.stderr)
    );

    let extract_dir = out_dir.path().join("extract");
    fs::create_dir_all(&extract_dir).unwrap();
    let extract_output = std::process::Command::new("tar")
        .arg("-xzf")
        .arg(&archive)
        .arg("-C")
        .arg(&extract_dir)
        .output()
        .unwrap();
    assert!(
        extract_output.status.success(),
        "extract failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&extract_output.stdout),
        String::from_utf8_lossy(&extract_output.stderr)
    );
    let packaged_binary = extract_dir
        .join("any-switch-v9.9.9-test-target")
        .join("any-switch");
    let version_output = std::process::Command::new(&packaged_binary)
        .arg("--version")
        .output()
        .unwrap();
    assert!(
        version_output.status.success(),
        "packaged binary failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&version_output.stdout),
        String::from_utf8_lossy(&version_output.stderr)
    );
    assert_eq!(
        String::from_utf8(version_output.stdout).unwrap().trim(),
        concat!("any-switch ", env!("CARGO_PKG_VERSION"))
    );
    let packaged_evidence_script = extract_dir
        .join("any-switch-v9.9.9-test-target")
        .join("scripts")
        .join("manual-evidence.sh");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mode = fs::metadata(&packaged_evidence_script)
            .unwrap()
            .permissions()
            .mode();
        assert!(
            mode & 0o111 != 0,
            "packaged manual-evidence.sh should be directly executable"
        );
    }
    let evidence_help_output = std::process::Command::new(&packaged_evidence_script)
        .arg("--help")
        .output()
        .unwrap();
    assert!(
        evidence_help_output.status.success(),
        "packaged manual-evidence.sh failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&evidence_help_output.stdout),
        String::from_utf8_lossy(&evidence_help_output.stderr)
    );
    assert!(String::from_utf8(evidence_help_output.stderr)
        .unwrap()
        .contains("usage: manual-evidence.sh"));
    let evidence_home = extract_dir.join("manual-evidence-home");
    fs::create_dir_all(&evidence_home).unwrap();
    let evidence_path = extract_dir.join("manual-evidence-test.md");
    let evidence_output = std::process::Command::new(&packaged_evidence_script)
        .current_dir(extract_dir.join("any-switch-v9.9.9-test-target"))
        .env("HOME", &evidence_home)
        .env("ANY_SWITCH_TEST_HOME", &evidence_home)
        .env("ANY_SWITCH_BIN", &packaged_binary)
        .env("PATH", "/usr/bin:/bin")
        .env_remove("ANY_SWITCH_HOME")
        .arg(&evidence_path)
        .output()
        .unwrap();
    assert!(
        evidence_output.status.success(),
        "packaged manual-evidence.sh generation failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&evidence_output.stdout),
        String::from_utf8_lossy(&evidence_output.stderr)
    );
    let evidence = fs::read_to_string(&evidence_path).unwrap();
    assert!(
        evidence.contains(
            "Email addresses, UUID-like identifiers, common JSON identity names, and keychain account labels"
        )
    );
    assert!(evidence.contains("ANY_SWITCH_HOME` note: temporary; removed when this script exits"));
    assert!(evidence.contains("## any-switch Apps"));
    assert!(evidence.contains("## Evidence Summary"));
    assert!(evidence.contains("docs/evidence-followups.md"));
    assert!(evidence.contains("### Current-Stage Release Blockers"));
    assert!(evidence.contains("### Deferred Follow-Up Evidence"));
    assert!(evidence.contains("## Item 2: Claude OAuth Import On macOS"));
    assert!(evidence.contains("any-switch import-current claude manual-macos --kind oauth_capture"));
    assert!(!evidence
        .contains("any-switch import-current claude manual-macos --kind oauth_capture --yes"));
    assert!(evidence.contains("## Deferred Full-Coverage Experiments"));
    assert!(evidence.contains("not current-stage release blockers"));
    assert!(evidence.contains("## Item 3: Claude OAuth Import On Linux"));
    assert!(evidence.contains("any-switch import-current claude manual-linux --kind oauth_capture"));
    assert!(!evidence
        .contains("any-switch import-current claude manual-linux --kind oauth_capture --yes"));
    assert!(evidence.contains("## Windows Release Smoke Test"));
    assert!(evidence.contains("## Preflight E: Codex External Restore Flow"));
    assert!(evidence.contains("any-switch import-current codex manual-codex-refresh --kind auto"));
    assert!(!evidence
        .contains("any-switch import-current codex manual-codex-refresh --kind auto --yes"));
    assert!(evidence.contains("## Final Decision"));
    let temporary_home = evidence
        .lines()
        .find_map(|line| line.strip_prefix("- `ANY_SWITCH_HOME` used: "))
        .expect("manual evidence must record ANY_SWITCH_HOME");
    assert!(
        !std::path::Path::new(temporary_home).exists(),
        "manual-evidence.sh should remove its temporary ANY_SWITCH_HOME"
    );

    assert!(workflow.contains("scripts/package-release.sh"));
    assert!(release_doc.contains("app_definitions/builtin/*.yaml"));
    assert!(release_doc.contains("docs/evidence-followups.md"));
    assert!(release_doc.contains("scripts/manual-evidence.ps1"));
    assert!(release_doc.contains("powershell -NoProfile -ExecutionPolicy Bypass -File"));
    let powershell_evidence = fs::read_to_string(
        extract_dir
            .join("any-switch-v9.9.9-test-target")
            .join("scripts")
            .join("manual-evidence.ps1"),
    )
    .unwrap();
    assert!(powershell_evidence.contains("[Alias(\"h\")]"));
    assert!(powershell_evidence.contains("[switch]$Help"));
    assert!(powershell_evidence.contains("$PSScriptRoot"));
    assert!(powershell_evidence.contains("any-switch.exe"));
    assert!(powershell_evidence.contains("organizationName"));
    assert!(powershell_evidence.contains("<identity>"));
    assert!(powershell_evidence.contains("account=<identity>"));
    assert!(powershell_evidence.contains("## Evidence Summary"));
    assert!(powershell_evidence.contains("docs/evidence-followups.md"));
    assert!(powershell_evidence.contains("### Current-Stage Release Blockers"));
    assert!(powershell_evidence.contains("### Deferred Follow-Up Evidence"));
    assert!(powershell_evidence.contains("## Deferred Full-Coverage Experiments"));
    assert!(powershell_evidence.contains("## Windows Release Smoke Test"));
    assert!(powershell_evidence.contains("## Final Decision"));
}

#[test]
fn release_package_script_rejects_unsafe_artifact_names() {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for (tag, target) in [
        ("v9.9.9/bad", "test-target"),
        ("v9.9.9", "test target"),
        ("v9.9.9", "test/target"),
    ] {
        let output = std::process::Command::new("bash")
            .current_dir(&manifest_dir)
            .arg("scripts/package-release.sh")
            .arg(tag)
            .arg(target)
            .arg(env!("CARGO_BIN_EXE_any-switch"))
            .arg(manifest_dir.join(".test-invalid-release-package"))
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("may only contain"));
    }
}

fn collect_rust_files(dir: &std::path::Path, files: &mut Vec<std::path::PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect_rust_files(&path, files);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
}

fn assert_package_output_has_no_temporary_files(output_dir: &std::path::Path) {
    let leftovers = fs::read_dir(output_dir)
        .unwrap()
        .filter_map(|entry| {
            let name = entry.unwrap().file_name().to_string_lossy().to_string();
            (name.starts_with(".package-") || name.ends_with(".named")).then_some(name)
        })
        .collect::<Vec<_>>();
    assert!(
        leftovers.is_empty(),
        "package script should clean temporary files and directories: {leftovers:?}"
    );
}

fn assert_contains_all(haystack: &str, needles: &[&str], context: &str) {
    let missing = needles
        .iter()
        .copied()
        .filter(|needle| !haystack.contains(needle))
        .collect::<Vec<_>>();
    assert!(
        missing.is_empty(),
        "{context}; missing expected fragments: {missing:?}"
    );
}

fn assert_not_contains_any(haystack: &str, needles: &[&str], context: &str) {
    let present = needles
        .iter()
        .copied()
        .filter(|needle| haystack.contains(needle))
        .collect::<Vec<_>>();
    assert!(
        present.is_empty(),
        "{context}; forbidden fragments were present: {present:?}"
    );
}

fn assert_checkout_steps_do_not_persist_credentials(workflow: &serde_yaml::Value, path: &str) {
    let jobs = workflow["jobs"]
        .as_mapping()
        .unwrap_or_else(|| panic!("{path} missing jobs"));
    let mut checkout_steps = 0;
    for (_job_name, job) in jobs {
        let Some(steps) = job["steps"].as_sequence() else {
            continue;
        };
        for step in steps {
            if step["uses"].as_str() == Some("actions/checkout@v6") {
                checkout_steps += 1;
                assert_eq!(
                    step["with"]["persist-credentials"].as_bool(),
                    Some(false),
                    "{path} checkout step should set persist-credentials: false"
                );
            }
        }
    }
    assert!(checkout_steps > 0, "{path} should contain checkout steps");
}
