# Acceptance Evidence

This document maps the MVP acceptance criteria in `docs/design.md` section 13
to current implementation evidence. It is an audit aid, not a completion claim.

Status legend:

- `automated`: covered by source plus unit/integration tests in this repository.
- `partial`: implementation exists, but the evidence does not cover the full
  acceptance scope.
- `manual`: requires real app, OS, or release-environment verification outside
  the local test harness.

## Automated Gate

Run before claiming a release candidate:

```bash
scripts/verify-local.sh
```

`scripts/verify-local.sh` runs shell syntax checks, patch whitespace checks,
fmt, all tests, Clippy with warnings denied, release build, offline Cargo
source packaging, workflow YAML parsing through Rust tests, built-in Definition
validation, release packaging, checksum verification, and tar listing. CI runs
this gate on Linux x86_64 plus explicit
macOS Intel and macOS arm64 runners, and separately runs `cargo check
--all-targets`, Clippy with warnings denied, file-lock, username, and
process-name smoke tests, the Windows PowerShell evidence helper help path, and
release build plus release archive packaging for the Windows x86_64 target. CI
and release workflows use Node 24-compatible action versions directly instead
of relying on the temporary `FORCE_JAVASCRIPT_ACTIONS_TO_NODE24` runtime
override. CI uses per-ref concurrency and cancels stale duplicate runs; release
uses per-tag concurrency without cancellation so duplicate tag events cannot
race publishing. The Windows CI package smoke verifies that the archive contains
`any-switch.exe`. The release workflow repeats the same
gate before building tarballs, and first verifies that the pushed tag equals
`v<Cargo.toml package version>`. Each build matrix branch uploads workflow
artifacts only; a separate publish job waits for all targets, verifies every
expected tarball and checksum is present, and then uploads the complete set to
the GitHub Release. The Windows release matrix branch also runs the Windows
target Clippy, smoke tests, and evidence helper check before packaging
`any-switch.exe`.
Release archives are produced by `scripts/package-release.sh`;
`release_workflow_uploads_documented_binary_artifacts` verifies that tag builds
produce Linux x86_64, macOS x86_64, macOS arm64, and Windows x86_64 tarballs
plus checksums, stage them as workflow artifacts, and publish them to GitHub
Releases only after the complete target set exists. The
`release_archive_includes_auditable_builtin_definitions`
exercises the package script locally and verifies the archive contents,
including the user guide, read-only copies of the built-in App Definitions for
audit, and the manual evidence helper scripts. It also verifies that packaging
does not leave hidden staging directories behind, extracts the archive, runs the
packaged `any-switch --version` binary, runs
`scripts/manual-evidence.sh --help`, checks the packaged PowerShell evidence
helper wiring, and generates a redacted evidence file against the packaged
binary while verifying that the shell helper removes its temporary
`ANY_SWITCH_HOME`.

## Section 13 Criteria

