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
    assert_eq!(workflow["permissions"]["contents"].as_str(), Some("write"));

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
            "x86_64-unknown-linux-gnu"
        ]
    );
    for target in &targets {
        assert!(
            release_doc.contains(target),
            "docs/release.md must document release target {target}"
        );
    }

    let steps = workflow["jobs"]["build"]["steps"].as_sequence().unwrap();
    assert!(steps.iter().any(|step| step["run"]
        .as_str()
        .is_some_and(|run| run.contains("scripts/package-release.sh"))));
    let release_step = steps
        .iter()
        .find(|step| {
            step["uses"]
                .as_str()
                .is_some_and(|uses| uses.starts_with("softprops/action-gh-release@"))
        })
        .expect("release workflow must upload artifacts to a GitHub release");
    let files = release_step["with"]["files"].as_str().unwrap();
    assert!(files.contains("any-switch-${{ github.ref_name }}-${{ matrix.target }}.tar.gz"));
    assert!(files.contains("any-switch-${{ github.ref_name }}-${{ matrix.target }}.tar.gz.sha256"));
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
        "any-switch-v9.9.9-test-target/CONTRIBUTING.md",
        "any-switch-v9.9.9-test-target/SECURITY.md",
        "any-switch-v9.9.9-test-target/LICENSE-APACHE",
        "any-switch-v9.9.9-test-target/LICENSE-MIT",
        "any-switch-v9.9.9-test-target/docs/design.md",
        "any-switch-v9.9.9-test-target/docs/release.md",
        "any-switch-v9.9.9-test-target/docs/acceptance.md",
        "any-switch-v9.9.9-test-target/docs/manual-verification.md",
        "any-switch-v9.9.9-test-target/docs/manual-evidence-template.md",
        "any-switch-v9.9.9-test-target/scripts/manual-evidence.sh",
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
    assert!(evidence.contains("ANY_SWITCH_HOME` note: temporary; removed when this script exits"));
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
