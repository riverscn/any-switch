# Deferred Evidence Tracker

This tracker keeps the manual evidence that is not required for the current
macOS-evidenced stage release visible and actionable. These items remain
required before any release note, README, or acceptance report claims full
`docs/design.md` section 13 coverage.

Use the `Tracking` column for the durable issue, release checklist item, or
maintainer-owned task that will collect the final evidence. The repository
includes `.github/ISSUE_TEMPLATE/release_checklist.yml` for that release
checklist. Replace `tracked here` with a repository issue or release checklist
link when one exists. Local ignored `manual-evidence-*.md` files may prove an
item for the maintainer, but they are not durable project tracking until their
redacted result is copied into a release checklist, issue, or other retained
evidence record. The checklist records evidence and decisions; it does not
replace `scripts/verify-local.sh`, GitHub Actions release artifacts, or
post-release checksum verification.

| Item | Status | Tracking | Completion evidence |
|------|--------|----------|---------------------|
| 5-macOS: restart smoke tests | pending | tracked here | Restart relevant apps after switching profiles on macOS for each macOS-supported built-in app/profile kind, then confirm the active account/provider/model matches the selected profile. |
| A-macOS: Claude refresh-token rotation | pending | tracked here | Capture before/after refresh and verify whether old captures remain usable or writeback records new bytes safely. |
| B-macOS: Claude source mismatch behavior | pending | tracked here | Modify only one of Claude Keychain or `oauthAccount`, record Claude Code startup behavior, restore external backups, and re-import. |
| C-macOS: Claude runtime JSON sampling | pending | tracked here | Record `~/.claude.json` write frequency, whether `oauthAccount` changes with top-level `userID`, formatting, trailing newline, and key order. |
| E-macOS: Codex external restore flow | passed locally; pending durable link | local redacted evidence retained by maintainer; copy into release checklist before full coverage claim | Restore Codex state outside any-switch, then confirm `import-current` captures or refreshes the intended profile. |
| 3: Linux Claude OAuth import | pending | tracked here | Import real Linux Claude OAuth state from `${CLAUDE_CONFIG_DIR:-~/.claude}/.credentials.json` plus `~/.claude.json`, then verify identity and `matched` status. |
| 5-Linux: restart smoke tests | pending | tracked here | Restart relevant apps after switching profiles on Linux for each Linux-supported built-in app/profile kind, then confirm the active account/provider/model matches the selected profile. |
| 5-Windows: restart smoke tests | pending | tracked here | Restart relevant apps after switching profiles on Windows for each Windows-supported built-in app/profile kind, then confirm the active account/provider/model matches the selected profile. |
| W: Windows release smoke | pending | tracked here | Verify checksum, extract `x86_64-pc-windows-msvc` archive, and run `any-switch.exe --version`, `apps`, and `doctor`. This does not imply Windows Credential Manager or Claude Windows OAuth capture support. |

When an item passes, copy the redacted command output or maintainer-visible
evidence link into the `Tracking` column and mark the item `passed`. If local
ignored evidence exists but has not been copied into durable release tracking,
leave the status as pending a durable link. If an item is intentionally skipped,
use `skipped: <issue-or-checklist-link>` and explain why the full section 13
claim remains blocked.