| # | Status | Evidence |
|---|--------|----------|
| 1 | automated | `tests/cli.rs::add_rejects_sensitive_field_argument`, `add_rejects_invalid_field_keys`, `add_does_not_require_yes_and_rejects_unused_yes`, `show_redacts_secret_fields`, and static add/use coverage for Claude/Codex profiles. |
| 2 | partial | Claude OAuth import is definition-driven and includes `secret_entry` support with a fixture backend in `src/keychain.rs`; `claude_import_can_capture_macos_keychain_fixture` covers the built-in Keychain source on macOS CI/local macOS, `doctor_reports_definition_driven_non_secret_target_summary` covers doctor reporting for current-platform capture sources without leaking secret bytes, and `claude_import_uses_oauth_account_identity_when_credentials_are_opaque` records that Claude credential tokens are opaque while identity comes from `~/.claude.json.$.oauthAccount`. Current-stage real macOS Claude Code Keychain import evidence is manual release evidence and must be copied into the release checklist or another durable maintainer-visible record before publishing; this row remains `partial` because repository automation cannot prove the real local Keychain import by itself. |
| 3 | partial | Claude file-backed OAuth capture, `${CLAUDE_CONFIG_DIR:-~/.claude}` expansion, current-platform doctor capture-source reporting, and target identity consistency are covered by `claude_import_uses_claude_config_dir_for_file_backed_credentials`, `doctor_reports_definition_driven_non_secret_target_summary`, `doctor_json_reports_definition_summary_without_secret_values`, `claude_import_uses_oauth_account_identity_when_credentials_are_opaque`, and `claude_status_and_writeback_detect_oauth_account_identity_mismatch`; the built-in file source is explicitly scoped to Linux, and real Linux Claude capture remains manual. |
| 4 | automated | `codex_oauth_import_use_and_writeback`, `codex_auto_import_uses_chatgpt_auth_mode_even_when_config_has_model_fields`, `codex_import_accepts_legacy_api_key_without_mode_when_store_is_implicit`, `codex_import_accepts_legacy_api_key_without_mode_even_when_config_has_model_fields`, `codex_import_rejects_mixed_chatgpt_and_api_key_auth`, and credential-store guard tests. |
| 5 | partial | In-repo tests cover env/file/OAuth switching, OAuth cleanup, and rollback paths; real app restart confirmation on macOS, Linux, and Windows for each platform-supported app/profile kind is manual. Windows Claude OAuth capture remains excluded until that Definition source is enabled after separate evidence. |
| 6 | automated | `codex_oauth_import_use_and_writeback`, `oauth_identity_verify_failure_rolls_back_immediately`, `claude_status_and_writeback_detect_oauth_account_identity_mismatch`, and profile metadata immutability assertions around OAuth writeback. |
| 7 | automated | `status_reports_matched_with_overrides_without_leaking_env_secret`, `status_reports_missing_when_static_target_file_is_absent`, `status_no_active_includes_import_current_hint`, `status_rejects_unknown_app_filter`, `pending_switch_blocks_writes_and_status_reports_interrupted`, and OAuth drift tests. |
| 8 | automated | `use_oauth_records_stale_capture_warning` and `user_app_definition_can_drive_file_template` check dry-run JSON redaction while preserving identity summaries. |
| 9 | automated | Backup creation, restore, pruning, OAuth/TOML target coverage, and hardlink accounting are covered by restore/backup tests and `backup::tests::identical_backup_blobs_are_hardlinked_and_counted_once`. |
| 10 | automated | `restore_rejects_backup_with_hash_mismatch`, `restore_target_rejects_unsafe_backup_id_before_reading_manifest`, `restore_oauth_backup_ignores_allow_running_and_requires_assume_app_stopped`, `resolved_target_change_reports_drift_and_requires_acceptance`, and restore recovery tests. |
| 11 | automated | `process_probe_blocks_static_write_unless_allow_running`, `use_without_yes_requires_confirmation_in_non_tty`, `oauth_assume_app_stopped_requires_confirmation_and_can_escape_probe`, and `restore_oauth_backup_ignores_allow_running_and_requires_assume_app_stopped` cover default refusal, OAuth `--allow-running` rejection, ordinary `import-current --yes` rejection, non-interactive confirmation refusal, `--assume-app-stopped --yes`, and history warnings with PID/start time/command. |
| 12 | automated | Definition validation rejects executable login/reauth fields; no login/reauth command exists in `src/cli.rs`. |
| 13 | automated | `remove_deletes_profile_capture_and_clears_active_without_touching_live_target` and `remove_rejects_invalid_profile_id_without_deleting_outside_capture_dir`. |
| 14 | automated | Redaction tests for `list --json`, `show`, `status`, human and JSON `doctor`, dry-run JSON, and secret argv rejection; `list_rejects_unknown_app_filter` prevents app-filter typos from returning an empty, misleading list. |
| 15 | automated | `doctor_reports_permission_warnings` covers widened `profiles.yaml`, `apps.d`, `overrides.d`, `captures`, and `backups` permissions; secret file permission tests and private write tests in `src/paths.rs` cover write-side tightening/rejection. |
| 16 | automated | `paths::tests::current_os_home_ignores_home_env`, `current_os_user_ignores_user_env`, and `expands_defaulted_env_template_for_any_app_definition`. |
| 17 | automated | Core behavior is driven by App Definition handlers; built-in definitions are discovered from `src/app_definitions/builtin/*.yaml`, `production_source_does_not_branch_on_builtin_app_ids` rejects built-in app id literals in production Rust source, `doctor_uses_user_definition_without_builtin_app_assumptions` covers doctor on a user-defined app, `doctor_reports_definition_driven_non_secret_target_summary` covers Definition-driven JSON object schema warnings for upgraded app state, and `user_app_definition_can_drive_file_template` exercises add/use/status/doctor/backup/restore/remove for a non-built-in app. |
| 18 | automated | `user_app_definition_can_drive_env_injection`, `user_app_definition_can_drive_file_template`, and `user_app_definition_can_drive_toml_managed_paths` cover user-defined handlers; the file-template case also verifies lifecycle commands remain generic rather than built-in-app specific. |
| 19 | automated | `definition_executable_login_field_is_rejected`, `definition_target_inside_switch_home_is_rejected`, `app_definitions::tests::rejects_reserved_opaque_capture_until_implemented`, and app definition validation unit tests for handlers and paths. |
| 20 | automated | `override_appends_process_probe_and_field_defaults` covers `apps` plus human/JSON `apps show`; `override_cannot_replace_targets_or_handlers` covers the override safety boundary; `apps_export_respects_source_and_override_output_rules` covers system/resolved export, override skeleton generation, `apps validate`, and output overwrite refusal; `diagnostics_commands_work_when_installed_registry_is_invalid` keeps `config path` and standalone `apps validate <file>` usable when installed definitions are broken. |
| 21 | automated | README Safety Notes, `docs/user-guide.md`, and `docs/design.md` section 11.4 state the OAuth stop-app requirement and recommend stopping apps for static writes. |
| 22 | automated | `codex_oauth_import_use_and_writeback` asserts OAuth writeback does not mutate `profiles.yaml`; `use_and_status_do_not_modify_profiles_yaml_for_static_profiles` asserts static `use` and `status` preserve `profiles.yaml` bytes and mtime; `restore_target_prunes_backups_after_success` asserts `restore-target` also preserves `profiles.yaml`; write paths are confined to profile management commands. |
| 23 | automated | `codex_oauth_toml_capture_only_restores_managed_paths`, `handlers::tests::toml_managed_paths_keep_unknown_table`, and user TOML definition tests. |
| 24 | automated | `pending_switch_blocks_writes_and_status_reports_interrupted`, `pending_applying_with_backup_rolls_back_before_next_write`, and `pending_use_applying_commits_when_live_matches_target_profile`. |
| 25 | automated | `remove_does_not_delete_profile_or_capture_when_app_is_locked` and remove lock ordering in `src/cli.rs`. |
| 26 | automated | `detach_clears_active_without_touching_profile_capture_backup_or_live_target` covers profile/capture/backup/live-target preservation, active clearing, structured history `from_profile`, and the explicit import-current-not-use hint; `detach_rejects_invalid_app_id_before_creating_lock_path` covers app id path safety for detach locks; `detach_rejects_unknown_app_without_writing_active_state` covers unknown-app state pollution; `status_no_active_includes_import_current_hint` covers the no-active hint. |
| 27 | automated | Identity validation unit tests, `oauth_optional_identity_mismatch_warns_without_blocking`, `oauth_identity_verify_failure_rolls_back_immediately`, and `rejects_oauth_definition_without_process_probe`. |
| 28 | automated | JSON writer tests plus `restore_claude_oauth_backup_restores_json_subtrees_not_whole_file` cover minified/pretty preservation and unmanaged subtree order. |
| 29 | automated | Backup hardlink unit test and `doctor_reports_backup_usage` / `doctor_warns_when_backup_usage_exceeds_soft_limit` / `doctor_backup_usage_ignores_unsafe_manifest_stored_as_paths`. |
| 30 | automated | `definition_target_inside_switch_home_is_rejected` and path boundary unit tests. |
| 31 | automated | `target_lock_waits_for_different_apps_pointing_to_same_file` and `import_current_respects_target_locks`. |
| 32 | automated | `restore_target_bookkeeping_recovery_does_not_update_active` and `restore_target_applying_recovery_commits_when_live_matches_restore_backup`. |
| 33 | automated | Status tests for managed overrides, `doctor_json_reports_definition_summary_without_secret_values` for definition-driven doctor override diagnostics without leaking secret values, plus TOML managed surface tests. |
| 34 | automated | App definition validation tests reject OAuth definitions without process probes and invalid JSON paths. |
| 35 | automated | `edit_rejects_immutable_field_changes`, `edit_does_not_update_profile_or_open_editor_when_app_is_locked`, `add_force_refuses_to_replace_profile_when_app_has_pending_switch`, and `add_force_does_not_replace_profile_when_app_is_locked` cover immutable-field validation, pending-switch refusal, and same-App lock contention before profile mutation. |
| 36 | automated | `state_lock_waits_and_preserves_entries_for_concurrent_app_bookkeeping` covers state lock serialization; `state::tests::active_state_rejects_invalid_ids` and `status_rejects_invalid_active_state_ids` cover active state id validation before status/bookkeeping proceeds; `state::tests::pending_state_rejects_invalid_or_mismatched_contents` covers pending journal content validation before recovery/status bookkeeping proceeds. |
| 37 | automated | `import_current_marks_profile_active_with_resolved_targets` and `resolved_target_change_reports_drift_and_requires_acceptance`. |
| 38 | automated | `use_reports_missing_optional_capture_blob_recorded_in_manifest`, `use_oauth_rejects_missing_capture_before_pending_or_backup`, and `doctor_reports_missing_oauth_capture`, which covers `status`, human `doctor`, and JSON `doctor` reporting for missing current-platform capture blobs. |
| 39 | automated | `doctor_warns_when_switch_home_is_under_cloud_sync_root`. |
| 40 | automated | `status_no_active_includes_import_current_hint` and `detach_clears_active_without_touching_profile_capture_backup_or_live_target` cover the explicit import-current-not-use hint. |
| 41 | automated | `codex_auto_import_uses_chatgpt_auth_mode_even_when_config_has_model_fields`, `codex_import_accepts_legacy_api_key_without_mode_when_store_is_implicit`, and `codex_import_accepts_legacy_api_key_without_mode_even_when_config_has_model_fields`. |

