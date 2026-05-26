# Manual Verification

These checks collect the real app and OS evidence that cannot be proven by the
repository test harness. Run them before declaring the MVP complete or cutting a
release candidate that claims full `docs/design.md` section 13 coverage.

Record results in `docs/manual-evidence-template.md` or an issue / release note
that preserves the same fields. Redact secret values and capture blob contents.

Use a dedicated test account and a temporary switch home:

```bash
export SWITCH_CLI_HOME="$HOME/.switch-cli-manual-verify"
switch-cli doctor
```

Record the operating system, target app versions, `switch-cli --version`, and
the exact command output snippets needed to prove each item.

To initialize an evidence file with read-only diagnostics, run:

```bash
SWITCH_CLI_BIN=target/release/switch-cli \
  scripts/manual-evidence.sh manual-evidence-$(date -u +%Y%m%dT%H%M%SZ).md
```

The generated file does not perform imports, switches, restores, or any other
write operation. It captures environment and `doctor` / `status` output as a
starting point, with email addresses and UUID-like identifiers redacted; the
script refuses to overwrite an existing evidence file. If `SWITCH_CLI_HOME` is
not set, the script uses a temporary switch home under the current user's home
directory and removes it on exit, so it does not initialize `~/.switch-cli`.
Set `SWITCH_CLI_HOME` explicitly when you want the diagnostics to inspect an
existing switch-cli state directory. The real app experiments below still
require manual execution and review.

Files named `manual-evidence-*.md` are ignored by Git to reduce accidental
publication of local release-candidate evidence. Store final evidence in a
private issue or release checklist unless the project explicitly decides to
publish a redacted record.

## Claude OAuth Import On macOS

Purpose: covers acceptance item 2 and preflight items A, B, and C.

Prerequisites:

- macOS with Claude Code installed.
- Claude Code is logged in with OAuth.
- Claude Code is fully quit before each `switch-cli` OAuth command.

Steps:

1. Confirm Claude Code is not running:

   ```bash
   switch-cli doctor claude
   ```

   Passing evidence: no `process` rows for Claude.

2. Import the current OAuth state:

   ```bash
   switch-cli import-current claude manual-macos --kind oauth_capture
   switch-cli show claude-manual-macos
   switch-cli status claude
   ```

   Passing evidence: profile kind is `oauth_capture`, required identity fields
   are present, and `status` is `matched`.

3. Refresh-token rotation experiment:

   - Save hashes of `captures/claude-manual-macos/*` and `manifest.json`.
   - Start Claude Code and use it long enough to trigger token refresh.
   - Quit Claude Code.
   - Run `switch-cli use claude-manual-macos --yes`.
   - Compare capture hashes and `manifest.json`.

   Passing evidence: writeback either records new bytes safely or proves the
   old capture remains usable after restore; note the observed behavior.

4. Keychain / `oauthAccount` mismatch experiment:

   - Back up the current `~/.claude.json` and Keychain entry externally.
   - Modify only one side, leaving the other unchanged.
   - Start Claude Code and record whether it auto-corrects, shows stale UI, or
     fails.
   - Restore the external backup, then re-run `switch-cli import-current`.

   Passing evidence: observed behavior is recorded and does not contradict the
   source-consistency checks in `switch-cli status` / `switch-cli use`.

5. Runtime JSON sampling:

   - While Claude Code is running, monitor `~/.claude.json` writes and note
     whether `oauthAccount` / `userID` are written with other fields.
   - Capture whether Claude writes minified JSON or pretty JSON, the indent
     width, trailing newline behavior, and top-level key order.

   Passing evidence: the sampled format is compatible with the JSON
   preservation behavior implemented by switch-cli, or a follow-up issue exists
   for any mismatch.

## Claude OAuth Import On Linux

Purpose: covers acceptance item 3.

Prerequisites:

- Linux with Claude Code configured for file-backed credentials.
- Claude Code is logged in with OAuth and fully stopped.

Steps:

1. Import the current OAuth state:

   ```bash
   switch-cli import-current claude manual-linux --kind oauth_capture
   switch-cli show claude-manual-linux
   switch-cli status claude
   ```

2. Verify the capture directory contains the current-platform credential file
   and `manifest.json`:

   ```bash
   find "$SWITCH_CLI_HOME/captures/claude-manual-linux" -maxdepth 1 -type f -print
   ```

Passing evidence: required identity fields are present, current-platform capture
blobs exist, and `status` is `matched`.

## Profile Switch Restart Smoke Test

Purpose: covers acceptance item 5.

Run once on macOS and once on Linux for each supported app available on that
platform.

Steps:

1. Create or import two profiles for the same app.
2. Quit the app.
3. Switch to profile A:

   ```bash
   switch-cli use <profile-a> --yes
   switch-cli status <app>
   ```

4. Start the app and verify the visible account/provider/model matches profile
   A.
5. Quit the app.
6. Switch to profile B and repeat the verification.

Passing evidence: app-visible state matches the selected profile after restart
for both directions.

## Codex External Restore Flow

Purpose: covers preflight item E.

Prerequisites:

- Codex CLI installed.
- File-backed `${CODEX_HOME:-~/.codex}/auth.json` state available.

Steps:

1. Import a Codex OAuth or API-key profile:

   ```bash
   switch-cli import-current codex manual-codex --kind auto
   ```

2. Change Codex authentication outside switch-cli, then restore the intended
   state outside switch-cli as a user would.
3. Run import-current again:

   ```bash
   switch-cli import-current codex manual-codex-refresh --kind auto
   switch-cli status codex
   ```

Passing evidence: switch-cli either updates the matching profile by required
identity or creates a new profile for a genuinely different identity, and
`status` reports the expected state.
