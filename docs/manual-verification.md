# Manual Verification

These checks collect the real app and OS evidence that cannot be proven by the
repository test harness. For the current stage, the macOS Claude OAuth import
evidence is sufficient to cut a release candidate as long as the release notes
do not claim full `docs/design.md` section 13 coverage. Broader macOS restart
and risk experiments, plus Linux and Windows real-app evidence, are deferred to
follow-up release work.

Record results in `docs/manual-evidence-template.md` or an issue / release note
that preserves the same fields. Redact secret values and capture blob contents.
Keep the "Evidence Summary" table current while you work: mark each item
passed, failed, skipped with reason, or linked to a follow-up issue. Use
`docs/evidence-followups.md` as the durable tracker for deferred items until
they move to a repository issue or release checklist. The current stage may
release when the current-stage macOS Claude OAuth import blocker is passed and
deferred follow-up evidence has tracking. Do not claim full section 13 coverage
while any deferred item is still pending.

Use a dedicated test account and a temporary any-switch home:

```bash
export ANY_SWITCH_HOME="$HOME/.any-switch-manual-verify"
any-switch doctor
```

Record the operating system, target app versions, `any-switch --version`, and
the exact command output snippets needed to prove each item.

To initialize an evidence file with read-only diagnostics, run:

```bash
ANY_SWITCH_BIN=target/release/any-switch \
  scripts/manual-evidence.sh manual-evidence-$(date -u +%Y%m%dT%H%M%SZ).md
```

The generated file does not perform imports, switches, restores, or any other
write operation. It captures environment and `doctor` / `status` output as a
starting point, with email addresses, UUID-like identifiers, and common JSON
identity names, and Keychain account labels redacted; the script refuses to
overwrite an existing evidence file. If `ANY_SWITCH_HOME` is not set, the script
uses a temporary any-switch home under the current user's home directory and
removes it on exit, so it does not initialize `~/.any-switch`.
Set `ANY_SWITCH_HOME` explicitly when you want the diagnostics to inspect an
existing any-switch state directory. The real app experiments below still
require manual execution and review.

Files named `manual-evidence-*.md` are ignored by Git to reduce accidental
publication of local release-candidate evidence. Store final evidence in a
private issue or release checklist unless the project explicitly decides to
publish a redacted record.

## Claude OAuth Import On macOS

Purpose: the current-stage release blocker covers acceptance item 2. The
refresh-token rotation, source-mismatch, and runtime JSON sampling experiments
below cover deferred full section 13 evidence items A, B, and C.

Prerequisites:

- macOS with Claude Code installed.
- Claude Code is logged in with OAuth.
- Claude Code is fully quit before each `any-switch` OAuth command.

Current-stage blocker steps:

1. Confirm Claude Code is not running:

   ```bash
   any-switch doctor claude
   ```

   Passing evidence: no `process` rows for Claude.

2. Import the current OAuth state:

   ```bash
   any-switch import-current claude manual-macos --kind oauth_capture
   any-switch show claude-manual-macos
   any-switch status claude
   ```

   Passing evidence: profile kind is `oauth_capture`, required identity fields
   are present, and `status` is `matched`.

Deferred full-coverage experiments:

3. Refresh-token rotation experiment:

   - Save hashes of `captures/claude-manual-macos/*` and `manifest.json`.
   - Start Claude Code and use it long enough to trigger token refresh.
   - Quit Claude Code.
   - Run `any-switch use claude-manual-macos` and confirm by typing `yes`.
   - Compare capture hashes and `manifest.json`.

   Passing evidence: writeback either records new bytes safely or proves the
   old capture remains usable after restore; note the observed behavior.

4. Keychain / `oauthAccount` mismatch experiment:

   - Back up the current `~/.claude.json` and Keychain entry externally.
   - Modify only one side, leaving the other unchanged.
   - Start Claude Code and record whether it auto-corrects, shows stale UI, or
     fails.
   - Restore the external backup, then re-run `any-switch import-current`.

   Passing evidence: observed behavior is recorded and does not contradict the
   source-consistency checks in `any-switch status` / `any-switch use`.