## Required Manual Evidence

These items cannot be proven solely by the repository tests. The executable
checklist is in `docs/manual-verification.md`; use
`docs/manual-evidence-template.md` to record release-candidate evidence.
Deferred items are tracked in `docs/evidence-followups.md` until they have
maintainer-visible evidence.

For the current release stage, the macOS Claude OAuth import evidence is
release-blocking. Broader restart, risk-experiment, Linux, and Windows evidence
remains required before claiming full section 13 coverage, but it may be
deferred to follow-up release work if the release notes clearly state that this
is a macOS-evidenced stage release.

| Item | Stage | Required evidence |
|------|-------|-------------------|
| 2 | Current-stage blocker | Real macOS Claude Code OAuth import from Keychain plus `~/.claude.json`, with captured identity fields verified. |
| 5-macOS | Deferred | Restart relevant apps after switching profiles on macOS for each macOS-supported built-in app/profile kind, then confirm the active account/provider/model matches the selected profile. |
| A-macOS | Deferred | Claude refresh token rotation experiment on macOS: capture before/after refresh and verify whether old captures remain usable. |
| B-macOS | Deferred | Claude Keychain / `oauthAccount` mismatch experiment on macOS: modify only one source and record Claude Code startup behavior. |
| C-macOS | Deferred | Claude runtime write-frequency and JSON-format sampling for `~/.claude.json` on macOS. |
| E-macOS | Deferred | Codex external restore flow on macOS: restore state outside any-switch, then confirm `import-current` captures or refreshes the intended profile. |
| 3 | Deferred | Real Linux Claude OAuth import from `${CLAUDE_CONFIG_DIR:-~/.claude}/.credentials.json` plus `~/.claude.json`. |
| 5-Linux | Deferred | Restart relevant apps after switching profiles on Linux for each Linux-supported built-in app/profile kind, then confirm the active account/provider/model matches the selected profile. |
| 5-Windows | Deferred | Restart relevant apps after switching profiles on Windows for each Windows-supported built-in app/profile kind, then confirm the active account/provider/model matches the selected profile. Windows Claude OAuth capture is not required until the built-in Definition enables that source. |
| W | Deferred | Windows release smoke: verify checksum, extract `x86_64-pc-windows-msvc` archive, and run `any-switch.exe --version`, `apps`, and `doctor`; this covers release packaging and startup, not Windows Credential Manager or Claude Windows OAuth capture support. |

Item D is already recorded in `docs/design.md` as the Codex CLI 0.133.0
file-backed `auth.json` schema observation.
