# Manual Verification Evidence

Use this file as the release-candidate evidence record for the real app checks
in `docs/manual-verification.md`. Attach command output snippets, redact secret
values, and keep enough context for another maintainer to reproduce the result.
You can initialize a local evidence file with `scripts/manual-evidence.sh`; the
script only runs read-only diagnostics and redacts email / UUID-like identifiers,
and it refuses to overwrite an existing evidence file. Unless `ANY_SWITCH_HOME`
is already set, it uses a temporary switch home and removes it on exit. Complete
the manual experiment sections below afterwards. Generated files named
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

## Preflight A: Claude Refresh Token Rotation

- [ ] Capture hashes recorded before Claude Code refresh.
- [ ] Claude Code used long enough to trigger refresh, then quit.
- [ ] `any-switch use claude-manual-macos --yes` completed or failed with the
      expected safety error.
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

Run on macOS and Linux for each supported app available on that platform.

| OS | App | Profile A | Profile B | A visible after restart | B visible after restart | Pass |
|----|-----|-----------|-----------|--------------------------|--------------------------|------|
|    |     |           |           |                          |                          |      |

Evidence:

```text
paste redacted command output and app-visible observations here
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

- [ ] All required manual evidence passed.
- [ ] Any deviations have linked follow-up issues.
- [ ] Release candidate can claim full `docs/design.md` section 13 coverage.

Decision notes:

```text
write final release decision here
```