5. Runtime JSON sampling:

   - While Claude Code is running, monitor `~/.claude.json` writes and note
     whether `oauthAccount` is written with other fields, and whether top-level
     `userID` changes independently.
   - Capture whether Claude writes minified JSON or pretty JSON, the indent
     width, trailing newline behavior, and top-level key order.

   Passing evidence: the sampled format is compatible with the JSON
   preservation behavior implemented by any-switch, or a follow-up issue exists
   for any mismatch.

## Claude OAuth Import On Linux

Purpose: covers acceptance item 3.

Prerequisites:

- Linux with Claude Code configured for file-backed credentials.
- Claude Code is logged in with OAuth and fully stopped.

Steps:

1. Import the current OAuth state:

   ```bash
   any-switch import-current claude manual-linux --kind oauth_capture
   any-switch show claude-manual-linux
   any-switch status claude
   ```

2. Verify the capture directory contains the current-platform credential file
   and `manifest.json`:

   ```bash
   find "$ANY_SWITCH_HOME/captures/claude-manual-linux" -maxdepth 1 -type f -print
   ```

Passing evidence: required identity fields are present, current-platform capture
blobs exist, and `status` is `matched`.

## Profile Switch Restart Smoke Test

Purpose: covers acceptance item 5.

Run once on macOS, Linux, and Windows for each supported app/profile kind
available on that platform.

Steps:

1. Create or import two profiles for the same app.
2. Quit the app.
3. Switch to profile A:

   ```bash
   any-switch use <profile-a>
   any-switch status <app>
   ```

4. Confirm the switch by typing `yes`, then start the app and verify the visible
   account/provider/model matches profile A.
5. Quit the app.
6. Switch to profile B and repeat the verification.

Passing evidence: app-visible state matches the selected profile after restart
for both directions.

## Windows Release Smoke Test

Purpose: verifies the Windows release archive shape and basic executable
behavior. This does not imply support for Windows Credential Manager or every
OAuth backend; the bundled Claude OAuth file source is Linux-scoped until
Windows Claude credentials behavior is confirmed separately.

Prerequisites:

- Windows x86_64 machine or runner.
- Downloaded `any-switch-<tag>-x86_64-pc-windows-msvc.tar.gz` and matching
  `.sha256` file from the GitHub Release.

Steps:

1. Verify the checksum:

   ```powershell
   $archive = ".\any-switch-<tag>-x86_64-pc-windows-msvc.tar.gz"
   $expected = ((Get-Content "${archive}.sha256") -split "\s+")[0].ToLowerInvariant()
   $actual = (Get-FileHash $archive -Algorithm SHA256).Hash.ToLowerInvariant()
   if ($actual -ne $expected) { throw "checksum mismatch" }
   ```

2. Extract the archive:

   ```powershell
   tar -xzf .\any-switch-<tag>-x86_64-pc-windows-msvc.tar.gz
   ```

3. Run:

   ```powershell
   .\any-switch-<tag>-x86_64-pc-windows-msvc\any-switch.exe --version
   .\any-switch-<tag>-x86_64-pc-windows-msvc\any-switch.exe apps
   .\any-switch-<tag>-x86_64-pc-windows-msvc\any-switch.exe doctor
   ```

4. Optional: initialize a redacted evidence file from the extracted archive:

   ```powershell
   powershell -NoProfile -ExecutionPolicy Bypass -File `
     .\any-switch-<tag>-x86_64-pc-windows-msvc\scripts\manual-evidence.ps1 `
     manual-evidence-<tag>-windows.md
   ```

Passing evidence: commands exit successfully, output is redacted, and `doctor`
does not report a packaging or startup failure.

## Codex External Restore Flow

Purpose: covers preflight item E.

Prerequisites:

- Codex CLI installed.
- File-backed `${CODEX_HOME:-~/.codex}/auth.json` state available.

Steps:

1. Import a Codex OAuth or API-key profile:

   ```bash
   any-switch import-current codex manual-codex --kind auto
   ```

2. Change Codex authentication outside any-switch, then restore the intended
   state outside any-switch as a user would.
3. Run import-current again:

   ```bash
   any-switch import-current codex manual-codex-refresh --kind auto
   any-switch status codex
   ```

Passing evidence: any-switch either updates the matching profile by required
identity or creates a new profile for a genuinely different identity, and
`status` reports the expected state.
