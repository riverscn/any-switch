# Manual Verification Evidence

Use this file as the release-candidate evidence record for the real app checks
in `docs/manual-verification.md`. Attach command output snippets, redact secret
values, and keep enough context for another maintainer to reproduce the result.
You can initialize a local evidence file with `scripts/manual-evidence.sh`, or
with `scripts/manual-evidence.ps1` on Windows. The scripts only run read-only
diagnostics and redact email addresses, UUID-like identifiers, and common JSON
identity names, and Keychain account labels, and they refuse to overwrite an
existing evidence file. Unless `ANY_SWITCH_HOME` is already set, they use a
temporary switch home and remove it on exit. Complete the manual experiment sections below afterwards. Generated files named
`manual-evidence-*.md` are ignored by Git; keep final release evidence in a
private issue or release checklist unless the project explicitly chooses to
publish a redacted record.

## Environment

- Date:
- Operator:
- OS and version:
- CPU architecture:
- `any-switch --version`:
- Git commit:
- Claude Code version:
- Codex CLI version:
- `ANY_SWITCH_HOME` used:

## Local Gate

- [ ] `scripts/verify-local.sh` passed.

Evidence:

```text
paste command summary here
```

## Evidence Summary

Use `pending`, `passed`, `failed`, or `skipped: <linked issue>` as the status.
For the current release stage, the macOS Claude OAuth import evidence is
release-blocking. Broader restart, risk-experiment, Linux, and Windows evidence
is deferred and must have follow-up tracking before this stage is released. Use
`docs/evidence-followups.md` for in-repository tracking until a repository issue
or release checklist exists. Do not claim full section 13 coverage while any
deferred item is still pending, failed, or skipped.

### Current-Stage Release Blockers

| Item | Status | Evidence location or follow-up issue |
|------|--------|---------------------------------------|
| 2: macOS Claude OAuth import | pending | |

### Deferred Follow-Up Evidence

| Item | Status | Evidence location or follow-up issue |
|------|--------|---------------------------------------|
| 5-macOS: restart smoke tests | pending | `docs/evidence-followups.md` |
| A-macOS: Claude refresh-token rotation | pending | `docs/evidence-followups.md` |
| B-macOS: Claude source mismatch behavior | pending | `docs/evidence-followups.md` |
| C-macOS: Claude runtime JSON sampling | pending | `docs/evidence-followups.md` |
| E-macOS: Codex external restore flow | pending | `docs/evidence-followups.md` |
| 3: Linux Claude OAuth import | pending | `docs/evidence-followups.md` |
| 5-Linux: restart smoke tests | pending | `docs/evidence-followups.md` |
| 5-Windows: restart smoke tests | pending | `docs/evidence-followups.md` |
| W: Windows release smoke | pending | `docs/evidence-followups.md` |

## Item 2: Claude OAuth Import On macOS

- [ ] Claude Code fully quit before OAuth commands.
- [ ] `any-switch doctor claude` showed no Claude process rows, or only a
      documented process-probe warning unrelated to a running Claude app.
- [ ] `any-switch import-current claude manual-macos --kind oauth_capture`
      succeeded.
- [ ] `any-switch show claude-manual-macos` showed `oauth_capture` and required
      identity fields.
- [ ] `any-switch status claude` reported `matched`.

Evidence:

```text
paste redacted command output here
```

## Deferred Full-Coverage Experiments

The following sections are not current-stage release blockers. Complete them
before claiming full `docs/design.md` section 13 coverage, or keep their
follow-up tracking current in `docs/evidence-followups.md`.

## Preflight A: Claude Refresh Token Rotation

- [ ] Capture hashes recorded before Claude Code refresh.
- [ ] Claude Code used long enough to trigger refresh, then quit.
- [ ] `any-switch use claude-manual-macos` was confirmed by typing `yes`, then
      completed or failed with the expected safety error.
- [ ] Capture hash / manifest behavior recorded.

Conclusion:

```text
write observed rotation behavior here
```

## Preflight B: Claude Keychain / oauthAccount Mismatch

- [ ] External backups created before mutation.
- [ ] Only one side was modified.
- [ ] Claude Code startup behavior recorded.
- [ ] External backups restored.
- [ ] `any-switch import-current claude ... --kind oauth_capture` behavior
      recorded.

Conclusion:

```text
write observed mismatch behavior here
```

## Preflight C: Claude Runtime JSON Sampling

- [ ] Runtime writes to `~/.claude.json` observed.
- [ ] `oauthAccount` write grouping recorded, and whether top-level `userID`
      changes independently was noted.
- [ ] JSON formatting, newline behavior, and key order recorded.

Conclusion:

```text
write sampled JSON behavior here
```

## Item 3: Claude OAuth Import On Linux

- [ ] Claude Code fully stopped.
- [ ] `any-switch import-current claude manual-linux --kind oauth_capture`
      succeeded.
- [ ] `any-switch show claude-manual-linux` showed `oauth_capture` and required
      identity fields.
- [ ] `any-switch status claude` reported `matched`.
- [ ] `captures/claude-manual-linux/credentials.json` and `manifest.json`
      existed.

Evidence:

```text
paste redacted command output here
```

## Item 5: Restart Smoke Tests

Run on macOS, Linux, and Windows for each supported app/profile kind available
on that platform.

| OS | App | Profile A | Profile B | A visible after restart | B visible after restart | Pass |
|----|-----|-----------|-----------|--------------------------|--------------------------|------|
|    |     |           |           |                          |                          |      |

Evidence:

```text
paste redacted command output and app-visible observations here
```

## Windows Release Smoke Test

- [ ] Windows archive checksum verified.
- [ ] Archive extracted and contained `any-switch.exe`.
- [ ] `any-switch.exe --version` succeeded.
- [ ] `any-switch.exe apps` succeeded.
- [ ] `any-switch.exe doctor` succeeded without packaging/startup failure.

Evidence:

```text
paste redacted command output here
```

## Preflight E: Codex External Restore Flow

- [ ] Initial Codex profile imported.
- [ ] Codex auth changed outside any-switch.
- [ ] Intended state restored outside any-switch.
- [ ] `any-switch import-current codex manual-codex-refresh --kind auto`
      succeeded.
- [ ] `any-switch status codex` reported the expected state.

Conclusion:

```text
write observed import/refresh behavior here
```

## Final Decision

- [ ] Every current-stage release blocker is marked `passed`.
- [ ] Deferred Linux / Windows evidence has linked follow-up tracking.
- [ ] Any `failed` or `skipped` item has a linked follow-up issue.
- [ ] Release notes describe this as a macOS-evidenced stage release and do not
      claim full `docs/design.md` section 13 coverage until deferred evidence
      is complete.

Decision notes:

```text
write final release decision here
```
